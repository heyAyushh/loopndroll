// allow: SIZE_OK — transcript preview scanner keeps tail/full-scan fallback and timestamp extraction in one consistency boundary.
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const ASSISTANT_MESSAGE_MAX_CHARS: usize = 600;
const FIRST_USER_PROMPT_MAX_CHARS: usize = 600;
const RESPONSE_ITEM_RECORD_TYPE: &str = "response_item";
const EVENT_MESSAGE_RECORD_TYPE: &str = "event_msg";
const MESSAGE_PAYLOAD_TYPE: &str = "message";
const USER_MESSAGE_PAYLOAD_TYPE: &str = "user_message";
const ASSISTANT_ROLE: &str = "assistant";
const USER_ROLE: &str = "user";
const OUTPUT_TEXT_CONTENT_TYPE: &str = "output_text";
const TEXT_CONTENT_TYPE: &str = "text";
const ELLIPSIS_CHARS: usize = 3;
const BYTES_PER_KIB: u64 = 1024;
const TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES: u64 = 64 * BYTES_PER_KIB;
/// The real first prompt sits at the head of the transcript, usually after
/// injected instruction blocks, so the head window is larger than the tail's.
const FIRST_USER_PROMPT_HEAD_SCAN_BYTES: u64 = 256 * BYTES_PER_KIB;
/// User-role records Codex injects around real prompts (AGENTS.md payloads,
/// skill bodies, interrupt markers, goal/environment context); these must
/// never surface as the session's first prompt.
const INJECTED_USER_TEXT_PREFIXES: &[&str] = &[
    "# AGENTS.md instructions",
    "<user_instructions",
    "<environment_context",
    "<skill>",
    "<turn_aborted>",
];
const MILLISECONDS_PER_SECOND: i64 = 1_000;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
const UNIX_MILLISECONDS_THRESHOLD: i64 = 100_000_000_000;
const TIMESTAMP_FIELD_NAMES: &[&str] = &[
    "timestamp",
    "created_at",
    "createdAt",
    "created_at_ms",
    "createdAtMs",
    "time",
    "ts",
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TranscriptPreview {
    pub latest_assistant_message: Option<AssistantMessagePreview>,
    pub first_user_prompt: Option<String>,
    pub latest_activity_at_ms: Option<i64>,
    pub latest_message_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssistantMessagePreview {
    pub text: String,
    pub created_at_ms: Option<i64>,
}

pub fn latest_assistant_message_for_path(transcript_path: &Path) -> Option<String> {
    transcript_preview_for_path(transcript_path)
        .and_then(|preview| preview.latest_assistant_message)
        .map(|message| message.text)
}

pub fn transcript_preview_for_path_fast(transcript_path: &Path) -> Option<TranscriptPreview> {
    transcript_preview_from_tail(transcript_path)
}

pub fn transcript_preview_for_path(transcript_path: &Path) -> Option<TranscriptPreview> {
    let fast_preview = transcript_preview_for_path_fast(transcript_path);
    if fast_preview
        .as_ref()
        .is_some_and(transcript_preview_has_user_context)
    {
        return fast_preview;
    }

    let full_preview = || {
        let file = File::open(transcript_path).ok()?;
        transcript_preview_from_reader(BufReader::new(file))
    };
    full_preview().or(fast_preview)
}

fn transcript_preview_from_tail(transcript_path: &Path) -> Option<TranscriptPreview> {
    let mut file = File::open(transcript_path).ok()?;
    let file_len = file.metadata().ok()?.len();
    if file_len <= TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES {
        return transcript_preview_from_reader(BufReader::new(file));
    }
    let start = file_len.saturating_sub(TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buffer = Vec::with_capacity(TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES as usize);
    file.read_to_end(&mut buffer).ok()?;
    let tail = String::from_utf8_lossy(&buffer);
    let mut preview = transcript_preview_from_reversed_lines(tail.lines().rev())?;
    preview.first_user_prompt = first_user_prompt_from_head(transcript_path);
    Some(preview)
}

/// Scans forward from the start of the transcript for the first genuine user
/// prompt, bounded so large transcripts never trigger a full scan.
fn first_user_prompt_from_head(transcript_path: &Path) -> Option<String> {
    let file = File::open(transcript_path).ok()?;
    let reader = BufReader::new(file.take(FIRST_USER_PROMPT_HEAD_SCAN_BYTES));
    for line in reader.lines().map_while(Result::ok) {
        let Some(record) = message_record_from_transcript_line(&line) else {
            continue;
        };
        if record.role != USER_ROLE {
            continue;
        }
        let Some(text) = record.text.as_deref() else {
            continue;
        };
        if is_injected_user_text(text) {
            continue;
        }
        return Some(truncate_text(text, FIRST_USER_PROMPT_MAX_CHARS));
    }
    None
}

fn is_injected_user_text(text: &str) -> bool {
    let text = text.trim_start();
    INJECTED_USER_TEXT_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix))
}

fn transcript_preview_from_reader(reader: impl BufRead) -> Option<TranscriptPreview> {
    let mut preview = TranscriptPreview::default();
    let mut saw_message = false;

    for line in reader.lines().map_while(Result::ok) {
        let Some(record) = message_record_from_transcript_line(&line) else {
            continue;
        };
        saw_message = true;
        if let Some(created_at_ms) = record.created_at_ms {
            preview.latest_activity_at_ms = Some(created_at_ms);
        }
        if record.role == USER_ROLE
            && let Some(created_at_ms) = record.created_at_ms
        {
            preview.latest_message_at_ms = Some(created_at_ms);
        }
        if record.role == USER_ROLE
            && preview.first_user_prompt.is_none()
            && let Some(text) = record.text.as_deref()
            && !is_injected_user_text(text)
        {
            preview.first_user_prompt = Some(truncate_text(text, FIRST_USER_PROMPT_MAX_CHARS));
        }
        if record.role == ASSISTANT_ROLE
            && let Some(text) = record.text.as_deref()
        {
            preview.latest_assistant_message = Some(AssistantMessagePreview {
                text: truncate_text(text, ASSISTANT_MESSAGE_MAX_CHARS),
                created_at_ms: record.created_at_ms,
            });
        }
    }

    saw_message.then_some(preview)
}

fn transcript_preview_from_reversed_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Option<TranscriptPreview> {
    let mut preview = TranscriptPreview::default();
    let mut captured_latest_activity = false;
    let mut captured_latest_user_message = false;

    for line in lines {
        let Some(record) = message_record_from_transcript_line(line) else {
            continue;
        };
        if !captured_latest_activity && let Some(created_at_ms) = record.created_at_ms {
            preview.latest_activity_at_ms = Some(created_at_ms);
            captured_latest_activity = true;
        }
        // The reversed tail scan only sees the latest turns, so it can never
        // claim a first user prompt; the head scan owns that field.
        if !captured_latest_user_message && record.role == USER_ROLE {
            if let Some(created_at_ms) = record.created_at_ms {
                preview.latest_message_at_ms = Some(created_at_ms);
            }
            captured_latest_user_message = true;
        }
        if preview.latest_assistant_message.is_none()
            && record.role == ASSISTANT_ROLE
            && let Some(text) = record.text
        {
            preview.latest_assistant_message = Some(AssistantMessagePreview {
                text: truncate_text(&text, ASSISTANT_MESSAGE_MAX_CHARS),
                created_at_ms: record.created_at_ms,
            });
        }
        if captured_latest_activity
            && captured_latest_user_message
            && preview.latest_assistant_message.is_some()
        {
            break;
        }
    }
    preview
        .latest_assistant_message
        .is_some()
        .then_some(preview)
}

fn transcript_preview_has_user_context(preview: &TranscriptPreview) -> bool {
    preview.latest_assistant_message.is_some()
        && preview.latest_activity_at_ms.is_some()
        && (preview.latest_message_at_ms.is_some() || preview.first_user_prompt.is_some())
}

struct TranscriptMessageRecord {
    role: String,
    text: Option<String>,
    created_at_ms: Option<i64>,
}

fn message_record_from_transcript_line(line: &str) -> Option<TranscriptMessageRecord> {
    let value = serde_json::from_str::<Value>(line.trim()).ok()?;
    match value.get("type").and_then(Value::as_str)? {
        RESPONSE_ITEM_RECORD_TYPE => response_item_message_record(&value),
        EVENT_MESSAGE_RECORD_TYPE => user_event_message_record(&value),
        _ => None,
    }
}

fn response_item_message_record(value: &Value) -> Option<TranscriptMessageRecord> {
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str)? != MESSAGE_PAYLOAD_TYPE {
        return None;
    }
    let role = payload.get("role").and_then(Value::as_str)?.to_owned();
    let text = if matches!(role.as_str(), ASSISTANT_ROLE | USER_ROLE) {
        payload.get("content").and_then(content_text)
    } else {
        None
    };
    let created_at_ms =
        timestamp_millis_from_object(value).or_else(|| timestamp_millis_from_object(payload));

    Some(TranscriptMessageRecord {
        role,
        text,
        created_at_ms,
    })
}

fn user_event_message_record(value: &Value) -> Option<TranscriptMessageRecord> {
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str)? != USER_MESSAGE_PAYLOAD_TYPE {
        return None;
    }
    let created_at_ms =
        timestamp_millis_from_object(value).or_else(|| timestamp_millis_from_object(payload));
    let text = user_event_text(payload);
    Some(TranscriptMessageRecord {
        role: USER_ROLE.to_owned(),
        text,
        created_at_ms,
    })
}

fn user_event_text(payload: &Value) -> Option<String> {
    ["message", "text", "content"]
        .into_iter()
        .find_map(|field_name| payload.get(field_name).and_then(message_text_value))
}

fn message_text_value(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str().and_then(normalized_optional_string) {
        return Some(text);
    }
    value
        .get("content")
        .and_then(content_text)
        .or_else(|| content_text(value))
}

fn content_text(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str().and_then(normalized_optional_string) {
        return Some(text);
    }
    let content = content.as_array()?;
    let text_parts = content
        .iter()
        .filter_map(|item| {
            let content_type = item.get("type").and_then(Value::as_str)?;
            if !matches!(content_type, OUTPUT_TEXT_CONTENT_TYPE | TEXT_CONTENT_TYPE) {
                return None;
            }
            item.get("text")
                .and_then(Value::as_str)
                .and_then(normalized_optional_string)
        })
        .collect::<Vec<_>>();
    normalized_optional_string(&text_parts.join("\n"))
}

fn normalized_optional_string(value: &str) -> Option<String> {
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!value.is_empty()).then_some(value)
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    let take_chars = max_chars.saturating_sub(ELLIPSIS_CHARS);
    format!(
        "{}...",
        value
            .chars()
            .take(take_chars)
            .collect::<String>()
            .trim_end()
    )
}

fn timestamp_millis_from_object(value: &Value) -> Option<i64> {
    TIMESTAMP_FIELD_NAMES
        .iter()
        .find_map(|field_name| value.get(*field_name).and_then(timestamp_millis_from_value))
}

fn timestamp_millis_from_value(value: &Value) -> Option<i64> {
    if let Some(timestamp) = value.as_i64() {
        return normalize_unix_timestamp(timestamp);
    }
    if let Some(timestamp) = value.as_f64() {
        return normalize_unix_timestamp_float(timestamp);
    }
    let timestamp = value.as_str()?.trim();
    if timestamp.is_empty() {
        return None;
    }
    if let Ok(timestamp) = timestamp.parse::<i64>() {
        return normalize_unix_timestamp(timestamp);
    }
    OffsetDateTime::parse(timestamp, &Rfc3339)
        .ok()
        .map(|date| (date.unix_timestamp_nanos() / NANOSECONDS_PER_MILLISECOND) as i64)
}

fn normalize_unix_timestamp(timestamp: i64) -> Option<i64> {
    if timestamp.abs() < UNIX_MILLISECONDS_THRESHOLD {
        timestamp.checked_mul(MILLISECONDS_PER_SECOND)
    } else {
        Some(timestamp)
    }
}

fn normalize_unix_timestamp_float(timestamp: f64) -> Option<i64> {
    if !timestamp.is_finite() {
        return None;
    }
    let milliseconds = if timestamp.abs() < UNIX_MILLISECONDS_THRESHOLD as f64 {
        timestamp * MILLISECONDS_PER_SECOND as f64
    } else {
        timestamp
    };
    let rounded_milliseconds = milliseconds.round();
    if rounded_milliseconds < i64::MIN as f64 || rounded_milliseconds > i64::MAX as f64 {
        return None;
    }
    Some(rounded_milliseconds as i64)
}

#[cfg(test)]
mod tests {
    use super::{
        latest_assistant_message_for_path, transcript_preview_for_path,
        transcript_preview_for_path_fast,
    };
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn latest_assistant_message_reads_from_tail() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("tail.jsonl");
        let filler = "x".repeat((super::TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES + 1) as usize);
        let old_message = assistant_record("old");
        let new_message = assistant_record("new tail message");
        let latest_user_message = user_record_at("latest prompt", "2026-06-16T00:01:00Z");
        fs::write(
            &transcript_path,
            format!("{old_message}\n{filler}\n{new_message}\n{latest_user_message}"),
        )
        .expect("write transcript");

        assert_eq!(
            latest_assistant_message_for_path(&transcript_path).as_deref(),
            Some("new tail message")
        );
        assert_eq!(
            transcript_preview_for_path(&transcript_path)
                .and_then(|preview| preview.first_user_prompt)
                .as_deref(),
            Some("latest prompt")
        );
    }

    #[test]
    fn latest_assistant_message_falls_back_to_full_scan_when_tail_has_no_match() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("fallback.jsonl");
        let old_message = assistant_record("old full scan message");
        let filler = "x".repeat((super::TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES + 1) as usize);
        fs::write(&transcript_path, format!("{old_message}\n{filler}")).expect("write transcript");

        assert_eq!(
            latest_assistant_message_for_path(&transcript_path).as_deref(),
            Some("old full scan message")
        );
    }

    #[test]
    fn fast_transcript_preview_does_not_full_scan_when_tail_has_no_match() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("fast-no-fallback.jsonl");
        let old_message = assistant_record("old full scan message");
        let filler = "x".repeat((super::TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES + 1) as usize);
        fs::write(&transcript_path, format!("{old_message}\n{filler}")).expect("write transcript");

        assert_eq!(transcript_preview_for_path_fast(&transcript_path), None);
        assert_eq!(
            transcript_preview_for_path(&transcript_path)
                .and_then(|preview| preview.latest_assistant_message)
                .map(|message| message.text)
                .as_deref(),
            Some("old full scan message")
        );
    }

    #[test]
    fn fast_transcript_preview_keeps_tail_assistant_without_nearby_user_message() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("fast-tail-assistant.jsonl");
        let old_user_message = user_record_at("old first prompt", "2026-06-16T08:00:00Z");
        let filler = "x".repeat((super::TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES + 1) as usize);
        let assistant_message = assistant_record_at("fresh assistant", "2026-06-16T08:02:00Z");
        fs::write(
            &transcript_path,
            format!("{old_user_message}\n{filler}\n{assistant_message}"),
        )
        .expect("write transcript");

        let fast_preview = transcript_preview_for_path_fast(&transcript_path).expect("preview");
        assert_eq!(
            fast_preview
                .latest_assistant_message
                .as_ref()
                .map(|message| message.text.as_str()),
            Some("fresh assistant")
        );
        assert_eq!(
            fast_preview.first_user_prompt.as_deref(),
            Some("old first prompt")
        );

        let full_preview = transcript_preview_for_path(&transcript_path).expect("preview");
        assert_eq!(
            full_preview
                .latest_assistant_message
                .as_ref()
                .map(|message| message.text.as_str()),
            Some("fresh assistant")
        );
        assert_eq!(
            full_preview.first_user_prompt.as_deref(),
            Some("old first prompt")
        );
    }

    #[test]
    fn transcript_preview_reports_latest_user_message_time() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("timestamps.jsonl");
        let old_assistant_message = assistant_record_at("old assistant", "2026-06-16T08:00:00Z");
        let latest_user_message = user_record_at("new user", "2026-06-16T08:01:00Z");
        fs::write(
            &transcript_path,
            format!("{old_assistant_message}\n{latest_user_message}"),
        )
        .expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");
        assert_eq!(
            preview
                .latest_assistant_message
                .as_ref()
                .map(|message| message.text.as_str()),
            Some("old assistant")
        );
        assert_eq!(
            preview
                .latest_assistant_message
                .and_then(|message| message.created_at_ms),
            Some(1_781_596_800_000)
        );
        assert_eq!(preview.latest_activity_at_ms, Some(1_781_596_860_000));
        assert_eq!(preview.latest_message_at_ms, Some(1_781_596_860_000));
        assert_eq!(preview.first_user_prompt.as_deref(), Some("new user"));
    }

    #[test]
    fn transcript_preview_does_not_treat_assistant_reply_as_sent_message() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("assistant-after-user.jsonl");
        let user_message = user_record_at("new user", "2026-06-16T08:01:00Z");
        let assistant_message = assistant_record_at("assistant reply", "2026-06-16T08:02:00Z");
        fs::write(
            &transcript_path,
            format!("{user_message}\n{assistant_message}"),
        )
        .expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");

        assert_eq!(preview.latest_message_at_ms, Some(1_781_596_860_000));
        assert_eq!(preview.latest_activity_at_ms, Some(1_781_596_920_000));
        assert_eq!(
            preview
                .latest_assistant_message
                .and_then(|message| message.created_at_ms),
            Some(1_781_596_920_000)
        );
    }

    #[test]
    fn transcript_preview_reads_codex_user_message_events() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("user-event.jsonl");
        let assistant_message = assistant_record_at("assistant", "2026-06-16T08:00:00Z");
        let user_message = user_event_record_at("new user", "2026-06-16T08:01:00Z");
        fs::write(
            &transcript_path,
            format!("{assistant_message}\n{user_message}"),
        )
        .expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");

        assert_eq!(preview.latest_activity_at_ms, Some(1_781_596_860_000));
        assert_eq!(preview.latest_message_at_ms, Some(1_781_596_860_000));
        assert_eq!(preview.first_user_prompt.as_deref(), Some("new user"));
    }

    #[test]
    fn transcript_preview_reads_head_first_prompt_for_large_files() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("large-tail-preview.jsonl");
        let first_user_message = user_record_at("old first prompt", "2026-06-16T08:00:00Z");
        let filler = "x".repeat((super::TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES + 1) as usize);
        let assistant_message = assistant_record_at("fresh assistant", "2026-06-16T08:02:00Z");
        let latest_user_message = user_record_at("latest user", "2026-06-16T08:03:00Z");
        fs::write(
            &transcript_path,
            format!("{first_user_message}\n{filler}\n{assistant_message}\n{latest_user_message}"),
        )
        .expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");

        assert_eq!(
            preview
                .latest_assistant_message
                .as_ref()
                .map(|message| message.text.as_str()),
            Some("fresh assistant")
        );
        assert_eq!(preview.latest_activity_at_ms, Some(1_781_596_980_000));
        assert_eq!(preview.latest_message_at_ms, Some(1_781_596_980_000));
        assert_eq!(
            preview.first_user_prompt.as_deref(),
            Some("old first prompt")
        );
    }

    #[test]
    fn transcript_preview_skips_injected_user_records_for_first_prompt() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("injected-user-records.jsonl");
        let instructions = user_record_at(
            "# AGENTS.md instructions for /repo body",
            "2026-06-16T08:00:00Z",
        );
        let skill = user_record_at("<skill> body of a skill", "2026-06-16T08:00:01Z");
        let real_prompt = user_record_at("real prompt", "2026-06-16T08:00:02Z");
        let aborted = user_record_at(
            "<turn_aborted> The user interrupted the previous turn on purpose.",
            "2026-06-16T08:00:03Z",
        );
        let assistant_message = assistant_record_at("assistant", "2026-06-16T08:00:04Z");
        fs::write(
            &transcript_path,
            format!("{instructions}\n{skill}\n{real_prompt}\n{aborted}\n{assistant_message}"),
        )
        .expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");

        assert_eq!(preview.first_user_prompt.as_deref(), Some("real prompt"));
    }

    #[test]
    fn transcript_preview_head_scan_never_claims_tail_user_messages() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("head-bound.jsonl");
        let filler = "x".repeat((super::FIRST_USER_PROMPT_HEAD_SCAN_BYTES + 1) as usize);
        let assistant_message = assistant_record_at("fresh assistant", "2026-06-16T08:02:00Z");
        let latest_user_message = user_record_at("latest user", "2026-06-16T08:03:00Z");
        fs::write(
            &transcript_path,
            format!("{filler}\n{assistant_message}\n{latest_user_message}"),
        )
        .expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");

        assert_eq!(preview.first_user_prompt, None);
        assert_eq!(preview.latest_message_at_ms, Some(1_781_596_980_000));
    }

    #[test]
    fn transcript_preview_preserves_fractional_second_timestamps() {
        let tempdir = tempdir().expect("tempdir");
        let transcript_path = tempdir.path().join("fractional-seconds.jsonl");
        let message = json!({
            "type": super::RESPONSE_ITEM_RECORD_TYPE,
            "timestamp": 1781596860.321,
            "payload": {
                "type": super::MESSAGE_PAYLOAD_TYPE,
                "role": super::USER_ROLE,
                "content": [
                    {
                        "type": super::TEXT_CONTENT_TYPE,
                        "text": "fractional"
                    }
                ]
            }
        });
        fs::write(&transcript_path, message.to_string()).expect("write transcript");

        let preview = transcript_preview_for_path(&transcript_path).expect("preview");
        assert_eq!(preview.latest_activity_at_ms, Some(1_781_596_860_321));
        assert_eq!(preview.latest_message_at_ms, Some(1_781_596_860_321));
        assert_eq!(preview.first_user_prompt.as_deref(), Some("fractional"));
    }

    fn assistant_record(text: &str) -> String {
        assistant_record_at(text, "2026-06-16T00:00:00Z")
    }

    fn assistant_record_at(text: &str, timestamp: &str) -> String {
        json!({
            "type": super::RESPONSE_ITEM_RECORD_TYPE,
            "timestamp": timestamp,
            "payload": {
                "type": super::MESSAGE_PAYLOAD_TYPE,
                "role": super::ASSISTANT_ROLE,
                "content": [
                    {
                        "type": super::OUTPUT_TEXT_CONTENT_TYPE,
                        "text": text
                    }
                ]
            }
        })
        .to_string()
    }

    fn user_record_at(text: &str, timestamp: &str) -> String {
        json!({
            "type": super::RESPONSE_ITEM_RECORD_TYPE,
            "timestamp": timestamp,
            "payload": {
                "type": super::MESSAGE_PAYLOAD_TYPE,
                "role": "user",
                "content": [
                    {
                        "type": super::TEXT_CONTENT_TYPE,
                        "text": text
                    }
                ]
            }
        })
        .to_string()
    }

    fn user_event_record_at(text: &str, timestamp: &str) -> String {
        json!({
            "type": super::EVENT_MESSAGE_RECORD_TYPE,
            "timestamp": timestamp,
            "payload": {
                "type": super::USER_MESSAGE_PAYLOAD_TYPE,
                "message": text
            }
        })
        .to_string()
    }
}

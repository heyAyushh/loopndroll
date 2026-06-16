use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

const ASSISTANT_MESSAGE_MAX_CHARS: usize = 600;
const RESPONSE_ITEM_RECORD_TYPE: &str = "response_item";
const MESSAGE_PAYLOAD_TYPE: &str = "message";
const ASSISTANT_ROLE: &str = "assistant";
const OUTPUT_TEXT_CONTENT_TYPE: &str = "output_text";
const TEXT_CONTENT_TYPE: &str = "text";
const ELLIPSIS_CHARS: usize = 3;
const BYTES_PER_KIB: u64 = 1024;
const TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES: u64 = 64 * BYTES_PER_KIB;

pub fn latest_assistant_message_for_path(transcript_path: &Path) -> Option<String> {
    latest_assistant_message_from_tail(transcript_path).or_else(|| {
        let file = File::open(transcript_path).ok()?;
        latest_assistant_message_from_reader(BufReader::new(file))
    })
}

fn latest_assistant_message_from_tail(transcript_path: &Path) -> Option<String> {
    let mut file = File::open(transcript_path).ok()?;
    let file_len = file.metadata().ok()?.len();
    if file_len <= TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES {
        return latest_assistant_message_from_reader(BufReader::new(file));
    }
    let start = file_len.saturating_sub(TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buffer = Vec::with_capacity(TRANSCRIPT_PREVIEW_TAIL_SCAN_BYTES as usize);
    file.read_to_end(&mut buffer).ok()?;
    let tail = String::from_utf8_lossy(&buffer);
    latest_assistant_message_from_reversed_lines(tail.lines().rev())
}

fn latest_assistant_message_from_reader(reader: impl BufRead) -> Option<String> {
    let mut latest_message = None;
    for line in reader.lines().map_while(Result::ok) {
        let Some(message) = assistant_message_from_transcript_line(&line) else {
            continue;
        };
        latest_message = Some(truncate_text(&message, ASSISTANT_MESSAGE_MAX_CHARS));
    }
    latest_message
}

fn latest_assistant_message_from_reversed_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Option<String> {
    for line in lines {
        let Some(message) = assistant_message_from_transcript_line(line) else {
            continue;
        };
        return Some(truncate_text(&message, ASSISTANT_MESSAGE_MAX_CHARS));
    }
    None
}

fn assistant_message_from_transcript_line(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line.trim()).ok()?;
    if value.get("type").and_then(Value::as_str)? != RESPONSE_ITEM_RECORD_TYPE {
        return None;
    }
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str)? != MESSAGE_PAYLOAD_TYPE {
        return None;
    }
    if payload.get("role").and_then(Value::as_str)? != ASSISTANT_ROLE {
        return None;
    }
    content_text(payload.get("content")?)
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

#[cfg(test)]
mod tests {
    use super::latest_assistant_message_for_path;
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
        fs::write(
            &transcript_path,
            format!("{old_message}\n{filler}\n{new_message}"),
        )
        .expect("write transcript");

        assert_eq!(
            latest_assistant_message_for_path(&transcript_path).as_deref(),
            Some("new tail message")
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

    fn assistant_record(text: &str) -> String {
        json!({
            "type": super::RESPONSE_ITEM_RECORD_TYPE,
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
}

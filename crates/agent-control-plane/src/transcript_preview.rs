use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

const ASSISTANT_MESSAGE_MAX_CHARS: usize = 600;
const RESPONSE_ITEM_RECORD_TYPE: &str = "response_item";
const MESSAGE_PAYLOAD_TYPE: &str = "message";
const ASSISTANT_ROLE: &str = "assistant";
const OUTPUT_TEXT_CONTENT_TYPE: &str = "output_text";
const TEXT_CONTENT_TYPE: &str = "text";
const ELLIPSIS_CHARS: usize = 3;

pub fn latest_assistant_message_for_path(transcript_path: &Path) -> Option<String> {
    let file = File::open(transcript_path).ok()?;
    let reader = BufReader::new(file);
    let mut latest_message = None;
    for line in reader.lines().map_while(Result::ok) {
        let Some(message) = assistant_message_from_transcript_line(&line) else {
            continue;
        };
        latest_message = Some(truncate_text(&message, ASSISTANT_MESSAGE_MAX_CHARS));
    }
    latest_message
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

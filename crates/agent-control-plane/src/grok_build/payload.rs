use anyhow::{Context, Result};
use serde_json::Value;

use crate::mobile::session::MobileHookPayload;

const GROK_HOOK_EVENT_ENV: &str = "GROK_HOOK_EVENT";
const GROK_SESSION_ID_ENV: &str = "GROK_SESSION_ID";
const GROK_WORKSPACE_ROOT_ENV: &str = "GROK_WORKSPACE_ROOT";

pub fn is_grok_hook_invocation() -> bool {
    std::env::var(GROK_HOOK_EVENT_ENV).is_ok()
}

pub fn parse_hook_payload(input: &str) -> Result<MobileHookPayload> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(MobileHookPayload {
            hook_event_name: String::new(),
            session_id: std::env::var(GROK_SESSION_ID_ENV).ok(),
            turn_id: None,
            cwd: std::env::var(GROK_WORKSPACE_ROOT_ENV).ok(),
            last_assistant_message: None,
        });
    }

    let value: Value =
        serde_json::from_str(trimmed).context("parse hook payload JSON from stdin")?;
    if is_grok_hook_payload(&value) {
        return Ok(grok_payload_from_value(&value));
    }

    serde_json::from_value(value).context("decode Codex hook payload")
}

fn is_grok_hook_payload(value: &Value) -> bool {
    if std::env::var(GROK_HOOK_EVENT_ENV).is_ok() {
        return true;
    }
    value
        .get("hookEventName")
        .and_then(Value::as_str)
        .is_some_and(|name| normalize_grok_event_name(name) != name || name.contains('_'))
}

fn grok_payload_from_value(value: &Value) -> MobileHookPayload {
    let hook_event_name = value
        .get("hookEventName")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var(GROK_HOOK_EVENT_ENV).ok())
        .map(|name| normalize_grok_event_name(&name))
        .unwrap_or_default();
    let session_id = value
        .get("sessionId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var(GROK_SESSION_ID_ENV).ok());
    let cwd = value
        .get("cwd")
        .or_else(|| value.get("workspaceRoot"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| std::env::var(GROK_WORKSPACE_ROOT_ENV).ok());
    let last_assistant_message = value
        .get("lastAssistantMessage")
        .and_then(Value::as_str)
        .map(str::to_owned);

    MobileHookPayload {
        hook_event_name,
        session_id,
        turn_id: value
            .get("turnId")
            .and_then(Value::as_str)
            .map(str::to_owned),
        cwd,
        last_assistant_message,
    }
}

fn normalize_grok_event_name(name: &str) -> String {
    match name {
        "stop" | "Stop" => "Stop".to_owned(),
        "session_start" | "SessionStart" => "SessionStart".to_owned(),
        "user_prompt_submit" | "UserPromptSubmit" => "UserPromptSubmit".to_owned(),
        "session_end" | "SessionEnd" => "SessionEnd".to_owned(),
        "notification" | "Notification" => "Notification".to_owned(),
        other if other.contains('_') => {
            let pascal = other
                .split('_')
                .filter(|segment| !segment.is_empty())
                .map(|segment| {
                    let mut chars = segment.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    }
                })
                .collect::<String>();
            if pascal.is_empty() {
                other.to_owned()
            } else {
                pascal
            }
        }
        other => other.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_grok_stop_payload() {
        let payload = grok_payload_from_value(&serde_json::json!({
            "hookEventName": "stop",
            "sessionId": "session-1",
            "cwd": "/Users/test/project",
            "lastAssistantMessage": "Done for now."
        }));

        assert_eq!(payload.hook_event_name, "Stop");
        assert_eq!(payload.session_id.as_deref(), Some("session-1"));
        assert_eq!(payload.cwd.as_deref(), Some("/Users/test/project"));
        assert_eq!(
            payload.last_assistant_message.as_deref(),
            Some("Done for now.")
        );
    }

    #[test]
    fn parses_codex_payload_without_grok_normalization() {
        let payload = parse_hook_payload(
            r#"{"hookEventName":"Stop","sessionId":"thread-main","cwd":"/tmp/project"}"#,
        )
        .expect("parse codex payload");

        assert_eq!(payload.hook_event_name, "Stop");
        assert_eq!(payload.session_id.as_deref(), Some("thread-main"));
    }
}

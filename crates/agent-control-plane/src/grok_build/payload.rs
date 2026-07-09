use anyhow::Result;

pub use crate::hooks::adapter::is_grok_hook_invocation;
use crate::hooks::adapter::parse_grok_hook_payload as parse_adapter_grok_hook_payload;
use crate::mobile::session::MobileHookPayload;

pub fn parse_hook_payload(input: &str) -> Result<MobileHookPayload> {
    parse_adapter_grok_hook_payload(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_grok_stop_payload() {
        let payload = parse_hook_payload(
            r#"{"hookEventName":"stop","sessionId":"session-1","cwd":"/Users/test/project","lastAssistantMessage":"Done for now."}"#,
        )
        .expect("parse grok payload");

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

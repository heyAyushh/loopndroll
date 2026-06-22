use crate::assistant::AssistantKind;
use crate::codex::LaunchKind;
use crate::mobile::api::assistant_identity::{
    CLAUDE_CODE_ASSISTANT_CLIENT, CLAUDE_SOURCE_LABEL, CODEX_ASSISTANT_CLIENT, CODEX_SOURCE_LABEL,
    CURSOR_ASSISTANT_CLIENT, DEVIN_ASSISTANT_CLIENT, DEVIN_SOURCE_LABEL, ZED_ASSISTANT_CLIENT,
    ZED_SOURCE_LABEL, thread_matches_assistant_surface,
};
use crate::mobile::api::prompt_delivery::{
    PromptDeliveryAction, PromptResumeTarget, prompt_delivery_action_for_thread,
};
use crate::mobile::api::summary::session_summary;
use crate::mobile::api::test_support::{session_state_with_lifecycle, test_thread};
use crate::mobile::session::{MOBILE_SESSION_STATUS_ACTIVE, MobileSessionState};
use serde_json::Value;

#[test]
fn prompt_delivery_target_resumes_active_codex_sessions() {
    let thread = test_thread("thread-1", AssistantKind::Codex, None);
    let session_state = session_state_with_lifecycle("thread-1", MOBILE_SESSION_STATUS_ACTIVE);

    assert!(matches!(
        prompt_delivery_action_for_thread(&thread, &session_state),
        Ok(PromptDeliveryAction::ResumeCodex(PromptResumeTarget { thread_id, .. }))
            if thread_id == "thread-1"
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(true));
    assert_eq!(summary["promptDeliveryUnavailableReason"], Value::Null);
}

#[test]
fn session_summary_separates_activity_and_message_freshness() {
    let mut thread = test_thread("thread-1", AssistantKind::Codex, None);
    thread.updated_at_ms = Some(1_781_596_920_000);
    thread.latest_message_at_ms = Some(1_781_596_860_000);
    let session_state = session_state_with_lifecycle("thread-1", MOBILE_SESSION_STATUS_ACTIVE);

    let summary = session_summary(&thread, 0, &session_state);

    assert_eq!(summary["lastUpdatedAt"], "2026-06-16T08:02:00Z");
    assert_eq!(summary["lastActivityAt"], "2026-06-16T08:02:00Z");
    assert_eq!(summary["lastMessageAt"], "2026-06-16T08:01:00Z");
    assert_eq!(summary["updatedAtMs"], 1_781_596_920_000_i64);
    assert_eq!(summary["latestMessageAtMs"], 1_781_596_860_000_i64);
    assert_eq!(summary["lastActivityAtMs"], 1_781_596_920_000_i64);
    assert_eq!(summary["lastMessageAtMs"], 1_781_596_860_000_i64);
    assert_eq!(summary["metadata"]["assistantKind"], "codex");
    assert_eq!(
        summary["metadata"]["supportsSubagents"],
        serde_json::json!(true)
    );
    assert_eq!(summary["metadata"]["spawn"]["launchKind"], "main");
}

#[test]
fn session_summary_preserves_fractional_millisecond_freshness() {
    let mut thread = test_thread("thread-1", AssistantKind::Codex, None);
    thread.updated_at_ms = Some(1_781_596_920_321);
    thread.latest_message_at_ms = Some(1_781_596_860_123);
    let session_state = session_state_with_lifecycle("thread-1", MOBILE_SESSION_STATUS_ACTIVE);

    let summary = session_summary(&thread, 0, &session_state);

    assert_eq!(summary["lastUpdatedAt"], "2026-06-16T08:02:00.321Z");
    assert_eq!(summary["lastActivityAt"], "2026-06-16T08:02:00.321Z");
    assert_eq!(summary["lastMessageAt"], "2026-06-16T08:01:00.123Z");
}

#[test]
fn session_summary_uses_capabilities_for_subagent_metadata() {
    let mut codex_thread = test_thread("thread-1", AssistantKind::Codex, None);
    codex_thread.capabilities.spawn.children = vec!["child-thread".to_owned()];
    let mut devin_thread = test_thread(
        "devin:looper:session-1",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    devin_thread.capabilities.spawn.launch_kind = LaunchKind::Subagent;
    let session_state = MobileSessionState::default();

    let codex_summary = session_summary(&codex_thread, 0, &session_state);
    let devin_summary = session_summary(&devin_thread, 1, &session_state);

    assert_eq!(
        codex_summary["metadata"]["supportsSubagents"],
        serde_json::json!(true)
    );
    assert_eq!(
        codex_summary["metadata"]["spawn"]["children"],
        serde_json::json!(["child-thread"])
    );
    assert_eq!(
        devin_summary["metadata"]["supportsSubagents"],
        serde_json::json!(false)
    );
    assert_eq!(devin_summary["metadata"]["spawn"]["launchKind"], "subagent");
}

#[test]
fn codex_acp_session_under_devin_paths_stays_on_codex_surface() {
    let mut thread = test_thread("thread-1", AssistantKind::Codex, None);
    thread.source = Some("codex-acp".to_owned());
    thread.originator = Some("Codex ACP via Devin - Next".to_owned());
    thread.transcript_path = Some(
        "/Users/test/Library/Application Support/Devin - Next/User/acp-events/1.ndjson".to_owned(),
    );
    let session_state = session_state_with_lifecycle("thread-1", MOBILE_SESSION_STATUS_ACTIVE);

    let summary = session_summary(&thread, 0, &session_state);

    assert!(thread_matches_assistant_surface(&thread, "codex"));
    assert!(!thread_matches_assistant_surface(&thread, "devin"));
    assert_eq!(summary["assistantClient"], CODEX_ASSISTANT_CLIENT);
    assert_eq!(summary["metadata"]["sourceDisplayName"], CODEX_SOURCE_LABEL);
}

#[test]
fn typed_devin_sessions_with_sparse_metadata_stay_on_devin_surface() {
    let thread = test_thread(
        "devin:looper:session-1",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    let session_state = MobileSessionState::default();

    assert!(thread_matches_assistant_surface(&thread, "devin"));
    assert!(!thread_matches_assistant_surface(&thread, "codex"));

    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["assistantClient"], DEVIN_ASSISTANT_CLIENT);
    assert_eq!(summary["metadata"]["sourceDisplayName"], DEVIN_SOURCE_LABEL);
}

#[test]
fn typed_codex_surface_clients_do_not_fall_back_to_devin_paths() {
    let mut thread = test_thread("thread-1", AssistantKind::Cursor, None);
    thread.originator = Some("Devin - Next".to_owned());
    thread.transcript_path = Some(
        "/Users/test/Library/Application Support/Devin - Next/User/acp-events/1.ndjson".to_owned(),
    );
    let session_state = MobileSessionState::default();

    assert!(thread_matches_assistant_surface(&thread, "codex"));
    assert!(!thread_matches_assistant_surface(&thread, "devin"));

    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["assistantClient"], CURSOR_ASSISTANT_CLIENT);
    assert_eq!(summary["metadata"]["sourceDisplayName"], CODEX_SOURCE_LABEL);
}

#[test]
fn typed_claude_sessions_use_claude_mobile_surface() {
    let thread = test_thread("thread-1", AssistantKind::ClaudeCode, None);
    let session_state = MobileSessionState::default();

    assert!(thread_matches_assistant_surface(&thread, "claude-code"));
    assert!(!thread_matches_assistant_surface(&thread, "codex"));
    assert!(!thread_matches_assistant_surface(&thread, "devin"));

    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["assistantClient"], CLAUDE_CODE_ASSISTANT_CLIENT);
    assert_eq!(
        summary["metadata"]["sourceDisplayName"],
        CLAUDE_SOURCE_LABEL
    );
}

#[test]
fn typed_zed_sessions_use_zed_mobile_identity() {
    let thread = test_thread("thread-1", AssistantKind::Zed, None);
    let session_state = MobileSessionState::default();

    assert!(thread_matches_assistant_surface(&thread, "zed"));
    assert!(!thread_matches_assistant_surface(&thread, "codex"));
    assert!(!thread_matches_assistant_surface(&thread, "devin"));

    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["assistantClient"], ZED_ASSISTANT_CLIENT);
    assert_eq!(summary["metadata"]["sourceDisplayName"], ZED_SOURCE_LABEL);
}

#[test]
fn unknown_sessions_still_fall_back_to_path_inference() {
    let mut thread = test_thread("thread-1", AssistantKind::Unknown, None);
    thread.transcript_path = Some("/Users/test/.codex/sessions/thread-1.jsonl".to_owned());
    thread.source = Some("vscode".to_owned());
    thread.originator = Some("Devin - Next".to_owned());
    let session_state = MobileSessionState::default();

    assert!(thread_matches_assistant_surface(&thread, "devin"));
    assert!(!thread_matches_assistant_surface(&thread, "codex"));

    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["assistantClient"], DEVIN_ASSISTANT_CLIENT);
}

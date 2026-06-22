use crate::assistant::AssistantKind;
use crate::mobile::api::availability::{
    DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON,
    DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON, INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON,
};
use crate::mobile::api::prompt_delivery::{
    PromptDeliveryAction, PromptResumeTarget, prompt_delivery_action_for_thread,
};
use crate::mobile::api::summary::{AWAIT_REPLY_PRESET, session_summary};
use crate::mobile::api::test_support::{session_state_with_lifecycle, test_thread};
use crate::mobile::session::{
    MOBILE_SESSION_STATUS_ACTIVE, MOBILE_SESSION_STATUS_STOPPED, MobileSessionError,
    MobileSessionState,
};

#[test]
fn prompt_delivery_target_resumes_waiting_codex_sessions() {
    let thread = test_thread("thread-1", AssistantKind::Codex, None);
    let mut session_state = session_state_with_lifecycle("thread-1", MOBILE_SESSION_STATUS_STOPPED);
    session_state
        .sessions
        .entry("thread-1".to_owned())
        .or_default()
        .preset = Some(AWAIT_REPLY_PRESET.to_owned());

    assert!(matches!(
        prompt_delivery_action_for_thread(&thread, &session_state),
        Ok(PromptDeliveryAction::ResumeCodex(PromptResumeTarget { thread_id, .. }))
            if thread_id == "thread-1"
    ));
}

#[test]
fn prompt_delivery_target_uses_devin_local_hook_transport_when_active() {
    let thread = test_thread(
        "devin:devin-cli:shadow-canidae",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    let session_state = MobileSessionState::default();

    let action = prompt_delivery_action_for_thread(&thread, &session_state)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("Devin local sessions should queue via hook transport");
    assert!(matches!(action, PromptDeliveryAction::QueueForHook));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(true));
    assert!(summary["promptDeliveryUnavailableReason"].is_null());
}

#[test]
fn prompt_delivery_target_rejects_stopped_devin_hook_transport() {
    let thread = test_thread(
        "devin:devin-cli:shadow-canidae",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_STOPPED),
    );
    let session_state = MobileSessionState::default();

    let error = prompt_delivery_action_for_thread(&thread, &session_state)
        .expect_err("stopped Devin local sessions cannot be woken by hooks");
    assert!(matches!(
        error,
        MobileSessionError::PromptDeliveryUnavailableReason(reason)
            if reason == DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(false));
    assert_eq!(
        summary["promptDeliveryUnavailableReason"],
        DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON
    );
}

#[test]
fn prompt_delivery_target_uses_claude_acp_hook_transport_when_active() {
    let thread = test_thread(
        "devin:claude-acp:bd6aa5c3-b6d1-4331-97e0-045c44652e2d",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    let session_state = MobileSessionState::default();

    let action = prompt_delivery_action_for_thread(&thread, &session_state)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("Devin ACP sessions should queue via hook transport");
    assert!(matches!(action, PromptDeliveryAction::QueueForHook));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(true));
    assert!(summary["promptDeliveryUnavailableReason"].is_null());
}

#[test]
fn prompt_delivery_target_uses_looper_acp_direct_transport() {
    let thread = test_thread(
        "devin:looper:session-1",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_STOPPED),
    );
    let session_state = MobileSessionState::default();

    let action = prompt_delivery_action_for_thread(&thread, &session_state)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("Looper-owned Devin ACP sessions should send directly");
    assert!(matches!(
        action,
        PromptDeliveryAction::SendDevinAcp { session_id } if session_id == "acp/looper/session-1"
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(true));
    assert!(summary["promptDeliveryUnavailableReason"].is_null());
}

#[test]
fn prompt_delivery_target_uses_zed_codex_looper_acp_direct_transport() {
    let thread = test_thread(
        "zed:codex:looper:session-1",
        AssistantKind::Zed,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    let session_state = MobileSessionState::default();

    let action = prompt_delivery_action_for_thread(&thread, &session_state)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("Looper-owned Zed ACP sessions should send directly");
    assert!(matches!(
        action,
        PromptDeliveryAction::SendLooperAcp { client_id, session_id }
            if client_id == "zed" && session_id == "acp/looper/session-1"
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(true));
    assert!(summary["promptDeliveryUnavailableReason"].is_null());
}

#[test]
fn prompt_delivery_preserves_legacy_zed_looper_acp_thread_ids() {
    let thread = test_thread(
        "zed:looper:session-1",
        AssistantKind::Zed,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    let session_state = MobileSessionState::default();

    let action = prompt_delivery_action_for_thread(&thread, &session_state)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("Legacy Looper-owned Zed ACP sessions should remain promptable");
    assert!(matches!(
        action,
        PromptDeliveryAction::SendLooperAcp { client_id, session_id }
            if client_id == "zed" && session_id == "acp/looper/session-1"
    ));
}

#[test]
fn stopped_zed_acp_sessions_are_visible_but_not_promptable() {
    let thread = test_thread(
        "zed:codex:session-1",
        AssistantKind::Zed,
        Some(MOBILE_SESSION_STATUS_STOPPED),
    );
    let session_state = MobileSessionState::default();

    let error = prompt_delivery_action_for_thread(&thread, &session_state)
        .expect_err("stopped Zed ACP sessions need a live bridge");
    assert!(matches!(
        error,
        MobileSessionError::PromptDeliveryUnavailableReason(_)
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(false));
    assert_eq!(
        summary["promptDeliveryUnavailableReason"],
        INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON
    );
}

#[test]
fn unsupported_devin_providers_remain_read_only() {
    let thread = test_thread(
        "devin:devin-cloud:session-1",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_STOPPED),
    );
    let session_state = MobileSessionState::default();

    let error = prompt_delivery_action_for_thread(&thread, &session_state)
        .expect_err("unsupported Devin provider");
    assert!(matches!(
        error,
        MobileSessionError::PromptDeliveryUnavailableReason(reason)
            if reason == DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(false));
    assert_eq!(
        summary["promptDeliveryUnavailableReason"],
        DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON
    );
}

#[test]
fn malformed_devin_thread_ids_remain_unavailable() {
    let thread = test_thread(
        "devin:session-1",
        AssistantKind::DevinDesktop,
        Some(MOBILE_SESSION_STATUS_STOPPED),
    );
    let session_state = MobileSessionState::default();

    let error = prompt_delivery_action_for_thread(&thread, &session_state)
        .expect_err("malformed Devin thread id");
    assert!(matches!(
        error,
        MobileSessionError::PromptDeliveryUnavailable
    ));
}

#[test]
fn prompt_delivery_target_uses_claude_hook_transport_when_active() {
    let thread = test_thread(
        "claude:session-1",
        AssistantKind::ClaudeCode,
        Some(MOBILE_SESSION_STATUS_ACTIVE),
    );
    let session_state = MobileSessionState::default();

    let action = prompt_delivery_action_for_thread(&thread, &session_state)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("active Claude Code sessions should queue via hook transport");
    assert!(matches!(action, PromptDeliveryAction::QueueForHook));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(true));
    assert!(summary["promptDeliveryUnavailableReason"].is_null());
}

#[test]
fn prompt_delivery_target_rejects_stopped_claude_hook_transport() {
    let thread = test_thread(
        "claude:session-1",
        AssistantKind::ClaudeCode,
        Some(MOBILE_SESSION_STATUS_STOPPED),
    );
    let session_state = MobileSessionState::default();

    let error = prompt_delivery_action_for_thread(&thread, &session_state)
        .expect_err("stopped Claude Code sessions cannot be woken by hooks");
    assert!(matches!(
        error,
        MobileSessionError::PromptDeliveryUnavailableReason(reason)
            if reason == INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON
    ));
    let summary = session_summary(&thread, 0, &session_state);
    assert_eq!(summary["canSendPrompt"], serde_json::json!(false));
    assert_eq!(
        summary["promptDeliveryUnavailableReason"],
        INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON
    );
}

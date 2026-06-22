use serde_json::{Value, json};

use crate::mobile::session::{
    DEFAULT_REMOTE_PROMPT, MobileCompletionCheck, MobileNotificationRoute, MobileSessionState,
};

const DEFAULT_SCOPE: &str = "global";

pub(super) fn mobile_global_settings(session_state: &MobileSessionState) -> Value {
    json!({
        "defaultPrompt": if session_state.default_prompt.is_empty() {
            DEFAULT_REMOTE_PROMPT
        } else {
            &session_state.default_prompt
        },
        "globalMode": session_state.global_preset,
        "scope": if session_state.scope.is_empty() {
            DEFAULT_SCOPE
        } else {
            &session_state.scope
        },
        "notificationLabel": notification_label(
            &session_state.notifications,
            session_state.global_notification_id.as_deref(),
        ),
        "defaultNotificationTargetIds": session_state.default_notification_target_ids,
        "notificationTargetLabels": notification_target_labels(
            &session_state.notifications,
            &session_state.default_notification_target_ids,
        ),
        "completionCheckLabel": completion_check_label(
            &session_state.completion_checks,
            session_state.global_completion_check_id.as_deref(),
        ),
        "completionCheckWaitForReply": session_state.global_completion_check_wait_for_reply,
        "assistantSurface": session_state.assistant_surface,
        "siriDefaultSessionId": session_state.siri_default_thread_id,
        "siriDefaultAssistantSurface": session_state.siri_default_assistant_surface,
        "siriCurrentSessionId": session_state.siri_current_thread_id,
        "siriCurrentAssistantSurface": session_state.siri_current_assistant_surface,
        "siriCurrentUpdatedAtMs": session_state.siri_current_updated_at_ms,
    })
}

pub(super) fn notification_summary(notification: &MobileNotificationRoute) -> Value {
    json!({
        "id": notification.id,
        "label": notification.label,
        "channel": notification.channel,
    })
}

pub(super) fn completion_check_summary(completion_check: &MobileCompletionCheck) -> Value {
    json!({
        "id": completion_check.id,
        "label": completion_check.label,
        "commandCount": completion_check.command_count(),
    })
}

fn notification_label(
    notifications: &[MobileNotificationRoute],
    notification_id: Option<&str>,
) -> Option<String> {
    let notification_id = notification_id?;
    notifications
        .iter()
        .find(|notification| notification.id == notification_id)
        .map(|notification| notification.label.clone())
}

fn notification_target_labels(
    notifications: &[MobileNotificationRoute],
    target_ids: &[String],
) -> Vec<String> {
    target_ids
        .iter()
        .filter_map(|target_id| notification_target_label(notifications, target_id))
        .collect()
}

fn notification_target_label(
    notifications: &[MobileNotificationRoute],
    target_id: &str,
) -> Option<String> {
    match target_id {
        "iphone" => Some("iPhone".to_owned()),
        "macos" => Some("macOS".to_owned()),
        _ => notifications
            .iter()
            .find(|notification| notification.id == target_id)
            .map(|notification| notification.label.clone()),
    }
}

fn completion_check_label(
    completion_checks: &[MobileCompletionCheck],
    completion_check_id: Option<&str>,
) -> Option<String> {
    let completion_check_id = completion_check_id?;
    completion_checks
        .iter()
        .find(|completion_check| completion_check.id == completion_check_id)
        .map(|completion_check| completion_check.label.clone())
}

use serde_json::{Value, json};

use crate::control_plane::{DesktopSnapshot, DesktopThread};
use crate::mobile::session::MobileSessionState;

use super::assistant_identity::thread_matches_assistant_surface;
use super::overrides::{
    effective_completion_check_id, effective_completion_check_wait_for_reply, is_deleted,
    session_notification_ids, session_override,
};
use super::settings::{completion_check_summary, notification_summary};
use super::summary::{nullable_string_value, session_summary};

pub fn mobile_session_detail(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Option<Value> {
    let (index, thread) =
        snapshot.threads.iter().enumerate().find(|(_, thread)| {
            thread.thread_id == thread_id && !is_deleted(thread, session_state)
        })?;
    let visible_surface = assistant_surface.unwrap_or(&session_state.assistant_surface);
    if !thread_matches_assistant_surface(thread, visible_surface) {
        return None;
    }
    let session_override = session_override(thread, session_state);
    let mut detail = session_summary(thread, index, session_state);
    let detail_object = detail.as_object_mut()?;
    detail_object.insert(
        "latestAssistantMessage".to_owned(),
        nullable_string_value(latest_assistant_message(thread).as_deref()),
    );
    detail_object.insert(
        "firstUserPrompt".to_owned(),
        nullable_string_value(thread.first_user_prompt.as_deref()),
    );
    detail_object.insert(
        "notificationIds".to_owned(),
        json!(session_notification_ids(session_override, session_state)),
    );
    detail_object.insert(
        "completionCheckID".to_owned(),
        nullable_string_value(effective_completion_check_id(
            session_override,
            session_state,
        )),
    );
    detail_object.insert(
        "completionCheckWaitForReply".to_owned(),
        json!(effective_completion_check_wait_for_reply(
            session_override,
            session_state,
        )),
    );
    detail_object.insert(
        "availableNotifications".to_owned(),
        json!(
            session_state
                .notifications
                .iter()
                .map(notification_summary)
                .collect::<Vec<_>>()
        ),
    );
    detail_object.insert(
        "availableCompletionChecks".to_owned(),
        json!(
            session_state
                .completion_checks
                .iter()
                .map(completion_check_summary)
                .collect::<Vec<_>>()
        ),
    );
    Some(detail)
}

fn latest_assistant_message(thread: &DesktopThread) -> Option<String> {
    thread.assistant_preview.clone()
}

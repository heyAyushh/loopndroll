use crate::assistant::AssistantKind;
use crate::codex::LaunchKind;
use crate::control_plane::DesktopThread;
use crate::mobile::session::{MobileSessionOverride, MobileSessionState};

pub(super) fn session_notification_ids(
    session_override: Option<&MobileSessionOverride>,
    session_state: &MobileSessionState,
) -> Vec<String> {
    session_override
        .map(|override_state| override_state.notification_ids.clone())
        .filter(|ids| !ids.is_empty())
        .or_else(|| {
            session_state
                .global_notification_id
                .clone()
                .map(|id| vec![id])
        })
        .unwrap_or_default()
}

pub(super) fn effective_completion_check_id<'a>(
    session_override: Option<&'a MobileSessionOverride>,
    session_state: &'a MobileSessionState,
) -> Option<&'a str> {
    session_override
        .and_then(|override_state| override_state.completion_check_id.as_deref())
        .or(session_state.global_completion_check_id.as_deref())
}

pub(super) fn effective_completion_check_wait_for_reply(
    session_override: Option<&MobileSessionOverride>,
    session_state: &MobileSessionState,
) -> bool {
    if let Some(session_override) = session_override
        && session_override.completion_check_id.is_some()
    {
        return session_override.completion_check_wait_for_reply;
    }
    session_state.global_completion_check_wait_for_reply
}

pub(super) fn effective_preset<'a>(
    session_override: Option<&'a MobileSessionOverride>,
    session_state: &'a MobileSessionState,
) -> Option<&'a str> {
    session_override
        .and_then(|override_state| override_state.preset.as_deref())
        .or(session_state.global_preset.as_deref())
}

pub(super) fn session_override<'a>(
    thread: &DesktopThread,
    session_state: &'a MobileSessionState,
) -> Option<&'a MobileSessionOverride> {
    session_state.sessions.get(&thread.thread_id)
}

pub(super) fn is_deleted(thread: &DesktopThread, session_state: &MobileSessionState) -> bool {
    session_override(thread, session_state)
        .map(|state| state.deleted)
        .unwrap_or(false)
}

pub(super) fn is_mobile_home_visible_thread(
    thread: &DesktopThread,
    session_state: &MobileSessionState,
) -> bool {
    let codex_subagent = matches!(thread.capabilities.assistant_kind, AssistantKind::Codex)
        && thread.capabilities.spawn.launch_kind == LaunchKind::Subagent;
    !is_deleted(thread, session_state) && !codex_subagent
}

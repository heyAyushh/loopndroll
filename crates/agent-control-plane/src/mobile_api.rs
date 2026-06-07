use std::collections::BTreeMap;

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::assistant::{infer_assistant_client_from_paths, session_matches_assistant_surface};
use crate::control_plane::{DesktopSnapshot, DesktopThread, GrokBuildStatus};
use crate::grok_build::GrokHookStatus;
use crate::mobile_session::{
    ASSISTANT_SURFACES, DEFAULT_REMOTE_PROMPT, MOBILE_SESSION_STATUS_ACTIVE,
    MOBILE_SESSION_STATUS_STOPPED, MobileCompletionCheck, MobileNotificationRoute,
    MobileSessionLifecycle, MobileSessionOverride, MobileSessionState,
};

const HOST_ID: &str = "rust-control-plane";
const HOST_NAME: &str = "Looper";
const DEFAULT_SCOPE: &str = "global";
const THREAD_REF_PREFIX: &str = "T";
const UNKNOWN_TASK_KIND: &str = "unknown";
const PROJECT_SESSION_KIND: &str = "project";
const INSTANT_CHAT_SESSION_KIND: &str = "instant-chat";
const TRANSCRIPT_SOURCE_KIND: &str = "transcript";
const TRANSCRIPT_SOURCE_LABEL: &str = "Transcript";
const AWAIT_REPLY_PRESET: &str = "await-reply";
const ACTIVE_SESSION_STATUS: &str = MOBILE_SESSION_STATUS_ACTIVE;
const ARCHIVED_SESSION_STATUS: &str = "archived";
const STOPPED_SESSION_STATUS: &str = MOBILE_SESSION_STATUS_STOPPED;
const WAITING_SESSION_STATUS: &str = "waiting";
const CODEX_SOURCE_LABEL: &str = "Codex";
const DEVIN_SOURCE_LABEL: &str = "Devin";
const GROK_BUILD_SOURCE_LABEL: &str = "Grok Build";

pub fn mobile_snapshot(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    base_url: &str,
    synced_at: &str,
) -> Value {
    let surface_sessions = mobile_surface_sessions(snapshot, session_state);
    let sessions = surface_sessions
        .get(&session_state.assistant_surface)
        .cloned()
        .unwrap_or_default();

    json!({
        "host": host_summary(base_url, synced_at),
        "globalSettings": mobile_global_settings(session_state),
        "sessions": sessions,
        "surfaceSessions": surface_sessions,
        "notifications": session_state
            .notifications
            .iter()
            .map(notification_summary)
            .collect::<Vec<_>>(),
        "completionChecks": session_state
            .completion_checks
            .iter()
            .map(completion_check_summary)
            .collect::<Vec<_>>(),
        "grokBuild": mobile_grok_build_status(&snapshot.grok_build),
    })
}

fn mobile_surface_sessions(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
) -> BTreeMap<String, Vec<Value>> {
    ASSISTANT_SURFACES
        .iter()
        .map(|surface| {
            (
                (*surface).to_owned(),
                mobile_sessions_for_surface(snapshot, session_state, surface),
            )
        })
        .collect()
}

fn mobile_sessions_for_surface(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    surface: &str,
) -> Vec<Value> {
    snapshot
        .threads
        .iter()
        .enumerate()
        .filter(|(_, thread)| !is_deleted(thread, session_state))
        .filter(|(_, thread)| thread_matches_assistant_surface(thread, surface))
        .map(|(index, thread)| session_summary(thread, index, session_state))
        .collect()
}

fn thread_matches_assistant_surface(thread: &DesktopThread, surface: &str) -> bool {
    session_matches_assistant_surface(
        thread.transcript_path.as_deref(),
        thread.cwd.as_deref(),
        thread.source.as_deref(),
        thread.originator.as_deref(),
        thread.agent_path.as_deref(),
        surface,
    )
}

pub fn mobile_grok_build_status(grok_build: &GrokBuildStatus) -> Value {
    json!({
        "hooks": mobile_grok_hook_status(&grok_build.hooks),
        "sessionCount": grok_build.session_count,
        "activeSessionCount": grok_build.active_session_count,
    })
}

fn mobile_grok_hook_status(hooks: &GrokHookStatus) -> Value {
    json!({
        "health": hooks.health,
        "owner": hooks.owner,
        "registeredEvents": hooks.registered_events,
        "hooksPath": hooks.hooks_path,
    })
}

pub fn mobile_global_settings(session_state: &MobileSessionState) -> Value {
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
        "completionCheckLabel": completion_check_label(
            &session_state.completion_checks,
            session_state.global_completion_check_id.as_deref(),
        ),
        "completionCheckWaitForReply": session_state.global_completion_check_wait_for_reply,
        "assistantSurface": session_state.assistant_surface,
    })
}

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

fn host_summary(base_url: &str, synced_at: &str) -> Value {
    json!({
        "id": HOST_ID,
        "name": HOST_NAME,
        "address": base_url,
        "isReachable": true,
        "lastSyncedAt": synced_at,
    })
}

fn session_summary(
    thread: &DesktopThread,
    index: usize,
    session_state: &MobileSessionState,
) -> Value {
    let session_override = session_override(thread, session_state);
    let kind = if thread.cwd.is_some() {
        PROJECT_SESSION_KIND
    } else {
        INSTANT_CHAT_SESSION_KIND
    };
    let is_archived = session_override
        .and_then(|state| state.archived)
        .unwrap_or(thread.archived);
    let effective_mode = effective_preset(session_override, session_state);
    let lifecycle = session_state.lifecycle.get(&thread.thread_id);
    let assistant_client = infer_assistant_client_from_paths(
        thread.transcript_path.as_deref(),
        thread.cwd.as_deref(),
        thread.source.as_deref(),
        thread.originator.as_deref(),
        thread.agent_path.as_deref(),
    );
    json!({
        "id": thread.thread_id,
        "ref": format!("{THREAD_REF_PREFIX}{}", index + 1),
        "title": session_title(thread),
        "status": session_status(
            is_archived,
            effective_mode,
            lifecycle,
            thread.runtime_status.as_deref()
        ),
        "effectiveMode": effective_mode,
        "lastUpdatedAt": thread_timestamp(thread),
        "assistantPreview": nullable_string_value(thread.assistant_preview.as_deref()),
        "isArchived": is_archived,
        "assistantClient": assistant_client,
        "metadata": {
            "kind": kind,
            "source": thread.source.as_deref().unwrap_or(UNKNOWN_TASK_KIND),
            "sourceDisplayName": source_display_name(&assistant_client),
            "originator": nullable_string_value(thread.originator.as_deref()),
            "projectName": thread.cwd.as_deref().map(project_name_from_path),
            "projectPath": thread.cwd,
            "taskKind": UNKNOWN_TASK_KIND,
            "transcriptAvailable": thread.transcript_path.is_some(),
            "gitRepository": git_repository(thread),
            "pullRequestURL": null,
            "supportsSubagents": true,
            "installedPlugins": [],
            "sources": session_sources(thread),
            "tags": session_tags(kind, source_display_name(&assistant_client)),
        },
    })
}

fn source_display_name(assistant_client: &str) -> &'static str {
    match assistant_client {
        "devin" => DEVIN_SOURCE_LABEL,
        "grok-build" => GROK_BUILD_SOURCE_LABEL,
        _ => CODEX_SOURCE_LABEL,
    }
}

fn notification_summary(notification: &MobileNotificationRoute) -> Value {
    json!({
        "id": notification.id,
        "label": notification.label,
        "channel": notification.channel,
    })
}

fn completion_check_summary(completion_check: &MobileCompletionCheck) -> Value {
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

fn session_notification_ids(
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

fn effective_completion_check_id<'a>(
    session_override: Option<&'a MobileSessionOverride>,
    session_state: &'a MobileSessionState,
) -> Option<&'a str> {
    session_override
        .and_then(|override_state| override_state.completion_check_id.as_deref())
        .or(session_state.global_completion_check_id.as_deref())
}

fn effective_completion_check_wait_for_reply(
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

fn effective_preset<'a>(
    session_override: Option<&'a MobileSessionOverride>,
    session_state: &'a MobileSessionState,
) -> Option<&'a str> {
    session_override
        .and_then(|override_state| override_state.preset.as_deref())
        .or(session_state.global_preset.as_deref())
}

fn nullable_string_value(value: Option<&str>) -> Value {
    value.map(|value| json!(value)).unwrap_or(Value::Null)
}

fn session_title(thread: &DesktopThread) -> String {
    thread
        .title
        .clone()
        .or_else(|| thread.cwd.as_deref().map(project_name_from_path))
        .unwrap_or_else(|| thread.thread_id.clone())
}

fn session_status(
    is_archived: bool,
    effective_mode: Option<&str>,
    lifecycle: Option<&MobileSessionLifecycle>,
    runtime_status: Option<&str>,
) -> &'static str {
    if is_archived {
        return ARCHIVED_SESSION_STATUS;
    }

    if let Some(lifecycle) = lifecycle {
        match lifecycle.status.as_str() {
            MOBILE_SESSION_STATUS_ACTIVE => return ACTIVE_SESSION_STATUS,
            MOBILE_SESSION_STATUS_STOPPED => return inactive_session_status(effective_mode),
            _ => {}
        }
    }

    if runtime_status == Some(MOBILE_SESSION_STATUS_ACTIVE) {
        return ACTIVE_SESSION_STATUS;
    }

    inactive_session_status(effective_mode)
}

fn inactive_session_status(effective_mode: Option<&str>) -> &'static str {
    match effective_mode {
        Some(AWAIT_REPLY_PRESET) => WAITING_SESSION_STATUS,
        _ => STOPPED_SESSION_STATUS,
    }
}

fn thread_timestamp(thread: &DesktopThread) -> String {
    thread
        .updated_at_ms
        .or(thread.created_at_ms)
        .and_then(timestamp_millis_to_iso)
        .unwrap_or_else(current_iso_time)
}

fn timestamp_millis_to_iso(timestamp_millis: i64) -> Option<String> {
    OffsetDateTime::from_unix_timestamp(timestamp_millis / 1_000)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

fn current_iso_time() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn project_name_from_path(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn git_repository(thread: &DesktopThread) -> Value {
    let Some(repository_path) = thread.cwd.as_deref() else {
        return Value::Null;
    };
    if thread.git_branch.is_none() && thread.git_sha.is_none() {
        return Value::Null;
    }
    json!({
        "repositoryName": project_name_from_path(repository_path),
        "repositoryPath": repository_path,
        "remoteURL": null,
        "branch": thread.git_branch,
        "commit": thread.git_sha,
    })
}

fn session_sources(thread: &DesktopThread) -> Vec<Value> {
    let mut sources = thread
        .cwd
        .as_deref()
        .map(|cwd| {
            vec![json!({
                "kind": "cwd",
                "label": "Working Directory",
                "value": cwd,
                "url": null,
            })]
        })
        .unwrap_or_default();
    if let Some(transcript_path) = thread.transcript_path.as_deref() {
        sources.push(json!({
            "kind": TRANSCRIPT_SOURCE_KIND,
            "label": TRANSCRIPT_SOURCE_LABEL,
            "value": transcript_path,
            "url": null,
        }));
    }
    sources
}

fn session_tags(kind: &str, source_display_name: &str) -> Vec<String> {
    vec![kind.to_owned(), source_display_name.to_owned()]
}

fn session_override<'a>(
    thread: &DesktopThread,
    session_state: &'a MobileSessionState,
) -> Option<&'a MobileSessionOverride> {
    session_state.sessions.get(&thread.thread_id)
}

fn is_deleted(thread: &DesktopThread, session_state: &MobileSessionState) -> bool {
    session_override(thread, session_state)
        .map(|state| state.deleted)
        .unwrap_or(false)
}

fn latest_assistant_message(thread: &DesktopThread) -> Option<String> {
    thread.assistant_preview.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grok_build::{GrokHookOwner, GrokHookStatus};

    #[test]
    fn mobile_grok_build_status_uses_mobile_contract() {
        let status = mobile_grok_build_status(&GrokBuildStatus {
            hooks: GrokHookStatus {
                registered_events: vec!["session".to_owned(), "stop".to_owned()],
                active_command: Some("looper hook".to_owned()),
                owner: GrokHookOwner::LooperRust,
                health: "healthy".to_owned(),
                hooks_path: Some("/Users/test/.grok/hooks/looper.json".to_owned()),
            },
            session_count: 3,
            active_session_count: 2,
        });

        assert_eq!(status["sessionCount"], 3);
        assert_eq!(status["activeSessionCount"], 2);
        assert_eq!(status["hooks"]["health"], "healthy");
        assert_eq!(status["hooks"]["owner"], "looper-rust");
        assert_eq!(
            status["hooks"]["registeredEvents"],
            serde_json::json!(["session", "stop"])
        );
    }
}

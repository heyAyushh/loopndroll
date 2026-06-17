use std::collections::BTreeMap;

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::assistant::{
    AssistantKind, assistant_client_matches_surface, infer_assistant_client_from_paths,
};
use crate::control_plane::{DesktopSnapshot, DesktopThread, GrokBuildStatus};
use crate::devin::{
    DevinPromptTransport, DevinThreadIdentity, devin_prompt_transport_for_provider,
    devin_thread_identity_from_public_thread_id,
};
use crate::grok_build::GrokHookStatus;
use crate::mobile_session::{
    ASSISTANT_SURFACES, DEFAULT_REMOTE_PROMPT, MOBILE_SESSION_STATUS_ACTIVE,
    MOBILE_SESSION_STATUS_STOPPED, MobileCompletionCheck, MobileNotificationRoute,
    MobileSessionError, MobileSessionLifecycle, MobileSessionOverride, MobileSessionState,
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
const CLAUDE_SOURCE_LABEL: &str = "Claude Code";
const DEVIN_SOURCE_LABEL: &str = "Devin";
const GROK_BUILD_SOURCE_LABEL: &str = "Grok Build";
const CODEX_ASSISTANT_CLIENT: &str = "codex";
const DEVIN_ASSISTANT_CLIENT: &str = "devin";
const GROK_BUILD_ASSISTANT_CLIENT: &str = "grok-build";
const CLAUDE_CODE_ASSISTANT_CLIENT: &str = "claude-code";
const CURSOR_ASSISTANT_CLIENT: &str = "cursor";
const OPENCLAW_ASSISTANT_CLIENT: &str = "openclaw";
const SUPER_ENGINEERING_ASSISTANT_CLIENT: &str = "super-engineering";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptResumeTarget {
    pub thread_id: String,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDeliveryAction {
    QueueForHook,
    SendDevinAcp { session_id: String },
    ResumeCodex(PromptResumeTarget),
}

pub fn mobile_snapshot(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    base_url: &str,
    grpc_base_urls: &[String],
    synced_at: &str,
) -> Value {
    let surface_sessions = mobile_surface_sessions(snapshot, session_state);
    let sessions = surface_sessions
        .get(&session_state.assistant_surface)
        .cloned()
        .unwrap_or_default();

    json!({
        "host": host_summary(base_url, grpc_base_urls, synced_at),
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
    assistant_client_matches_surface(assistant_client_for_thread(thread), surface)
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
        "siriDefaultSessionId": session_state.siri_default_thread_id,
        "siriDefaultAssistantSurface": session_state.siri_default_assistant_surface,
        "siriCurrentSessionId": session_state.siri_current_thread_id,
        "siriCurrentAssistantSurface": session_state.siri_current_assistant_surface,
        "siriCurrentUpdatedAtMs": session_state.siri_current_updated_at_ms,
    })
}

pub fn validate_mobile_prompt_delivery_target(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<(), MobileSessionError> {
    prompt_delivery_action_for_visible_target(snapshot, session_state, thread_id, assistant_surface)
        .map(|_| ())
}

pub fn prompt_delivery_action_for_target(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id)
        .ok_or(MobileSessionError::SessionNotFound)?;

    prompt_delivery_action_for_thread(thread, session_state)
}

pub fn prompt_delivery_action_for_visible_target(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id)
        .ok_or(MobileSessionError::SessionNotFound)?;

    let visible_surface = assistant_surface.unwrap_or(&session_state.assistant_surface);
    if !thread_matches_assistant_surface(thread, visible_surface) {
        return Err(MobileSessionError::SessionNotFound);
    }

    prompt_delivery_action_for_thread(thread, session_state)
}

fn prompt_delivery_action_for_thread(
    thread: &DesktopThread,
    session_state: &MobileSessionState,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let session_override = session_override(thread, session_state);
    if session_override.map(|state| state.deleted).unwrap_or(false) {
        return Err(MobileSessionError::SessionNotFound);
    }
    let is_archived = session_override
        .and_then(|state| state.archived)
        .unwrap_or(thread.archived);
    if is_archived {
        return Err(MobileSessionError::SessionArchived);
    }
    if !assistant_supports_prompt_delivery(&thread.capabilities.assistant_kind) {
        return Err(MobileSessionError::PromptDeliveryUnavailable);
    }
    if thread.capabilities.assistant_kind == AssistantKind::Codex {
        return Ok(PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
            thread_id: thread.thread_id.clone(),
            cwd: thread.cwd.clone(),
        }));
    }
    let effective_mode = effective_preset(session_override, session_state);
    let lifecycle = session_state.lifecycle.get(&thread.thread_id);
    let status = session_status(
        is_archived,
        effective_mode,
        lifecycle,
        thread.runtime_status.as_deref(),
    );
    if thread.capabilities.assistant_kind == AssistantKind::DevinDesktop {
        return devin_prompt_delivery_action(thread, &status);
    }

    match status {
        ACTIVE_SESSION_STATUS => Ok(PromptDeliveryAction::QueueForHook),
        _ => Err(MobileSessionError::PromptDeliveryUnavailableReason(
            INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON.to_owned(),
        )),
    }
}

const ARCHIVED_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "Archived sessions cannot receive prompts.";
const DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "This Devin provider does not support mobile prompt delivery yet.";
const DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON: &str =
    "This Devin Local session must be running before Looper can deliver prompts through hooks.";
const INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "This session must be running before Looper can queue prompts.";
const UNSUPPORTED_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "This assistant does not support mobile prompt delivery yet.";

struct PromptDeliveryAvailability {
    can_send_prompt: bool,
    unavailable_reason: Option<&'static str>,
}

fn assistant_supports_prompt_delivery(assistant_kind: &AssistantKind) -> bool {
    matches!(
        assistant_kind,
        AssistantKind::Codex
            | AssistantKind::DevinDesktop
            | AssistantKind::GrokBuild
            | AssistantKind::ClaudeCode
    )
}

fn prompt_delivery_availability(
    thread: &DesktopThread,
    is_archived: bool,
    status: &str,
) -> PromptDeliveryAvailability {
    if is_archived {
        return PromptDeliveryAvailability {
            can_send_prompt: false,
            unavailable_reason: Some(ARCHIVED_PROMPT_DELIVERY_UNAVAILABLE_REASON),
        };
    }

    if !assistant_supports_prompt_delivery(&thread.capabilities.assistant_kind) {
        return PromptDeliveryAvailability {
            can_send_prompt: false,
            unavailable_reason: Some(UNSUPPORTED_PROMPT_DELIVERY_UNAVAILABLE_REASON),
        };
    }
    if thread.capabilities.assistant_kind == AssistantKind::DevinDesktop {
        let Some((transport, _session_id)) = devin_prompt_transport(thread) else {
            return PromptDeliveryAvailability {
                can_send_prompt: false,
                unavailable_reason: Some(DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON),
            };
        };
        match transport {
            DevinPromptTransport::DevinAcpBridge => {}
            DevinPromptTransport::DevinHook if status != ACTIVE_SESSION_STATUS => {
                return PromptDeliveryAvailability {
                    can_send_prompt: false,
                    unavailable_reason: Some(
                        DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON,
                    ),
                };
            }
            DevinPromptTransport::CodexAppServer | DevinPromptTransport::DevinHook => {}
        }
    }

    let requires_active_session = !matches!(
        thread.capabilities.assistant_kind,
        AssistantKind::Codex | AssistantKind::DevinDesktop
    );
    if requires_active_session && status != ACTIVE_SESSION_STATUS {
        return PromptDeliveryAvailability {
            can_send_prompt: false,
            unavailable_reason: Some(INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON),
        };
    }

    PromptDeliveryAvailability {
        can_send_prompt: true,
        unavailable_reason: None,
    }
}

fn devin_prompt_transport(thread: &DesktopThread) -> Option<(DevinPromptTransport, String)> {
    let DevinThreadIdentity {
        provider_id,
        session_id,
    } = devin_thread_identity_from_public_thread_id(&thread.thread_id)?;
    let transport = devin_prompt_transport_for_provider(&provider_id)?;
    Some((transport, session_id))
}

fn devin_prompt_delivery_action(
    thread: &DesktopThread,
    status: &str,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let DevinThreadIdentity {
        provider_id,
        session_id,
    } = devin_thread_identity_from_public_thread_id(&thread.thread_id)
        .ok_or(MobileSessionError::PromptDeliveryUnavailable)?;
    let transport = devin_prompt_transport_for_provider(&provider_id).ok_or_else(|| {
        MobileSessionError::PromptDeliveryUnavailableReason(
            DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON.to_owned(),
        )
    })?;
    match transport {
        DevinPromptTransport::CodexAppServer => {
            Ok(PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
                thread_id: session_id,
                cwd: thread.cwd.clone(),
            }))
        }
        DevinPromptTransport::DevinAcpBridge => Ok(PromptDeliveryAction::SendDevinAcp {
            session_id: format!("acp/{provider_id}/{session_id}"),
        }),
        DevinPromptTransport::DevinHook if status == ACTIVE_SESSION_STATUS => {
            Ok(PromptDeliveryAction::QueueForHook)
        }
        DevinPromptTransport::DevinHook => {
            Err(MobileSessionError::PromptDeliveryUnavailableReason(
                DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON.to_owned(),
            ))
        }
    }
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

fn host_summary(base_url: &str, grpc_base_urls: &[String], synced_at: &str) -> Value {
    json!({
        "id": HOST_ID,
        "name": HOST_NAME,
        "address": base_url,
        "grpcAddress": grpc_base_urls.first().cloned().unwrap_or_default(),
        "grpcAddresses": grpc_base_urls,
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
    let status = session_status(
        is_archived,
        effective_mode,
        lifecycle,
        thread.runtime_status.as_deref(),
    );
    let prompt_delivery_availability = prompt_delivery_availability(thread, is_archived, status);
    let last_activity_at = thread_activity_timestamp(thread);
    let last_message_at = thread_message_timestamp(thread);
    let assistant_client = assistant_client_for_thread(thread);
    json!({
        "id": thread.thread_id,
        "ref": format!("{THREAD_REF_PREFIX}{}", index + 1),
        "title": session_title(thread),
        "status": status,
        "effectiveMode": effective_mode,
        "lastUpdatedAt": last_activity_at.clone(),
        "lastActivityAt": last_activity_at,
        "lastMessageAt": nullable_string_value(last_message_at.as_deref()),
        "assistantPreview": nullable_string_value(thread.assistant_preview.as_deref()),
        "isArchived": is_archived,
        "canSendPrompt": prompt_delivery_availability.can_send_prompt,
        "promptDeliveryUnavailableReason": nullable_string_value(
            prompt_delivery_availability.unavailable_reason
        ),
        "assistantClient": assistant_client,
        "goal": mobile_thread_goal_summary(thread.goal.as_ref()),
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

fn assistant_client_for_thread(thread: &DesktopThread) -> &'static str {
    match thread.capabilities.assistant_kind {
        AssistantKind::Codex => CODEX_ASSISTANT_CLIENT,
        AssistantKind::DevinDesktop => DEVIN_ASSISTANT_CLIENT,
        AssistantKind::GrokBuild => GROK_BUILD_ASSISTANT_CLIENT,
        AssistantKind::ClaudeCode => CLAUDE_CODE_ASSISTANT_CLIENT,
        AssistantKind::Cursor => CURSOR_ASSISTANT_CLIENT,
        AssistantKind::OpenClaw => OPENCLAW_ASSISTANT_CLIENT,
        AssistantKind::Superconductor => SUPER_ENGINEERING_ASSISTANT_CLIENT,
        AssistantKind::Unknown => infer_assistant_client_from_paths(
            thread.transcript_path.as_deref(),
            thread.cwd.as_deref(),
            thread.source.as_deref(),
            thread.originator.as_deref(),
            thread.agent_path.as_deref(),
        ),
        _ => CODEX_ASSISTANT_CLIENT,
    }
}

fn mobile_thread_goal_summary(goal: Option<&crate::goals::ThreadGoalSummary>) -> Value {
    goal.map(|goal| {
        json!({
            "id": &goal.id,
            "title": &goal.title,
            "status": &goal.status,
            "lifecycle": &goal.lifecycle,
            "running": goal.running,
            "tokenBudget": goal.token_budget,
            "tokensUsed": goal.tokens_used,
            "timeUsedSeconds": goal.time_used_seconds,
            "updatedAtMs": goal.updated_at_ms,
        })
    })
    .unwrap_or(Value::Null)
}

fn source_display_name(assistant_client: &str) -> &'static str {
    match assistant_client {
        CLAUDE_CODE_ASSISTANT_CLIENT => CLAUDE_SOURCE_LABEL,
        DEVIN_ASSISTANT_CLIENT => DEVIN_SOURCE_LABEL,
        GROK_BUILD_ASSISTANT_CLIENT => GROK_BUILD_SOURCE_LABEL,
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

fn thread_activity_timestamp(thread: &DesktopThread) -> String {
    latest_thread_activity_millis(thread)
        .and_then(timestamp_millis_to_iso)
        .unwrap_or_else(current_iso_time)
}

fn thread_message_timestamp(thread: &DesktopThread) -> Option<String> {
    thread
        .latest_message_at_ms
        .and_then(timestamp_millis_to_iso)
}

fn latest_thread_activity_millis(thread: &DesktopThread) -> Option<i64> {
    [
        thread.updated_at_ms,
        thread.latest_message_at_ms,
        thread.created_at_ms,
    ]
    .into_iter()
    .flatten()
    .max()
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
    use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities};
    use crate::grok_build::{GrokHookOwner, GrokHookStatus};

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
        assert_eq!(summary["canSendPrompt"], true);
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
    }

    #[test]
    fn codex_acp_session_under_devin_paths_stays_on_codex_surface() {
        let mut thread = test_thread("thread-1", AssistantKind::Codex, None);
        thread.source = Some("codex-acp".to_owned());
        thread.originator = Some("Codex ACP via Devin - Next".to_owned());
        thread.transcript_path = Some(
            "/Users/test/Library/Application Support/Devin - Next/User/acp-events/1.ndjson"
                .to_owned(),
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
            "/Users/test/Library/Application Support/Devin - Next/User/acp-events/1.ndjson"
                .to_owned(),
        );
        let session_state = MobileSessionState::default();

        assert!(thread_matches_assistant_surface(&thread, "codex"));
        assert!(!thread_matches_assistant_surface(&thread, "devin"));

        let summary = session_summary(&thread, 0, &session_state);
        assert_eq!(summary["assistantClient"], CURSOR_ASSISTANT_CLIENT);
        assert_eq!(summary["metadata"]["sourceDisplayName"], CODEX_SOURCE_LABEL);
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

    #[test]
    fn prompt_delivery_target_resumes_waiting_codex_sessions() {
        let thread = test_thread("thread-1", AssistantKind::Codex, None);
        let mut session_state =
            session_state_with_lifecycle("thread-1", MOBILE_SESSION_STATUS_STOPPED);
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
            .expect("Devin local sessions should queue via hook transport");
        assert!(matches!(action, PromptDeliveryAction::QueueForHook));
        let summary = session_summary(&thread, 0, &session_state);
        assert_eq!(summary["canSendPrompt"], true);
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
        assert_eq!(summary["canSendPrompt"], false);
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
            .expect("Devin ACP sessions should queue via hook transport");
        assert!(matches!(action, PromptDeliveryAction::QueueForHook));
        let summary = session_summary(&thread, 0, &session_state);
        assert_eq!(summary["canSendPrompt"], true);
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
            .expect("Looper-owned Devin ACP sessions should send directly");
        assert!(matches!(
            action,
            PromptDeliveryAction::SendDevinAcp { session_id } if session_id == "acp/looper/session-1"
        ));
        let summary = session_summary(&thread, 0, &session_state);
        assert_eq!(summary["canSendPrompt"], true);
        assert!(summary["promptDeliveryUnavailableReason"].is_null());
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
        assert_eq!(summary["canSendPrompt"], false);
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
            .expect("active Claude Code sessions should queue via hook transport");
        assert!(matches!(action, PromptDeliveryAction::QueueForHook));
        let summary = session_summary(&thread, 0, &session_state);
        assert_eq!(summary["canSendPrompt"], true);
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
        assert_eq!(summary["canSendPrompt"], false);
        assert_eq!(
            summary["promptDeliveryUnavailableReason"],
            INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON
        );
    }

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

    fn session_state_with_lifecycle(thread_id: &str, status: &str) -> MobileSessionState {
        let mut session_state = MobileSessionState::default();
        session_state.lifecycle.insert(
            thread_id.to_owned(),
            MobileSessionLifecycle {
                status: status.to_owned(),
                updated_at: "2026-06-07T00:00:00Z".to_owned(),
            },
        );
        session_state
    }

    fn test_thread(
        thread_id: &str,
        assistant_kind: AssistantKind,
        runtime_status: Option<&str>,
    ) -> DesktopThread {
        DesktopThread {
            thread_id: thread_id.to_owned(),
            title: Some("Test session".to_owned()),
            cwd: None,
            transcript_path: None,
            source: None,
            originator: None,
            model: None,
            reasoning_effort: None,
            git_sha: None,
            git_branch: None,
            cli_version: None,
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
            created_at_ms: Some(1),
            updated_at_ms: Some(2),
            latest_message_at_ms: Some(3),
            assistant_preview: None,
            runtime_status: runtime_status.map(str::to_owned),
            archived: false,
            goal: None,
            capabilities: ThreadCapabilities {
                thread_id: thread_id.to_owned(),
                assistant_kind,
                tools: Vec::new(),
                mcp_tools: Vec::new(),
                app_tools: Vec::new(),
                automation_tools: Vec::new(),
                spawn: SpawnGraph {
                    parent_thread_id: None,
                    root_thread_id: thread_id.to_owned(),
                    children: Vec::new(),
                    launch_kind: LaunchKind::Main,
                },
                diff: DiffSummary {
                    git_branch: None,
                    git_sha: None,
                    produced_file_changes: false,
                    paths: Vec::new(),
                },
                agent_nickname: None,
                agent_role: None,
                agent_path: None,
            },
        }
    }
}

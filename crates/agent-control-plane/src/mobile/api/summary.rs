use serde_json::{Value, json};

use crate::control_plane::DesktopThread;
use crate::control_plane::session_fsm::{ACTIVE_STATUS as FSM_ACTIVE_STATUS, projected_status};
use crate::mobile::session::{MobileSessionLifecycle, MobileSessionState};

use super::assistant_identity::{assistant_client_for_thread, source_display_name};
use super::availability::prompt_delivery_availability;
use super::metadata::{
    git_repository, project_name_from_path, session_sources, session_supports_subagents,
    session_tags,
};
use super::overrides::{effective_preset, session_override};
use super::time::{
    latest_thread_activity_millis, thread_activity_timestamp, thread_message_timestamp,
};

const THREAD_REF_PREFIX: &str = "T";
const UNKNOWN_TASK_KIND: &str = "unknown";
const PROJECT_SESSION_KIND: &str = "project";
const INSTANT_CHAT_SESSION_KIND: &str = "instant-chat";
pub(super) const ACTIVE_SESSION_STATUS: &str = FSM_ACTIVE_STATUS;
#[cfg(test)]
pub(super) const AWAIT_REPLY_PRESET: &str = "await-reply";

pub(super) fn session_summary(
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
    let last_activity_at_ms = latest_thread_activity_millis(thread);
    let assistant_client = assistant_client_for_thread(thread);
    json!({
        "id": thread.thread_id,
        "ref": format!("{THREAD_REF_PREFIX}{}", index + 1),
        "title": session_title(thread),
        "status": status,
        "effectiveMode": effective_mode,
        "lastUpdatedAt": last_activity_at.clone(),
        "createdAtMs": thread.created_at_ms,
        "updatedAtMs": thread.updated_at_ms,
        "latestMessageAtMs": thread.latest_message_at_ms,
        "lastActivityAtMs": last_activity_at_ms,
        "lastActivityAt": last_activity_at,
        "lastMessageAtMs": thread.latest_message_at_ms,
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
            "sourceDisplayName": source_display_name(assistant_client),
            "assistantKind": thread.capabilities.assistant_kind,
            "originator": nullable_string_value(thread.originator.as_deref()),
            "projectName": thread.cwd.as_deref().map(project_name_from_path),
            "projectPath": thread.cwd,
            "taskKind": UNKNOWN_TASK_KIND,
            "transcriptAvailable": thread.transcript_path.is_some(),
            "gitRepository": git_repository(thread),
            "pullRequestURL": null,
            "supportsSubagents": session_supports_subagents(thread),
            "spawn": {
                "parentThreadId": thread.capabilities.spawn.parent_thread_id,
                "rootThreadId": thread.capabilities.spawn.root_thread_id,
                "children": thread.capabilities.spawn.children,
                "launchKind": thread.capabilities.spawn.launch_kind,
            },
            "installedPlugins": [],
            "sources": session_sources(thread),
            "tags": session_tags(kind, source_display_name(assistant_client)),
        },
    })
}

pub(super) fn nullable_string_value(value: Option<&str>) -> Value {
    value.map(|value| json!(value)).unwrap_or(Value::Null)
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

fn session_title(thread: &DesktopThread) -> String {
    thread
        .title
        .clone()
        .or_else(|| thread.cwd.as_deref().map(project_name_from_path))
        .unwrap_or_else(|| thread.thread_id.clone())
}

pub(super) fn session_status(
    is_archived: bool,
    effective_mode: Option<&str>,
    lifecycle: Option<&MobileSessionLifecycle>,
    runtime_status: Option<&str>,
) -> &'static str {
    projected_status(
        is_archived,
        effective_mode,
        lifecycle.map(|lifecycle| lifecycle.status.as_str()),
        runtime_status,
    )
}

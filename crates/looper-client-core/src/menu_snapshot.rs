use std::cmp::Ordering;

use looper_session_core::SessionMode;
use serde_json::Value;

use crate::error::ClientCoreError;
use crate::model::{
    ClientLocalStateSnapshot, ClientPendingCommand, ClientPendingCommandKind, ClientStateMini,
};

const SESSION_ID_FIELD: &str = "sessionId";
const LEGACY_SESSION_ID_FIELD: &str = "id";
const REF_FIELD: &str = "ref";
const TITLE_FIELD: &str = "title";
const STATUS_FIELD: &str = "status";
const EFFECTIVE_MODE_FIELD: &str = "effectiveMode";
const CAN_SEND_PROMPT_FIELD: &str = "canSendPrompt";
const REPLYABLE_FIELD: &str = "replyable";
const PROMPT_UNAVAILABLE_REASON_FIELD: &str = "promptDeliveryUnavailableReason";
const BLOCKED_GOAL_FIELD: &str = "blockedGoal";
const QUEUE_COUNT_FIELD: &str = "queueCount";
const LIFECYCLE_FIELD: &str = "lifecycle";
const NOTIFICATION_STATUS_FIELD: &str = "notificationStatus";
const IS_ARCHIVED_FIELD: &str = "isArchived";
const ASSISTANT_PREVIEW_FIELD: &str = "assistantPreview";
const METADATA_FIELD: &str = "metadata";
const PROJECT_NAME_FIELD: &str = "projectName";
const PROJECT_PATH_FIELD: &str = "projectPath";
const LAST_ACTIVITY_MS_FIELD: &str = "lastActivityAtMs";
const UPDATED_AT_MS_FIELD: &str = "updatedAtMs";
const NOTIFICATION_TARGET_IDS_FIELD: &str = "targetIds";
const NOTIFICATION_ENABLED_FIELD: &str = "enabled";
const NOTIFICATION_KNOWN_FIELD: &str = "known";
const NOTIFICATION_USES_DEFAULT_FIELD: &str = "usesDefault";
const GOAL_ID_FIELD: &str = "id";
const GOAL_REASON_FIELD: &str = "reason";
const ARCHIVED_STATUS: &str = "archived";

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuBarSessionMiniLocalSnapshot {
    pub latest_seq: i64,
    pub sessions: Vec<ClientMenuBarSessionMini>,
    pub pending_commands: Vec<ClientMenuBarSessionMiniPendingCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuBarHumanStatusProjection {
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub lifecycle: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuSnapshotStreamUpdate {
    pub has_snapshot: bool,
    pub snapshot: ClientMenuBarSessionMiniLocalSnapshot,
    pub sync_reason: String,
    pub should_stop: bool,
    pub error_description: String,
    pub debug_message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuBarSessionMiniPendingCommand {
    pub kind: ClientPendingCommandKind,
    pub client_mutation_id: String,
    pub thread_id: String,
    pub notification_id: String,
    pub prompt: String,
    pub attempt_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuBarSessionMini {
    pub session_id: String,
    pub assistant_surface: String,
    pub seq: i64,
    pub revision: String,
    pub ref_id: String,
    pub title: String,
    pub subtitle: String,
    pub status: String,
    pub effective_mode: String,
    pub has_effective_mode: bool,
    pub replyable: bool,
    pub prompt_unavailable_reason: String,
    pub has_prompt_unavailable_reason: bool,
    pub blocked_goal: ClientMenuBarSessionMiniBlockedGoal,
    pub has_blocked_goal: bool,
    pub queue_count: i32,
    pub lifecycle: String,
    pub has_lifecycle: bool,
    pub notification_status: ClientMenuBarSessionMiniNotificationStatus,
    pub has_notification_status: bool,
    pub notification_title: String,
    pub has_notification_title: bool,
    pub is_archived: bool,
    pub assistant_preview: String,
    pub has_assistant_preview: bool,
    pub project_name: String,
    pub has_project_name: bool,
    pub project_path: String,
    pub has_project_path: bool,
    pub last_activity_at_ms: i64,
    pub has_last_activity_at_ms: bool,
    pub updated_at_ms: i64,
    pub has_updated_at_ms: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuBarSessionMiniBlockedGoal {
    pub id: String,
    pub title: String,
    pub status: String,
    pub lifecycle: String,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMenuBarSessionMiniNotificationStatus {
    pub enabled: bool,
    pub target_ids: Vec<String>,
    pub uses_default: bool,
}

#[uniffi::export]
pub fn reduce_state_minis_menu_snapshot(
    snapshot: ClientLocalStateSnapshot,
) -> Result<ClientMenuBarSessionMiniLocalSnapshot, ClientCoreError> {
    let mut sessions = snapshot
        .sessions
        .iter()
        .filter_map(|session| decode_session_mini(session).ok())
        .collect::<Vec<_>>();
    sessions.sort_by(compare_menu_sessions);

    Ok(ClientMenuBarSessionMiniLocalSnapshot {
        latest_seq: snapshot.latest_seq,
        sessions,
        pending_commands: snapshot
            .pending_commands
            .iter()
            .map(ClientMenuBarSessionMiniPendingCommand::from)
            .collect(),
    })
}

#[uniffi::export]
pub fn reduce_menu_snapshot_human_status(
    snapshot: ClientMenuBarSessionMiniLocalSnapshot,
    mobile_ready: bool,
    detach_on_quit: bool,
) -> ClientMenuBarHumanStatusProjection {
    let active_sessions = snapshot
        .sessions
        .iter()
        .filter(|session| !session.is_archived)
        .collect::<Vec<_>>();
    let blocked_count = active_sessions
        .iter()
        .filter(|session| session.has_blocked_goal)
        .count();
    let replyable_count = active_sessions
        .iter()
        .filter(|session| session.replyable)
        .count();
    let pending_count = snapshot.pending_commands.len();
    let detail = [
        "source=sessionMini".to_owned(),
        format!("seq={}", snapshot.latest_seq),
        format!("active={}", active_sessions.len()),
        format!("replyable={replyable_count}"),
        format!("blocked={blocked_count}"),
        format!("pending={pending_count}"),
        format!("iPhone={}", if mobile_ready { "ready" } else { "unknown" }),
    ]
    .join(" ");
    let needs_attention = blocked_count > 0;

    ClientMenuBarHumanStatusProjection {
        kind: if needs_attention {
            "needs_attention"
        } else {
            "ready"
        }
        .to_owned(),
        title: if needs_attention {
            "Needs attention"
        } else {
            "Realtime"
        }
        .to_owned(),
        detail,
        lifecycle: lifecycle_text(detach_on_quit),
    }
}

fn decode_session_mini(
    record: &ClientStateMini,
) -> Result<ClientMenuBarSessionMini, ClientCoreError> {
    let payload: Value = serde_json::from_str(&record.payload_json)
        .map_err(|_| ClientCoreError::InvalidStateMiniPayloadJson)?;
    let payload_session_id =
        first_nonblank_string(&payload, &[SESSION_ID_FIELD, LEGACY_SESSION_ID_FIELD])
            .ok_or(ClientCoreError::InvalidStateMiniPayloadJson)?;
    if payload_session_id != record.session_id {
        return Err(ClientCoreError::StateMiniSessionIdMismatch);
    }

    let ref_id =
        first_nonblank_string(&payload, &[REF_FIELD]).unwrap_or_else(|| payload_session_id.clone());
    let title = display_title(&payload, &ref_id, &record.session_id);
    let status = optional_string(&payload, STATUS_FIELD).unwrap_or_default();
    let effective_mode = optional_string(&payload, EFFECTIVE_MODE_FIELD);
    let prompt_unavailable_reason = optional_string(&payload, PROMPT_UNAVAILABLE_REASON_FIELD);
    let blocked_goal = blocked_goal(&payload);
    let lifecycle = optional_string(&payload, LIFECYCLE_FIELD);
    let notification_status = notification_status(&payload);
    let replyable = optional_bool(&payload, REPLYABLE_FIELD)
        .or_else(|| optional_bool(&payload, CAN_SEND_PROMPT_FIELD))
        .unwrap_or(false);
    let queue_count = optional_i64(&payload, QUEUE_COUNT_FIELD)
        .and_then(|count| i32::try_from(count).ok())
        .unwrap_or_default();
    let is_archived =
        optional_bool(&payload, IS_ARCHIVED_FIELD).unwrap_or_else(|| status == ARCHIVED_STATUS);
    let assistant_preview = optional_string(&payload, ASSISTANT_PREVIEW_FIELD);
    let project_name = metadata_string(&payload, PROJECT_NAME_FIELD);
    let project_path = metadata_string(&payload, PROJECT_PATH_FIELD);
    let last_activity_at_ms = optional_i64(&payload, LAST_ACTIVITY_MS_FIELD);
    let updated_at_ms = optional_i64(&payload, UPDATED_AT_MS_FIELD);
    let notification_title = notification_status_text(notification_status.as_ref());
    let subtitle = session_subtitle(SessionSubtitleInput {
        assistant_surface: &record.assistant_surface,
        effective_mode: effective_mode.as_deref(),
        replyable,
        prompt_unavailable_reason: prompt_unavailable_reason.as_deref(),
        blocked_goal: blocked_goal.as_ref(),
        queue_count,
        lifecycle: lifecycle.as_deref(),
        notification_title: notification_title.as_deref(),
        project_name: project_name.as_deref(),
        is_archived,
    });

    Ok(ClientMenuBarSessionMini {
        session_id: record.session_id.clone(),
        assistant_surface: record.assistant_surface.clone(),
        seq: record.seq,
        revision: record.revision.clone(),
        ref_id,
        title,
        subtitle,
        status: status.clone(),
        effective_mode: effective_mode.clone().unwrap_or_default(),
        has_effective_mode: effective_mode.is_some(),
        replyable,
        prompt_unavailable_reason: prompt_unavailable_reason.clone().unwrap_or_default(),
        has_prompt_unavailable_reason: prompt_unavailable_reason.is_some(),
        blocked_goal: blocked_goal
            .clone()
            .unwrap_or_else(ClientMenuBarSessionMiniBlockedGoal::empty),
        has_blocked_goal: blocked_goal.is_some(),
        queue_count,
        lifecycle: lifecycle.clone().unwrap_or_default(),
        has_lifecycle: lifecycle.is_some(),
        notification_status: notification_status
            .clone()
            .unwrap_or_else(ClientMenuBarSessionMiniNotificationStatus::empty),
        has_notification_status: notification_status.is_some(),
        notification_title: notification_title.clone().unwrap_or_default(),
        has_notification_title: notification_title.is_some(),
        is_archived,
        assistant_preview: assistant_preview.clone().unwrap_or_default(),
        has_assistant_preview: assistant_preview.is_some(),
        project_name: project_name.clone().unwrap_or_default(),
        has_project_name: project_name.is_some(),
        project_path: project_path.clone().unwrap_or_default(),
        has_project_path: project_path.is_some(),
        last_activity_at_ms: last_activity_at_ms.unwrap_or_default(),
        has_last_activity_at_ms: last_activity_at_ms.is_some(),
        updated_at_ms: updated_at_ms.unwrap_or_default(),
        has_updated_at_ms: updated_at_ms.is_some(),
    })
}

fn display_title(payload: &Value, ref_id: &str, fallback_id: &str) -> String {
    [TITLE_FIELD, REF_FIELD]
        .iter()
        .find_map(|field| optional_string(payload, field))
        .or_else(|| nonblank_string(ref_id))
        .unwrap_or_else(|| fallback_id.to_owned())
}

struct SessionSubtitleInput<'a> {
    assistant_surface: &'a str,
    effective_mode: Option<&'a str>,
    replyable: bool,
    prompt_unavailable_reason: Option<&'a str>,
    blocked_goal: Option<&'a ClientMenuBarSessionMiniBlockedGoal>,
    queue_count: i32,
    lifecycle: Option<&'a str>,
    notification_title: Option<&'a str>,
    project_name: Option<&'a str>,
    is_archived: bool,
}

fn session_subtitle(input: SessionSubtitleInput<'_>) -> String {
    let subtitle = [
        input.effective_mode.and_then(mode_text),
        if input.replyable {
            Some("Reply ready".to_owned())
        } else {
            input.prompt_unavailable_reason.and_then(nonblank_string)
        },
        input.blocked_goal.and_then(blocked_goal_text),
        queue_text(input.queue_count),
        input.lifecycle.and_then(lifecycle_state_text),
        input.notification_title.and_then(nonblank_string),
        input.project_name.and_then(nonblank_string),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" - ");
    let fallback = if subtitle.is_empty() {
        input.assistant_surface.to_owned()
    } else {
        subtitle
    };
    if input.is_archived {
        format!("Archived - {fallback}")
    } else {
        fallback
    }
}

fn mode_text(mode: &str) -> Option<String> {
    let mode = nonblank_string(mode)?;
    Some(
        SessionMode::parse(&mode)
            .map(session_mode_title)
            .unwrap_or_else(|_| mode),
    )
}

fn session_mode_title(mode: SessionMode) -> String {
    match mode {
        SessionMode::Infinite => "Infinite",
        SessionMode::AwaitReply => "Await Reply",
        SessionMode::CompletionChecks => "Completion Checks",
        SessionMode::MaxTurns1 => "Max Turns 1",
        SessionMode::MaxTurns2 => "Max Turns 2",
        SessionMode::MaxTurns3 => "Max Turns 3",
    }
    .to_owned()
}

fn blocked_goal_text(goal: &ClientMenuBarSessionMiniBlockedGoal) -> Option<String> {
    nonblank_string(&goal.title)
        .or_else(|| nonblank_string(&goal.reason))
        .or_else(|| nonblank_string(&goal.status))
        .map(|text| format!("Blocked: {text}"))
}

fn queue_text(queue_count: i32) -> Option<String> {
    if queue_count > 0 {
        Some(format!("Queue {queue_count}"))
    } else {
        None
    }
}

fn lifecycle_state_text(lifecycle: &str) -> Option<String> {
    nonblank_string(lifecycle).map(|text| format!("State {text}"))
}

fn lifecycle_text(detach_on_quit: bool) -> String {
    if detach_on_quit {
        "Detached on quit"
    } else {
        "Quit stops server"
    }
    .to_owned()
}

fn notification_status_text(
    status: Option<&ClientMenuBarSessionMiniNotificationStatus>,
) -> Option<String> {
    let status = status?;
    if !status.enabled {
        return Some("Notify off".to_owned());
    }
    if status.target_ids.is_empty() {
        return Some("Notify ready".to_owned());
    }
    Some(format!("Notify {}", status.target_ids.join("/")))
}

fn blocked_goal(payload: &Value) -> Option<ClientMenuBarSessionMiniBlockedGoal> {
    let goal = payload.get(BLOCKED_GOAL_FIELD)?.as_object()?;
    Some(ClientMenuBarSessionMiniBlockedGoal {
        id: optional_object_string(goal, GOAL_ID_FIELD).unwrap_or_default(),
        title: optional_object_string(goal, TITLE_FIELD).unwrap_or_default(),
        status: optional_object_string(goal, STATUS_FIELD).unwrap_or_default(),
        lifecycle: optional_object_string(goal, LIFECYCLE_FIELD).unwrap_or_default(),
        reason: optional_object_string(goal, GOAL_REASON_FIELD).unwrap_or_default(),
    })
}

fn notification_status(payload: &Value) -> Option<ClientMenuBarSessionMiniNotificationStatus> {
    let status = payload.get(NOTIFICATION_STATUS_FIELD)?.as_object()?;
    if object_bool(status, NOTIFICATION_KNOWN_FIELD) == Some(false) {
        return None;
    }
    let target_ids = status
        .get(NOTIFICATION_TARGET_IDS_FIELD)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .filter_map(nonblank_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(ClientMenuBarSessionMiniNotificationStatus {
        enabled: object_bool(status, NOTIFICATION_ENABLED_FIELD).unwrap_or(false),
        target_ids,
        uses_default: object_bool(status, NOTIFICATION_USES_DEFAULT_FIELD).unwrap_or(false),
    })
}

fn compare_menu_sessions(
    left: &ClientMenuBarSessionMini,
    right: &ClientMenuBarSessionMini,
) -> Ordering {
    let left_activity = menu_activity(left);
    let right_activity = menu_activity(right);
    right_activity
        .cmp(&left_activity)
        .then_with(|| right.seq.cmp(&left.seq))
        .then_with(|| left.session_id.cmp(&right.session_id))
}

fn menu_activity(session: &ClientMenuBarSessionMini) -> i64 {
    if session.has_last_activity_at_ms {
        return session.last_activity_at_ms;
    }
    if session.has_updated_at_ms {
        return session.updated_at_ms;
    }
    session.seq
}

fn first_nonblank_string(payload: &Value, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| optional_string(payload, field))
}

fn optional_string(payload: &Value, field: &str) -> Option<String> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .and_then(nonblank_string)
}

fn metadata_string(payload: &Value, field: &str) -> Option<String> {
    payload
        .get(METADATA_FIELD)
        .and_then(Value::as_object)
        .and_then(|metadata| optional_object_string(metadata, field))
}

fn optional_object_string(object: &serde_json::Map<String, Value>, field: &str) -> Option<String> {
    object
        .get(field)
        .and_then(Value::as_str)
        .and_then(nonblank_string)
}

fn nonblank_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn optional_bool(payload: &Value, field: &str) -> Option<bool> {
    payload.get(field).and_then(Value::as_bool)
}

fn object_bool(object: &serde_json::Map<String, Value>, field: &str) -> Option<bool> {
    object.get(field).and_then(Value::as_bool)
}

fn optional_i64(payload: &Value, field: &str) -> Option<i64> {
    payload.get(field).and_then(Value::as_i64)
}

impl ClientMenuBarSessionMiniBlockedGoal {
    fn empty() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            status: String::new(),
            lifecycle: String::new(),
            reason: String::new(),
        }
    }
}

impl ClientMenuBarSessionMiniNotificationStatus {
    fn empty() -> Self {
        Self {
            enabled: false,
            target_ids: Vec::new(),
            uses_default: false,
        }
    }
}

impl From<&ClientPendingCommand> for ClientMenuBarSessionMiniPendingCommand {
    fn from(command: &ClientPendingCommand) -> Self {
        Self {
            kind: command.kind,
            client_mutation_id: command.client_mutation_id.clone(),
            thread_id: command.thread_id.clone(),
            notification_id: command.notification_id.clone(),
            prompt: command.prompt.clone(),
            attempt_count: command.attempt_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClientPendingCommand;
    use serde_json::json;

    #[test]
    fn empty_minis_return_empty_menu_snapshot() {
        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 0,
            sessions: vec![],
            pending_commands: vec![],
            server_time: String::new(),
        })
        .expect("empty menu snapshot");

        assert_eq!(snapshot.latest_seq, 0);
        assert!(snapshot.sessions.is_empty());
        assert!(snapshot.pending_commands.is_empty());
    }

    #[test]
    fn projects_menu_rows_from_state_mini_payloads() {
        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 12,
            sessions: vec![mini(
                "thread-codex",
                "codex",
                12,
                payload_json("thread-codex", "S1", 900),
            )],
            pending_commands: vec![pending_prompt()],
            server_time: String::new(),
        })
        .expect("menu snapshot");

        let session = snapshot.sessions.first().expect("session");
        assert_eq!(session.session_id, "thread-codex");
        assert_eq!(session.assistant_surface, "codex");
        assert_eq!(session.ref_id, "S1");
        assert_eq!(session.title, "Project Alpha");
        assert_eq!(
            session.subtitle,
            "await_reply - Reply ready - Blocked: Ship realtime - Queue 2 - State active - Notify desktop - Looper"
        );
        assert_eq!(session.status, "running");
        assert_eq!(session.effective_mode, "await_reply");
        assert!(session.has_effective_mode);
        assert!(session.replyable);
        assert_eq!(session.blocked_goal.title, "Ship realtime");
        assert!(session.has_blocked_goal);
        assert_eq!(session.queue_count, 2);
        assert_eq!(session.lifecycle, "active");
        assert!(session.has_lifecycle);
        assert!(session.notification_status.enabled);
        assert_eq!(session.notification_status.target_ids, vec!["desktop"]);
        assert!(session.has_notification_status);
        assert_eq!(session.notification_title, "Notify desktop");
        assert!(session.has_notification_title);
        assert_eq!(session.project_name, "Looper");
        assert_eq!(session.project_path, "/Users/ay/Documents/looper");
        assert_eq!(session.last_activity_at_ms, 900);
        assert_eq!(snapshot.pending_commands[0].prompt, "Continue");
    }

    #[test]
    fn sorts_menu_rows_by_activity_seq_and_session_id() {
        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 14,
            sessions: vec![
                mini("older", "codex", 11, minimal_payload("older", 100, None)),
                mini("tie-b", "codex", 12, minimal_payload("tie-b", 200, None)),
                mini("newer", "codex", 13, minimal_payload("newer", 300, None)),
                mini("tie-a", "codex", 14, minimal_payload("tie-a", 200, None)),
                mini(
                    "seq-fallback",
                    "codex",
                    15,
                    minimal_payload("seq-fallback", 0, Some(0)),
                ),
            ],
            pending_commands: vec![],
            server_time: String::new(),
        })
        .expect("menu snapshot");
        let session_ids = snapshot
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            session_ids,
            vec!["newer", "tie-a", "tie-b", "older", "seq-fallback"]
        );
    }

    #[test]
    fn projects_menu_subtitle_display_text_in_rust() {
        let mut payload = serde_json::from_str::<Value>(&minimal_payload("archived", 100, None))
            .expect("payload");
        let object = payload.as_object_mut().expect("object");
        object.insert(
            EFFECTIVE_MODE_FIELD.to_owned(),
            Value::String("await-reply".to_owned()),
        );
        object.insert(
            PROMPT_UNAVAILABLE_REASON_FIELD.to_owned(),
            Value::String("Waiting for local hook".to_owned()),
        );
        object.insert(IS_ARCHIVED_FIELD.to_owned(), Value::Bool(true));
        object.insert(
            NOTIFICATION_STATUS_FIELD.to_owned(),
            json!({
                "enabled": false,
                "targetIds": [],
                "usesDefault": false,
            }),
        );
        object.insert(METADATA_FIELD.to_owned(), json!({}));
        let metadata = object
            .get_mut(METADATA_FIELD)
            .and_then(Value::as_object_mut)
            .expect("metadata");
        metadata.insert(
            PROJECT_NAME_FIELD.to_owned(),
            Value::String("Looper".to_owned()),
        );

        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 15,
            sessions: vec![mini("archived", "codex", 15, payload.to_string())],
            pending_commands: vec![],
            server_time: String::new(),
        })
        .expect("menu snapshot");
        let session = snapshot.sessions.first().expect("session");

        assert_eq!(
            session.subtitle,
            "Archived - Await Reply - Waiting for local hook - Notify off - Looper"
        );
        assert_eq!(session.notification_title, "Notify off");
    }

    #[test]
    fn projects_menu_human_status_in_rust() {
        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 201,
            sessions: vec![
                mini(
                    "thread-blocked",
                    "codex",
                    201,
                    payload_json("thread-blocked", "S1", 201),
                ),
                mini(
                    "thread-archived",
                    "codex",
                    200,
                    archived_payload("thread-archived"),
                ),
            ],
            pending_commands: vec![pending_prompt()],
            server_time: String::new(),
        })
        .expect("menu snapshot");
        let status = reduce_menu_snapshot_human_status(snapshot, true, true);

        assert_eq!(status.kind, "needs_attention");
        assert_eq!(status.title, "Needs attention");
        assert_eq!(status.lifecycle, "Detached on quit");
        assert!(status.detail.contains("source=sessionMini"));
        assert!(status.detail.contains("seq=201"));
        assert!(status.detail.contains("active=1"));
        assert!(status.detail.contains("replyable=1"));
        assert!(status.detail.contains("blocked=1"));
        assert!(status.detail.contains("pending=1"));
        assert!(status.detail.contains("iPhone=ready"));
    }

    #[test]
    fn projects_ready_menu_human_status_in_rust() {
        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 202,
            sessions: vec![mini(
                "thread-ready",
                "codex",
                202,
                minimal_payload("thread-ready", 202, None),
            )],
            pending_commands: vec![],
            server_time: String::new(),
        })
        .expect("menu snapshot");
        let status = reduce_menu_snapshot_human_status(snapshot, false, false);

        assert_eq!(status.kind, "ready");
        assert_eq!(status.title, "Realtime");
        assert_eq!(status.lifecycle, "Quit stops server");
        assert!(status.detail.contains("iPhone=unknown"));
    }

    #[test]
    fn skips_stale_invalid_minis() {
        let snapshot = reduce_state_minis_menu_snapshot(ClientLocalStateSnapshot {
            latest_seq: 2,
            sessions: vec![
                mini(
                    "envelope-id",
                    "codex",
                    1,
                    minimal_payload("payload-id", 100, None),
                ),
                mini(
                    "thread-valid",
                    "codex",
                    2,
                    minimal_payload("thread-valid", 200, None),
                ),
            ],
            pending_commands: vec![],
            server_time: String::new(),
        })
        .expect("menu projection skips invalid mini");

        assert_eq!(snapshot.latest_seq, 2);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].session_id, "thread-valid");
    }

    fn mini(
        session_id: &str,
        assistant_surface: &str,
        seq: i64,
        payload_json: String,
    ) -> ClientStateMini {
        ClientStateMini {
            session_id: session_id.to_owned(),
            assistant_surface: assistant_surface.to_owned(),
            seq,
            revision: format!("rev-{seq}"),
            payload_json,
        }
    }

    fn pending_prompt() -> ClientPendingCommand {
        ClientPendingCommand {
            kind: ClientPendingCommandKind::SendSessionPrompt,
            client_mutation_id: "mutation-1".to_owned(),
            thread_id: "thread-codex".to_owned(),
            preset: String::new(),
            assistant_surface: "codex".to_owned(),
            prompt_intent: "queue".to_owned(),
            prompt: "Continue".to_owned(),
            notification_id: String::new(),
            notification_target_ids: Vec::new(),
            archived: false,
            attempt_count: 1,
        }
    }

    fn payload_json(session_id: &str, reference: &str, activity_ms: i64) -> String {
        json!({
            "id": session_id,
            "ref": reference,
            "title": "  Project Alpha  ",
            "status": "running",
            "effectiveMode": "await_reply",
            "replyable": true,
            "promptDeliveryUnavailableReason": "",
            "blockedGoal": {
                "id": "goal-1",
                "title": "Ship realtime",
                "status": "blocked",
                "lifecycle": "waiting",
                "reason": "needs ACK"
            },
            "queueCount": 2,
            "lifecycle": "active",
            "notificationStatus": {
                "enabled": true,
                "targetIds": ["desktop", ""],
                "usesDefault": false
            },
            "assistantPreview": "last answer",
            "metadata": {
                "projectName": "Looper",
                "projectPath": "/Users/ay/Documents/looper"
            },
            "lastActivityAtMs": activity_ms,
            "updatedAtMs": activity_ms - 1
        })
        .to_string()
    }

    fn minimal_payload(
        session_id: &str,
        last_activity_ms: i64,
        updated_at_ms: Option<i64>,
    ) -> String {
        let mut payload = json!({
            "id": session_id,
            "ref": session_id,
            "status": "running",
            "lastActivityAtMs": last_activity_ms
        });
        if let Some(updated_at_ms) = updated_at_ms {
            payload["lastActivityAtMs"] = Value::Null;
            payload["updatedAtMs"] = json!(updated_at_ms);
        }
        payload.to_string()
    }

    fn archived_payload(session_id: &str) -> String {
        json!({
            "id": session_id,
            "title": "Archived",
            "isArchived": true,
            "lastActivityAtMs": 200,
        })
        .to_string()
    }
}

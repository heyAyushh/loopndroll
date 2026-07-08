use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::control_plane::session_fsm::{ACTIVE_STATUS, STOPPED_STATUS};
use crate::control_plane::{DesktopSnapshot, DesktopThread};
use crate::events::{MobileSessionMiniProjectionInput, MobileSessionMiniRecord};
use crate::mobile::session::{
    MobileSessionState, NOTIFICATION_TARGET_IPHONE, NOTIFICATION_TARGET_MACOS,
};

use super::overrides::{is_mobile_home_visible_thread, session_override};
use super::summary::session_summary;

const BLOCKED_GOAL_STATUSES: &[&str] = &["blocked", "usage-limited", "budget-limited", "unmet"];
/// `looper_session_core` exports `ACTIVE_STATUS`/`STOPPED_STATUS` but keeps its "waiting" label
/// private (`SessionState::display_status` / `inactive_projected_status`), so we mirror the
/// literal here rather than reach into a private item of another crate.
const WAITING_LIVE_STATUS: &str = "waiting";
/// A session can only present as "active" or "waiting" on the mobile mini projection while
/// there is evidence it is actually alive. Some surfaces have no live-process signal at all —
/// codex threads (`codex_thread_to_desktop_thread`) always set `runtime_status: None` and rely
/// entirely on hook-driven FSM lifecycle rows, so a missed terminating hook (crash, killed
/// process, ...) leaves the projected status "active"/"waiting" forever with nothing left to
/// correct it. Once a session has gone this long without fresh activity and without live
/// runtime evidence backing up "active"/"waiting", the mini projection presents it as stopped
/// instead. This is purely a presentation decay for the projection: the underlying
/// state/lifecycle rows a surface owns are never rewritten.
const STALE_LIVE_STATUS_DECAY_MS: i64 = 30 * 60 * 1000;
const EMBEDDED_CONTROL_FIELDS: &[&str] = &["revision", "globalSettings"];
const DETAIL_METADATA_FIELDS: &[&str] = &["spawn", "sources", "tags"];
const COMPACT_SESSION_MINI_FIELDS: &[&str] = &[
    "id",
    "sessionId",
    "seq",
    "assistantClient",
    "assistantSurface",
    "ref",
    "status",
    "effectiveMode",
    "canSendPrompt",
    "lastUpdatedAt",
    "createdAtMs",
    "updatedAtMs",
    "latestMessageAtMs",
    "lastActivityAtMs",
    "lastActivityAt",
    "lastMessageAtMs",
    "lastMessageAt",
    "isArchived",
    "title",
    "promptDeliveryUnavailableReason",
    "assistantPreview",
    "metadata",
    "replyable",
    "goal",
    "blockedGoal",
    "queueCount",
    "lifecycle",
    "notificationStatus",
];
#[cfg(test)]
const SESSION_MINI_CONTROL_FRAME_MAX_BYTES: usize = 512 * 1024;
#[cfg(test)]
const LEGACY_REPLAY_FIXTURE_SESSION_COUNT: usize = 260;
#[cfg(test)]
const LEGACY_REPLAY_REVISION_CHARS: usize = 6_000;
#[cfg(test)]
const LEGACY_REPLAY_DETAIL_CHARS: usize = 1_000;
#[cfg(test)]
const LEGACY_REPLAY_TAG_CHARS: usize = 100;
#[cfg(test)]
const METADATA_DETAIL_FIXTURE_TAG_COUNT: usize = 10;

pub fn session_mini_projection_inputs(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    queued_prompt_counts: &BTreeMap<String, i64>,
    seq: i64,
    revision: &str,
) -> Vec<MobileSessionMiniProjectionInput> {
    mobile_session_minis(snapshot, session_state, queued_prompt_counts, seq, revision)
        .into_iter()
        .filter_map(|mini| {
            let session_id = mini
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_owned)?;
            let assistant_surface = mini
                .get("assistantSurface")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Some(MobileSessionMiniProjectionInput {
                session_id,
                assistant_surface,
                body_json: mini,
            })
        })
        .collect()
}

pub fn session_mini_projection_inputs_from_records(
    records: &[MobileSessionMiniRecord],
) -> Vec<MobileSessionMiniProjectionInput> {
    records
        .iter()
        .filter_map(|record| {
            let body_json = serde_json::from_str::<Value>(&record.body_json).ok()?;
            Some(MobileSessionMiniProjectionInput {
                session_id: record.session_id.clone(),
                assistant_surface: record.assistant_surface.clone(),
                body_json,
            })
        })
        .collect()
}

pub fn mobile_session_minis(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    queued_prompt_counts: &BTreeMap<String, i64>,
    latest_seq: i64,
    revision: &str,
) -> Vec<Value> {
    session_mini_values(
        snapshot,
        session_state,
        queued_prompt_counts,
        latest_seq,
        revision,
    )
}

pub fn mobile_session_mini_snapshot(
    latest_seq: i64,
    revision: &str,
    server_time: &str,
    records: &[MobileSessionMiniRecord],
) -> Value {
    mobile_session_mini_response(latest_seq, revision, server_time, records, true)
}

pub fn mobile_session_mini_delta(
    latest_seq: i64,
    revision: &str,
    server_time: &str,
    records: &[MobileSessionMiniRecord],
    replace: bool,
) -> Value {
    mobile_session_mini_response(latest_seq, revision, server_time, records, replace)
}

pub fn compact_mobile_session_mini_record(record: &MobileSessionMiniRecord) -> Option<String> {
    let payload = serde_json::from_str::<Value>(&record.body_json).ok()?;
    compact_session_mini_payload(payload).map(|payload| payload.to_string())
}

pub fn session_mini_records_contain_session(
    records: &[MobileSessionMiniRecord],
    session_id: &str,
    assistant_surface: Option<&str>,
) -> Option<bool> {
    if records.is_empty() {
        return None;
    }

    Some(records.iter().any(|record| {
        record.session_id == session_id
            && assistant_surface
                .map(|surface| record.assistant_surface == surface)
                .unwrap_or(true)
    }))
}

pub fn session_mini_records_allow_prompt(
    records: &[MobileSessionMiniRecord],
    session_id: &str,
    assistant_surface: Option<&str>,
) -> Option<bool> {
    if records.is_empty() {
        return None;
    }

    records
        .iter()
        .find(|record| {
            record.session_id == session_id
                && assistant_surface
                    .map(|surface| record.assistant_surface == surface)
                    .unwrap_or(true)
        })
        .map(session_mini_record_allows_prompt)
        .or(Some(false))
}

pub fn session_mini_records_allow_reply_mode_prompt(
    records: &[MobileSessionMiniRecord],
    session_id: &str,
    assistant_surface: Option<&str>,
) -> Option<bool> {
    if records.is_empty() {
        return None;
    }

    records
        .iter()
        .find(|record| {
            record.session_id == session_id
                && assistant_surface
                    .map(|surface| record.assistant_surface == surface)
                    .unwrap_or(true)
        })
        .map(|record| {
            session_mini_record_allows_prompt(record) && session_mini_record_has_mode(record)
        })
        .or(Some(false))
}

pub fn session_mini_projection_inputs_with_mode(
    records: &[MobileSessionMiniRecord],
    session_id: &str,
    preset: Option<&str>,
) -> Vec<MobileSessionMiniProjectionInput> {
    records
        .iter()
        .filter(|record| record.session_id == session_id)
        .filter_map(|record| {
            let mut body = serde_json::from_str::<Value>(&record.body_json).ok()?;
            let body_object = body.as_object_mut()?;
            body_object.insert(
                "effectiveMode".to_owned(),
                preset.map(Value::from).unwrap_or(Value::Null),
            );
            Some(MobileSessionMiniProjectionInput {
                session_id: record.session_id.clone(),
                assistant_surface: record.assistant_surface.clone(),
                body_json: body,
            })
        })
        .collect()
}

pub fn session_mini_projection_inputs_with_mobile_state(
    records: &[MobileSessionMiniRecord],
    session_id: &str,
    session_state: &MobileSessionState,
) -> Vec<MobileSessionMiniProjectionInput> {
    let override_state = session_state.sessions.get(session_id);
    if override_state
        .map(|state| state.deleted || state.archived.unwrap_or(false))
        .unwrap_or(false)
    {
        return Vec::new();
    }

    records
        .iter()
        .filter(|record| record.session_id == session_id)
        .filter_map(|record| {
            let mut body = serde_json::from_str::<Value>(&record.body_json).ok()?;
            refresh_cached_mobile_state_overlay(&mut body, session_id, session_state)?;
            Some(MobileSessionMiniProjectionInput {
                session_id: record.session_id.clone(),
                assistant_surface: record.assistant_surface.clone(),
                body_json: body,
            })
        })
        .collect()
}

fn refresh_cached_mobile_state_overlay(
    body: &mut Value,
    session_id: &str,
    session_state: &MobileSessionState,
) -> Option<()> {
    let body_object = body.as_object_mut()?;
    let override_state = session_state.sessions.get(session_id);

    if let Some(archived) = override_state.and_then(|state| state.archived) {
        body_object.insert("isArchived".to_owned(), json!(archived));
    }
    if let Some(lifecycle) = session_state.lifecycle.get(session_id) {
        body_object.insert("lifecycle".to_owned(), json!(lifecycle.status));
        body_object.insert("status".to_owned(), json!(lifecycle.status));
    }

    let uses_default_notification_targets = override_state
        .map(|state| state.notification_ids.is_empty())
        .unwrap_or(true);
    let notification_target_ids = if uses_default_notification_targets {
        session_state.default_notification_target_ids.clone()
    } else {
        override_state
            .map(|state| state.notification_ids.clone())
            .unwrap_or_default()
    };
    body_object.insert(
        "notificationStatus".to_owned(),
        json!({
            "enabled": !notification_target_ids.is_empty(),
            "targetIds": notification_target_ids,
            "usesDefault": uses_default_notification_targets,
        }),
    );

    Some(())
}

pub fn latest_session_mini_revision(records: &[MobileSessionMiniRecord]) -> Option<String> {
    records
        .iter()
        .max_by_key(|record| record.seq)
        .and_then(|record| {
            let revision = record.revision.trim();
            (!revision.is_empty()).then(|| revision.to_owned())
        })
}

fn session_mini_record_allows_prompt(record: &MobileSessionMiniRecord) -> bool {
    let Ok(body) = serde_json::from_str::<Value>(&record.body_json) else {
        return false;
    };
    body.get("canSendPrompt")
        .and_then(Value::as_bool)
        .or_else(|| body.get("replyable").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn session_mini_record_has_mode(record: &MobileSessionMiniRecord) -> bool {
    let Ok(body) = serde_json::from_str::<Value>(&record.body_json) else {
        return false;
    };
    body.get("effectiveMode")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|mode| !mode.is_empty())
}

fn mobile_session_mini_response(
    latest_seq: i64,
    revision: &str,
    server_time: &str,
    records: &[MobileSessionMiniRecord],
    replace: bool,
) -> Value {
    let sessions = records
        .iter()
        .filter_map(|record| serde_json::from_str::<Value>(&record.body_json).ok())
        .filter_map(compact_session_mini_payload)
        .collect::<Vec<_>>();
    json!({
        "latest_seq": latest_seq,
        "latestSeq": latest_seq,
        "revision": revision,
        "server_time": server_time,
        "serverTime": server_time,
        "freshness": {
            "source": "mobile-session-mini-projection",
            "latest_seq": latest_seq,
            "latestSeq": latest_seq,
            "revision": revision,
            "server_time": server_time,
            "serverTime": server_time,
        },
        "snapshot_kind": "recovery",
        "snapshotKind": "recovery",
        "replace": replace,
        "sessions": sessions,
    })
}

fn session_mini_values(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    queued_prompt_counts: &BTreeMap<String, i64>,
    seq: i64,
    revision: &str,
) -> Vec<Value> {
    snapshot
        .threads
        .iter()
        .filter(|thread| is_mobile_home_visible_thread(thread, session_state))
        .enumerate()
        .map(|(index, thread)| {
            session_mini_value(
                thread,
                index,
                session_state,
                queued_prompt_counts,
                seq,
                revision,
            )
        })
        .filter(session_mini_is_unarchived)
        .collect()
}

fn session_mini_value(
    thread: &DesktopThread,
    index: usize,
    session_state: &MobileSessionState,
    queued_prompt_counts: &BTreeMap<String, i64>,
    seq: i64,
    _revision: &str,
) -> Value {
    let mut summary = session_summary(thread, index, session_state);
    let Some(summary) = summary.as_object_mut() else {
        return json!({});
    };
    // Decay before anything below reads "status" off the summary, so the mini's `status` field
    // (copied from `summary` further down) and the `lifecycle` fallback both see the demoted
    // value consistently.
    let last_activity_at_ms = summary.get("lastActivityAtMs").and_then(Value::as_i64);
    let decayed_status = decay_stale_live_status(
        summary.get("status").cloned().unwrap_or(Value::Null),
        thread,
        last_activity_at_ms,
        current_time_millis(),
    );
    summary.insert("status".to_owned(), decayed_status.clone());
    let summary: &Map<String, Value> = summary;

    let override_state = session_override(thread, session_state);
    let status = decayed_status;
    let notification_target_ids = notification_target_ids(thread, session_state);
    let lifecycle = session_state
        .lifecycle
        .get(&thread.thread_id)
        .map(|lifecycle| json!(lifecycle.status))
        .unwrap_or_else(|| status.clone());

    let mut mini = Map::new();
    copy_summary_field(summary, &mut mini, "id");
    mini.insert("sessionId".to_owned(), json!(thread.thread_id));
    mini.insert("seq".to_owned(), json!(seq));
    copy_summary_field(summary, &mut mini, "assistantClient");
    mini.insert(
        "assistantSurface".to_owned(),
        summary
            .get("assistantClient")
            .cloned()
            .unwrap_or(Value::Null),
    );
    for field in [
        "ref",
        "status",
        "effectiveMode",
        "canSendPrompt",
        "lastUpdatedAt",
        "createdAtMs",
        "updatedAtMs",
        "latestMessageAtMs",
        "lastActivityAtMs",
        "lastActivityAt",
        "lastMessageAtMs",
        "lastMessageAt",
        "isArchived",
    ] {
        copy_summary_field(summary, &mut mini, field);
    }
    copy_summary_field(summary, &mut mini, "title");
    copy_summary_field(summary, &mut mini, "promptDeliveryUnavailableReason");
    copy_summary_field(summary, &mut mini, "assistantPreview");
    if let Some(metadata) = compact_metadata(summary.get("metadata")) {
        mini.insert("metadata".to_owned(), metadata);
    }
    mini.insert(
        "replyable".to_owned(),
        summary
            .get("canSendPrompt")
            .cloned()
            .unwrap_or_else(|| json!(false)),
    );
    let summary_goal = summary.get("goal");
    mini.insert(
        "goal".to_owned(),
        summary_goal
            .and_then(compact_goal_payload)
            .unwrap_or(Value::Null),
    );
    mini.insert(
        "blockedGoal".to_owned(),
        blocked_goal(summary_goal).unwrap_or(Value::Null),
    );
    mini.insert(
        "queueCount".to_owned(),
        json!(
            queued_prompt_counts
                .get(&thread.thread_id)
                .copied()
                .unwrap_or_default()
        ),
    );
    mini.insert("lifecycle".to_owned(), lifecycle);
    mini.insert(
        "notificationStatus".to_owned(),
        json!({
            "enabled": !notification_target_ids.is_empty(),
            "targetIds": notification_target_ids,
            "usesDefault": override_state
                .map(|state| state.notification_ids.is_empty())
                .unwrap_or(true),
        }),
    );
    Value::Object(mini)
}

/// Demotes a mini's "active"/"waiting" status to "stopped" once a session has been silent
/// past `STALE_LIVE_STATUS_DECAY_MS` and has no live-runtime evidence backing the status.
///
/// "Live-runtime evidence" is whatever liveness check a surface already performs when building
/// its `DesktopThread` (claude's process match, an ACP host connection, a provider API's
/// running/is-active check — see `claude_session_to_desktop_thread`, `acp_runtime_session_to_desktop_thread`,
/// `devin::sessions`, `grok_build::sessions`). Those surfaces stamp `thread.runtime_status` with
/// `ACTIVE_STATUS` only when that fresh check says the session is actually running, so trust it
/// outright and skip decay. Codex threads have no such signal at all
/// (`codex_thread_to_desktop_thread` always sets `runtime_status: None`) and rely purely on
/// hook-driven FSM lifecycle rows that never self-correct if the terminating hook is missed; for
/// those, activity age alone decides.
fn decay_stale_live_status(
    status: Value,
    thread: &DesktopThread,
    last_activity_at_ms: Option<i64>,
    now_ms: i64,
) -> Value {
    let Some(status_str) = status.as_str() else {
        return status;
    };
    if status_str != ACTIVE_STATUS && status_str != WAITING_LIVE_STATUS {
        return status;
    }
    if thread.runtime_status.as_deref() == Some(ACTIVE_STATUS) {
        return status;
    }
    let is_stale = last_activity_at_ms
        .is_some_and(|activity_ms| now_ms.saturating_sub(activity_ms) > STALE_LIVE_STATUS_DECAY_MS);
    if is_stale {
        json!(STOPPED_STATUS)
    } else {
        status
    }
}

fn current_time_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn compact_session_mini_payload(mut payload: Value) -> Option<Value> {
    let object = payload.as_object_mut()?;
    for field in EMBEDDED_CONTROL_FIELDS {
        object.remove(*field);
    }
    if let Some(metadata) = object.get_mut("metadata").and_then(Value::as_object_mut) {
        for field in DETAIL_METADATA_FIELDS {
            metadata.remove(*field);
        }
    }

    let mut compact = Map::new();
    for field in COMPACT_SESSION_MINI_FIELDS {
        let Some(value) = object.get(*field) else {
            continue;
        };
        let compacted = match *field {
            "metadata" => compact_metadata(Some(value)).unwrap_or(Value::Null),
            "goal" => compact_goal_payload(value).unwrap_or(Value::Null),
            "blockedGoal" => compact_blocked_goal_payload(value).unwrap_or(Value::Null),
            "notificationStatus" => compact_notification_status(value).unwrap_or(Value::Null),
            _ => value.clone(),
        };
        compact.insert((*field).to_owned(), compacted);
    }
    Some(Value::Object(compact))
}

fn copy_summary_field(summary: &Map<String, Value>, mini: &mut Map<String, Value>, field: &str) {
    if let Some(value) = summary.get(field) {
        mini.insert(field.to_owned(), value.clone());
    }
}

fn blocked_goal(goal: Option<&Value>) -> Option<Value> {
    let goal = goal?;
    let status = goal.get("status").and_then(Value::as_str)?;
    if !BLOCKED_GOAL_STATUSES.contains(&status) {
        return None;
    }
    let mut blocked_goal = goal.as_object()?.clone();
    blocked_goal.insert("reason".to_owned(), json!(status));
    if let Some(title) = blocked_goal.get("title").cloned() {
        blocked_goal.insert("title".to_owned(), title);
    }
    Some(Value::Object(blocked_goal))
}

fn compact_goal_payload(goal: &Value) -> Option<Value> {
    if goal.is_null() {
        return Some(Value::Null);
    }
    let goal = goal.as_object()?;
    let mut compact = Map::new();
    for field in [
        "id",
        "title",
        "status",
        "lifecycle",
        "running",
        "updatedAtMs",
        "tokenBudget",
        "tokensUsed",
        "timeUsedSeconds",
    ] {
        if let Some(value) = goal.get(field)
            && !compact_goal_value_is_empty(value)
        {
            compact.insert(field.to_owned(), value.clone());
        }
    }
    Some(Value::Object(compact))
}

fn compact_goal_value_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(value) => value.is_empty(),
        Value::Object(value) => value.is_empty(),
        _ => false,
    }
}

fn session_mini_is_unarchived(mini: &Value) -> bool {
    !mini
        .get("isArchived")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn compact_metadata(metadata: Option<&Value>) -> Option<Value> {
    let metadata = metadata?.as_object()?;
    let mut compact = Map::new();
    for field in [
        "kind",
        "source",
        "sourceDisplayName",
        "assistantKind",
        "originator",
        "projectName",
        "projectPath",
        "taskKind",
        "pullRequestURL",
    ] {
        if let Some(value) = metadata.get(field) {
            compact.insert(field.to_owned(), value.clone());
        }
    }
    for field in ["transcriptAvailable", "supportsSubagents"] {
        if let Some(value) = metadata.get(field) {
            compact.insert(field.to_owned(), value.clone());
        }
    }
    if let Some(git_repository) = compact_git_repository(metadata.get("gitRepository")) {
        compact.insert("gitRepository".to_owned(), git_repository);
    }
    compact.insert("installedPlugins".to_owned(), Value::Array(Vec::new()));
    Some(Value::Object(compact))
}

fn compact_blocked_goal_payload(blocked_goal: &Value) -> Option<Value> {
    if blocked_goal.is_null() {
        return Some(Value::Null);
    }
    let blocked_goal = blocked_goal.as_object()?;
    let mut compact = Map::new();
    for field in ["id", "title", "reason", "status"] {
        if let Some(value) = blocked_goal.get(field) {
            compact.insert(field.to_owned(), value.clone());
        }
    }
    Some(Value::Object(compact))
}

fn compact_notification_status(notification_status: &Value) -> Option<Value> {
    if notification_status.is_null() {
        return Some(Value::Null);
    }
    let notification_status = notification_status.as_object()?;
    let mut compact = Map::new();
    for field in ["enabled", "usesDefault"] {
        if let Some(value) = notification_status.get(field) {
            compact.insert(field.to_owned(), value.clone());
        }
    }
    let target_ids = notification_status
        .get("targetIds")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    compact.insert("targetIds".to_owned(), target_ids);
    Some(Value::Object(compact))
}

fn compact_git_repository(git_repository: Option<&Value>) -> Option<Value> {
    let git_repository = git_repository?.as_object()?;
    let mut compact = Map::new();
    for field in ["repositoryName", "repositoryPath", "remoteURL", "branch"] {
        if let Some(value) = git_repository.get(field) {
            compact.insert(field.to_owned(), value.clone());
        }
    }
    Some(Value::Object(compact))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::AssistantKind;
    use crate::goals::{GoalStatus, ThreadGoalSummary};
    use crate::mobile::api::test_support::{session_state_with_lifecycle, test_thread};

    const FRESH_ACTIVITY_OFFSET_MS: i64 = 60 * 1_000;
    const STALE_ACTIVITY_OFFSET_MS: i64 = STALE_LIVE_STATUS_DECAY_MS + 60 * 1_000;

    fn thread_with_activity(
        thread_id: &str,
        runtime_status: Option<&str>,
        last_activity_at_ms: i64,
    ) -> DesktopThread {
        DesktopThread {
            created_at_ms: Some(last_activity_at_ms),
            updated_at_ms: Some(last_activity_at_ms),
            latest_message_at_ms: Some(last_activity_at_ms),
            ..test_thread(thread_id, AssistantKind::Codex, runtime_status)
        }
    }

    #[test]
    fn session_mini_keeps_fresh_active_status() {
        let now_ms = current_time_millis();
        let thread = thread_with_activity(
            "thread-fresh-active",
            None,
            now_ms - FRESH_ACTIVITY_OFFSET_MS,
        );
        let session_state = session_state_with_lifecycle("thread-fresh-active", "active");

        let mini = session_mini_value(
            &thread,
            0,
            &session_state,
            &BTreeMap::new(),
            1,
            "revision-1",
        );

        assert_eq!(mini["status"], ACTIVE_STATUS);
    }

    #[test]
    fn session_mini_decays_stale_active_status_without_runtime_evidence() {
        let now_ms = current_time_millis();
        let thread = thread_with_activity(
            "thread-stale-active",
            None,
            now_ms - STALE_ACTIVITY_OFFSET_MS,
        );
        let session_state = session_state_with_lifecycle("thread-stale-active", "active");

        let mini = session_mini_value(
            &thread,
            0,
            &session_state,
            &BTreeMap::new(),
            1,
            "revision-1",
        );

        assert_eq!(mini["status"], STOPPED_STATUS);
    }

    #[test]
    fn session_mini_decays_stale_waiting_status_without_runtime_evidence() {
        let now_ms = current_time_millis();
        let thread = thread_with_activity(
            "thread-stale-waiting",
            None,
            now_ms - STALE_ACTIVITY_OFFSET_MS,
        );
        let mut session_state = session_state_with_lifecycle("thread-stale-waiting", "stopped");
        session_state.global_preset = Some("await-reply".to_owned());

        let mini = session_mini_value(
            &thread,
            0,
            &session_state,
            &BTreeMap::new(),
            1,
            "revision-1",
        );

        assert_eq!(mini["status"], STOPPED_STATUS);
    }

    #[test]
    fn session_mini_keeps_stale_active_status_with_live_runtime_evidence() {
        // Claude (and devin/grok/ACP-host) threads carry a `runtime_status` that is
        // re-verified on every read (process match, connection state, provider API check).
        // That live evidence should be trusted even when the thread's own timestamps are
        // stale, unlike codex threads which have no such signal.
        let now_ms = current_time_millis();
        let thread = thread_with_activity(
            "thread-stale-but-running",
            Some(ACTIVE_STATUS),
            now_ms - STALE_ACTIVITY_OFFSET_MS,
        );
        let session_state = MobileSessionState::default();

        let mini = session_mini_value(
            &thread,
            0,
            &session_state,
            &BTreeMap::new(),
            1,
            "revision-1",
        );

        assert_eq!(mini["status"], ACTIVE_STATUS);
    }

    #[test]
    fn compact_session_mini_payload_preserves_oversized_allowed_text() {
        let oversized_title = "t".repeat(SESSION_MINI_CONTROL_FRAME_MAX_BYTES + 1);
        let oversized_preview = "p".repeat(SESSION_MINI_CONTROL_FRAME_MAX_BYTES + 1);
        let payload = json!({
            "sessionId": "thread-main",
            "title": oversized_title,
            "assistantPreview": oversized_preview,
            "unknownHuge": "x".repeat(SESSION_MINI_CONTROL_FRAME_MAX_BYTES),
        });

        let compacted = compact_session_mini_payload(payload).expect("compacted payload");

        assert_eq!(
            compacted["title"].as_str().expect("title").len(),
            SESSION_MINI_CONTROL_FRAME_MAX_BYTES + 1
        );
        assert_eq!(
            compacted["assistantPreview"]
                .as_str()
                .expect("assistant preview")
                .len(),
            SESSION_MINI_CONTROL_FRAME_MAX_BYTES + 1
        );
        assert!(compacted.get("unknownHuge").is_none());
    }

    #[test]
    fn compact_metadata_preserves_allowed_text_and_removes_detail_fields() {
        let oversized_project_path = "/".to_owned() + &"project/".repeat(100);
        let oversized_repository_path = "/".to_owned() + &"looper/".repeat(100);
        let oversized_project_path_len = oversized_project_path.len();
        let oversized_repository_path_len = oversized_repository_path.len();
        let metadata = json!({
            "kind": "project",
            "source": "codex",
            "sourceDisplayName": "Codex",
            "projectPath": oversized_project_path,
            "gitRepository": {
                "repositoryName": "looper",
                "repositoryPath": oversized_repository_path,
                "branch": "main",
            },
            "tags": vec!["tag".repeat(40); METADATA_DETAIL_FIXTURE_TAG_COUNT],
            "sources": vec![json!({"kind": "source", "label": "Transcript", "value": "value"})],
            "spawn": {"rootThreadId": "thread-main"},
            "installedPlugins": vec![json!({"name": "plugin"})],
        });

        let compacted = compact_metadata(Some(&metadata)).expect("compacted metadata");

        assert!(
            compacted["projectPath"]
                .as_str()
                .expect("project path")
                .len()
                == oversized_project_path_len
        );
        assert_eq!(compacted["gitRepository"]["repositoryName"], "looper");
        assert!(
            compacted["gitRepository"]["repositoryPath"]
                .as_str()
                .expect("repository path")
                .len()
                == oversized_repository_path_len
        );
        assert_eq!(compacted["gitRepository"]["branch"], "main");
        assert!(compacted.get("tags").is_none());
        assert!(compacted.get("sources").is_none());
        assert!(compacted.get("spawn").is_none());
        assert!(
            compacted["installedPlugins"]
                .as_array()
                .expect("installed plugins")
                .is_empty()
        );
    }

    #[test]
    fn compact_session_mini_payload_removes_embedded_control_fields() {
        let payload = json!({
            "sessionId": "thread-main",
            "seq": 42,
            "revision": "full-desktop-revision",
            "globalSettings": {
                "assistantSurface": "codex"
            },
            "metadata": {
                "projectPath": "/Users/ay/Documents/looper",
                "spawn": {"rootThreadId": "thread-main"},
                "sources": [{"label": "Transcript", "value": "transcript"}],
                "tags": ["codex"]
            },
            "title": "Main task",
            "goal": {
                "id": "goal-paused",
                "title": "Pause visible work",
                "status": "paused",
                "lifecycle": "paused",
                "running": false,
                "updatedAtMs": 1_781_596_920_321i64,
                "tokenBudget": null,
                "unknownHuge": "x".repeat(SESSION_MINI_CONTROL_FRAME_MAX_BYTES),
            },
            "unknownHuge": "x".repeat(SESSION_MINI_CONTROL_FRAME_MAX_BYTES),
        });

        let compacted = compact_session_mini_payload(payload).expect("compacted payload");

        assert_eq!(compacted["sessionId"], "thread-main");
        assert_eq!(compacted["seq"], 42);
        assert_eq!(
            compacted["metadata"]["projectPath"],
            "/Users/ay/Documents/looper"
        );
        assert!(compacted.get("revision").is_none());
        assert!(compacted.get("globalSettings").is_none());
        assert!(compacted.get("unknownHuge").is_none());
        assert!(compacted["metadata"].get("spawn").is_none());
        assert!(compacted["metadata"].get("sources").is_none());
        assert!(compacted["metadata"].get("tags").is_none());
        assert_eq!(compacted["goal"]["status"], "paused");
        assert_eq!(compacted["goal"]["running"], false);
        assert!(compacted["goal"].get("tokenBudget").is_none());
        assert!(compacted["goal"].get("unknownHuge").is_none());
        assert!(compacted.to_string().len() < SESSION_MINI_CONTROL_FRAME_MAX_BYTES);
    }

    #[test]
    fn session_mini_goal_projection_includes_paused_and_running_goals() {
        let paused = mini_for_goal(Some(goal_summary(
            "goal-paused",
            "Pause visible work",
            GoalStatus::Paused,
            false,
        )));
        let pursuing = mini_for_goal(Some(goal_summary(
            "goal-pursuing",
            "Keep working",
            GoalStatus::Pursuing,
            true,
        )));
        let no_goal = mini_for_goal(None);
        let blocked = mini_for_goal(Some(goal_summary(
            "goal-blocked",
            "Unblock mobile card",
            GoalStatus::Blocked,
            false,
        )));

        assert_eq!(paused["goal"]["id"], "goal-paused");
        assert_eq!(paused["goal"]["status"], "paused");
        assert_eq!(paused["goal"]["running"], false);
        assert_eq!(paused["blockedGoal"], Value::Null);
        assert!(paused["goal"].get("tokenBudget").is_none());

        assert_eq!(pursuing["goal"]["id"], "goal-pursuing");
        assert_eq!(pursuing["goal"]["status"], "pursuing");
        assert_eq!(pursuing["goal"]["running"], true);
        assert_eq!(pursuing["blockedGoal"], Value::Null);

        assert_eq!(no_goal["goal"], Value::Null);
        assert_eq!(no_goal["blockedGoal"], Value::Null);

        assert_eq!(blocked["goal"]["status"], "blocked");
        assert_eq!(blocked["blockedGoal"]["id"], "goal-blocked");
        assert_eq!(blocked["blockedGoal"]["status"], "blocked");
        assert_eq!(blocked["blockedGoal"]["reason"], "blocked");
    }

    #[test]
    fn legacy_bloated_rows_replay_under_control_frame_cap() {
        let records = (0..LEGACY_REPLAY_FIXTURE_SESSION_COUNT)
            .map(|index| MobileSessionMiniRecord {
                session_id: format!("thread-{index}"),
                assistant_surface: "codex".to_owned(),
                seq: 42,
                revision: "r".repeat(LEGACY_REPLAY_REVISION_CHARS),
                body_json: json!({
                    "sessionId": format!("thread-{index}"),
                    "assistantSurface": "codex",
                    "seq": 42,
                    "revision": "r".repeat(LEGACY_REPLAY_REVISION_CHARS),
                    "globalSettings": {
                        "assistantSurface": "codex",
                        "defaultPrompt": "p".repeat(LEGACY_REPLAY_DETAIL_CHARS),
                    },
                    "metadata": {
                        "projectPath": "/Users/ay/Documents/looper",
                        "spawn": {"rootThreadId": "thread-main"},
                        "sources": [{"label": "Transcript", "value": "s".repeat(LEGACY_REPLAY_DETAIL_CHARS)}],
                        "tags": ["tag".repeat(LEGACY_REPLAY_TAG_CHARS)],
                    },
                    "title": "Main task",
                    "status": "waiting",
                    "canSendPrompt": true,
                })
                .to_string(),
                updated_at_ms: 1_000,
            })
            .collect::<Vec<_>>();

        let payload =
            mobile_session_mini_delta(42, "revision-42", "2026-06-24T00:00:00Z", &records, true)
                .to_string();
        let payload_json: Value = serde_json::from_str(&payload).expect("payload json");
        let sessions_json = payload_json["sessions"].to_string();

        assert!(payload.len() < SESSION_MINI_CONTROL_FRAME_MAX_BYTES);
        assert_eq!(payload_json["revision"], "revision-42");
        assert!(!sessions_json.contains("\"revision\""));
        assert!(!sessions_json.contains("globalSettings"));
        assert!(!sessions_json.contains("\"spawn\""));
        assert!(!sessions_json.contains("\"sources\""));
        assert!(!sessions_json.contains("\"tags\""));
    }

    #[test]
    fn session_mini_projection_keeps_unarchived_rows_without_recency_cutoff() {
        assert!(session_mini_is_unarchived(&json!({
            "sessionId": "old-stopped-codex",
            "assistantSurface": "codex",
            "status": "stopped",
            "isArchived": false,
            "lastActivityAtMs": 1,
        })));
        assert!(!session_mini_is_unarchived(&json!({
            "sessionId": "archived-codex",
            "assistantSurface": "codex",
            "status": "archived",
            "isArchived": true,
            "lastActivityAtMs": 1,
        })));
    }

    fn mini_for_goal(goal: Option<ThreadGoalSummary>) -> Value {
        let mut thread = test_thread("thread-main", AssistantKind::Codex, Some("stopped"));
        thread.goal = goal;
        session_mini_value(
            &thread,
            0,
            &MobileSessionState::default(),
            &BTreeMap::new(),
            42,
            "revision-42",
        )
    }

    fn goal_summary(id: &str, title: &str, status: GoalStatus, running: bool) -> ThreadGoalSummary {
        ThreadGoalSummary {
            id: id.to_owned(),
            title: title.to_owned(),
            status: status.clone(),
            lifecycle: status,
            running,
            token_budget: None,
            tokens_used: Some(123),
            time_used_seconds: Some(45),
            updated_at_ms: Some(1_781_596_920_321),
        }
    }
}

fn notification_target_ids(
    thread: &DesktopThread,
    session_state: &MobileSessionState,
) -> Vec<String> {
    let override_state = session_override(thread, session_state);
    let Some(override_ids) = override_state
        .map(|state| state.notification_ids.clone())
        .filter(|ids| !ids.is_empty())
    else {
        return session_state.default_notification_target_ids.clone();
    };

    let mut target_ids = session_state
        .default_notification_target_ids
        .iter()
        .filter(|target_id| is_builtin_notification_target(target_id))
        .cloned()
        .collect::<Vec<_>>();
    target_ids.extend(override_ids);
    dedupe_preserving_order(target_ids)
}

fn is_builtin_notification_target(target_id: &str) -> bool {
    matches!(
        target_id,
        NOTIFICATION_TARGET_IPHONE | NOTIFICATION_TARGET_MACOS
    )
}

fn dedupe_preserving_order(target_ids: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    target_ids
        .into_iter()
        .filter(|target_id| seen.insert(target_id.clone()))
        .collect()
}

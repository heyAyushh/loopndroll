use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::control_plane::{DesktopSnapshot, DesktopThread};
use crate::events::{MobileSessionMiniProjectionInput, MobileSessionMiniRecord};
use crate::mobile::session::{
    MobileSessionState, NOTIFICATION_TARGET_IPHONE, NOTIFICATION_TARGET_MACOS,
};

use super::overrides::{is_deleted, session_override};
use super::settings::mobile_global_settings;
use super::summary::session_summary;

const BLOCKED_GOAL_STATUSES: &[&str] = &["blocked", "usage-limited", "budget-limited", "unmet"];

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

pub fn mobile_session_mini_snapshot(latest_seq: i64, records: &[MobileSessionMiniRecord]) -> Value {
    mobile_session_mini_response(latest_seq, records, true)
}

pub fn mobile_session_mini_delta(
    latest_seq: i64,
    records: &[MobileSessionMiniRecord],
    replace: bool,
) -> Value {
    mobile_session_mini_response(latest_seq, records, replace)
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
    records: &[MobileSessionMiniRecord],
    replace: bool,
) -> Value {
    let sessions = records
        .iter()
        .filter_map(|record| serde_json::from_str::<Value>(&record.body_json).ok())
        .collect::<Vec<_>>();
    json!({
        "latest_seq": latest_seq,
        "latestSeq": latest_seq,
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
        .enumerate()
        .filter(|(_, thread)| !is_deleted(thread, session_state))
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
        .collect()
}

fn session_mini_value(
    thread: &DesktopThread,
    index: usize,
    session_state: &MobileSessionState,
    queued_prompt_counts: &BTreeMap<String, i64>,
    seq: i64,
    revision: &str,
) -> Value {
    let summary = session_summary(thread, index, session_state);
    let Some(summary) = summary.as_object() else {
        return json!({});
    };
    let override_state = session_override(thread, session_state);
    let status = summary.get("status").cloned().unwrap_or(Value::Null);
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
    mini.insert("revision".to_owned(), json!(revision));
    mini.insert(
        "assistantSurface".to_owned(),
        summary
            .get("assistantClient")
            .cloned()
            .unwrap_or(Value::Null),
    );
    for field in [
        "ref",
        "title",
        "status",
        "effectiveMode",
        "canSendPrompt",
        "promptDeliveryUnavailableReason",
        "lastUpdatedAt",
        "createdAtMs",
        "updatedAtMs",
        "latestMessageAtMs",
        "lastActivityAtMs",
        "lastActivityAt",
        "lastMessageAtMs",
        "lastMessageAt",
        "assistantPreview",
        "isArchived",
        "metadata",
    ] {
        copy_summary_field(summary, &mut mini, field);
    }
    mini.insert(
        "replyable".to_owned(),
        summary
            .get("canSendPrompt")
            .cloned()
            .unwrap_or_else(|| json!(false)),
    );
    mini.insert(
        "blockedGoal".to_owned(),
        blocked_goal(summary.get("goal")).unwrap_or(Value::Null),
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
    mini.insert(
        "globalSettings".to_owned(),
        mobile_global_settings(session_state),
    );
    Value::Object(mini)
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
    Some(Value::Object(blocked_goal))
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

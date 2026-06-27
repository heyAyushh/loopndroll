use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

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
const ACTIONABLE_SESSION_STATUSES: &[&str] = &["active", "waiting"];
const RECENT_SESSION_WINDOW_DAYS: i64 = 7;
const HOURS_PER_DAY: i64 = 24;
const MINUTES_PER_HOUR: i64 = 60;
const SECONDS_PER_MINUTE: i64 = 60;
const MILLIS_PER_SECOND: i64 = 1_000;
const RECENT_SESSION_WINDOW_MS: i64 = RECENT_SESSION_WINDOW_DAYS
    * HOURS_PER_DAY
    * MINUTES_PER_HOUR
    * SECONDS_PER_MINUTE
    * MILLIS_PER_SECOND;
const MINI_TITLE_MAX_CHARS: usize = 240;
const MINI_PREVIEW_MAX_CHARS: usize = 600;
const MINI_REASON_MAX_CHARS: usize = 240;
const MINI_METADATA_TEXT_MAX_CHARS: usize = 320;
const MINI_METADATA_TAG_MAX_CHARS: usize = 64;
const MINI_METADATA_MAX_TAGS: usize = 8;

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
    let minis = snapshot
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
        .collect::<Vec<_>>();
    let reference_times = surface_reference_times(&minis);
    minis
        .into_iter()
        .filter(|mini| {
            let reference_time_ms = mini
                .get("assistantSurface")
                .and_then(Value::as_str)
                .and_then(|surface| reference_times.get(surface).copied())
                .unwrap_or_else(current_time_millis);
            session_mini_is_hot_path_visible(mini, reference_time_ms)
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
    copy_bounded_summary_field(summary, &mut mini, "title", MINI_TITLE_MAX_CHARS);
    copy_bounded_summary_field(
        summary,
        &mut mini,
        "promptDeliveryUnavailableReason",
        MINI_REASON_MAX_CHARS,
    );
    copy_bounded_summary_field(
        summary,
        &mut mini,
        "assistantPreview",
        MINI_PREVIEW_MAX_CHARS,
    );
    if let Some(metadata) = bounded_metadata(summary.get("metadata")) {
        mini.insert("metadata".to_owned(), metadata);
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

fn copy_bounded_summary_field(
    summary: &Map<String, Value>,
    mini: &mut Map<String, Value>,
    field: &str,
    max_chars: usize,
) {
    if let Some(value) = summary.get(field) {
        mini.insert(field.to_owned(), bounded_value(value, max_chars));
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
        blocked_goal.insert(
            "title".to_owned(),
            bounded_value(&title, MINI_TITLE_MAX_CHARS),
        );
    }
    Some(Value::Object(blocked_goal))
}

fn session_mini_is_hot_path_visible(mini: &Value, now_ms: i64) -> bool {
    if mini
        .get("isArchived")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return false;
    }
    if mini
        .get("status")
        .and_then(Value::as_str)
        .map(normalized_status)
        .is_some_and(|status| ACTIONABLE_SESSION_STATUSES.contains(&status.as_str()))
    {
        return true;
    }
    if mini.get("blockedGoal").is_some_and(|goal| !goal.is_null()) {
        return true;
    }
    if mini
        .get("queueCount")
        .and_then(Value::as_i64)
        .is_some_and(|count| count > 0)
    {
        return true;
    }
    latest_mini_activity_ms(mini)
        .is_some_and(|activity_ms| now_ms.saturating_sub(activity_ms) <= RECENT_SESSION_WINDOW_MS)
}

fn latest_mini_activity_ms(mini: &Value) -> Option<i64> {
    [
        "lastActivityAtMs",
        "updatedAtMs",
        "latestMessageAtMs",
        "lastMessageAtMs",
        "createdAtMs",
    ]
    .into_iter()
    .filter_map(|field| mini.get(field).and_then(Value::as_i64))
    .max()
}

fn surface_reference_times(minis: &[Value]) -> BTreeMap<String, i64> {
    let mut reference_times: BTreeMap<String, i64> = BTreeMap::new();
    for mini in minis {
        let Some(surface) = mini.get("assistantSurface").and_then(Value::as_str) else {
            continue;
        };
        let Some(activity_ms) = latest_mini_activity_ms(mini) else {
            continue;
        };
        reference_times
            .entry(surface.to_owned())
            .and_modify(|reference_time| *reference_time = (*reference_time).max(activity_ms))
            .or_insert(activity_ms);
    }
    reference_times
}

fn bounded_metadata(metadata: Option<&Value>) -> Option<Value> {
    let metadata = metadata?.as_object()?;
    let mut bounded = Map::new();
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
            bounded.insert(
                field.to_owned(),
                bounded_value(value, MINI_METADATA_TEXT_MAX_CHARS),
            );
        }
    }
    for field in ["transcriptAvailable", "supportsSubagents"] {
        if let Some(value) = metadata.get(field) {
            bounded.insert(field.to_owned(), value.clone());
        }
    }
    if let Some(git_repository) = bounded_git_repository(metadata.get("gitRepository")) {
        bounded.insert("gitRepository".to_owned(), git_repository);
    }
    if let Some(spawn) = metadata.get("spawn") {
        bounded.insert("spawn".to_owned(), spawn.clone());
    }
    if let Some(tags) = bounded_string_array(metadata.get("tags"), MINI_METADATA_TAG_MAX_CHARS) {
        bounded.insert("tags".to_owned(), tags);
    }
    if let Some(sources) = bounded_sources(metadata.get("sources")) {
        bounded.insert("sources".to_owned(), sources);
    }
    bounded.insert("installedPlugins".to_owned(), Value::Array(Vec::new()));
    Some(Value::Object(bounded))
}

fn bounded_git_repository(git_repository: Option<&Value>) -> Option<Value> {
    let git_repository = git_repository?.as_object()?;
    let mut bounded = Map::new();
    for field in ["repositoryName", "remoteURL", "branch"] {
        if let Some(value) = git_repository.get(field) {
            bounded.insert(
                field.to_owned(),
                bounded_value(value, MINI_METADATA_TEXT_MAX_CHARS),
            );
        }
    }
    Some(Value::Object(bounded))
}

fn bounded_sources(sources: Option<&Value>) -> Option<Value> {
    let sources = sources?.as_array()?;
    let values = sources
        .iter()
        .take(MINI_METADATA_MAX_TAGS)
        .filter_map(|source| {
            let source = source.as_object()?;
            let mut bounded = Map::new();
            for field in ["kind", "label", "value", "url"] {
                if let Some(value) = source.get(field) {
                    bounded.insert(
                        field.to_owned(),
                        bounded_value(value, MINI_METADATA_TEXT_MAX_CHARS),
                    );
                }
            }
            Some(Value::Object(bounded))
        })
        .collect::<Vec<_>>();
    Some(Value::Array(values))
}

fn bounded_string_array(value: Option<&Value>, max_chars: usize) -> Option<Value> {
    let values = value?
        .as_array()?
        .iter()
        .take(MINI_METADATA_MAX_TAGS)
        .map(|value| bounded_value(value, max_chars))
        .collect::<Vec<_>>();
    Some(Value::Array(values))
}

fn bounded_value(value: &Value, max_chars: usize) -> Value {
    value
        .as_str()
        .map(|value| Value::String(truncate_chars(value, max_chars)))
        .unwrap_or_else(|| value.clone())
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn normalized_status(status: &str) -> String {
    status.trim().to_ascii_lowercase().replace('_', "-")
}

fn current_time_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(i64::MAX))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_card_fields_limit_large_text() {
        let mut mini = Map::new();
        let summary = Map::from_iter([
            (
                "title".to_owned(),
                Value::String("t".repeat(MINI_TITLE_MAX_CHARS + 10)),
            ),
            (
                "assistantPreview".to_owned(),
                Value::String("p".repeat(MINI_PREVIEW_MAX_CHARS + 10)),
            ),
        ]);

        copy_bounded_summary_field(&summary, &mut mini, "title", MINI_TITLE_MAX_CHARS);
        copy_bounded_summary_field(
            &summary,
            &mut mini,
            "assistantPreview",
            MINI_PREVIEW_MAX_CHARS,
        );

        assert_eq!(
            mini["title"].as_str().expect("title").chars().count(),
            MINI_TITLE_MAX_CHARS
        );
        assert_eq!(
            mini["assistantPreview"]
                .as_str()
                .expect("assistant preview")
                .chars()
                .count(),
            MINI_PREVIEW_MAX_CHARS
        );
    }

    #[test]
    fn bounded_metadata_limits_text_and_list_fields() {
        let metadata = json!({
            "kind": "project",
            "source": "codex",
            "sourceDisplayName": "Codex",
            "projectPath": "/".to_owned() + &"project/".repeat(100),
            "tags": vec!["tag".repeat(40); MINI_METADATA_MAX_TAGS + 2],
            "installedPlugins": vec![json!({"name": "plugin"})],
        });

        let bounded = bounded_metadata(Some(&metadata)).expect("bounded metadata");

        assert!(
            bounded["projectPath"]
                .as_str()
                .expect("project path")
                .chars()
                .count()
                <= MINI_METADATA_TEXT_MAX_CHARS
        );
        assert_eq!(
            bounded["tags"].as_array().expect("tags").len(),
            MINI_METADATA_MAX_TAGS
        );
        assert!(
            bounded["tags"][0].as_str().expect("tag").chars().count()
                <= MINI_METADATA_TAG_MAX_CHARS
        );
        assert!(
            bounded["installedPlugins"]
                .as_array()
                .expect("installed plugins")
                .is_empty()
        );
    }

    #[test]
    fn hot_path_visibility_is_recent_per_surface() {
        let old_ms = 1_700_000_000_000;
        let recent_ms = old_ms + RECENT_SESSION_WINDOW_MS + MILLIS_PER_SECOND;
        let minis = vec![
            json!({
                "sessionId": "old-codex",
                "assistantSurface": "codex",
                "status": "stopped",
                "isArchived": false,
                "lastActivityAtMs": old_ms,
            }),
            json!({
                "sessionId": "recent-codex",
                "assistantSurface": "codex",
                "status": "stopped",
                "isArchived": false,
                "lastActivityAtMs": recent_ms,
            }),
            json!({
                "sessionId": "active-old-codex",
                "assistantSurface": "codex",
                "status": "active",
                "isArchived": false,
                "lastActivityAtMs": old_ms,
            }),
        ];
        let reference_times = surface_reference_times(&minis);
        let reference_time_ms = reference_times["codex"];
        let session_ids = minis
            .iter()
            .filter(|mini| session_mini_is_hot_path_visible(mini, reference_time_ms))
            .filter_map(|mini| {
                mini.get("sessionId")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>();

        assert_eq!(session_ids, vec!["recent-codex", "active-old-codex"]);
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

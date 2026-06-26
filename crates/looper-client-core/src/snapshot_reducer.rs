use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ClientCoreError;
use looper_session_core::ACTIVE_STATUS;

const DEFAULT_ASSISTANT_SURFACE: &str = "codex";

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientSnapshotProjection {
    pub selected_assistant_surface: String,
    pub visible_snapshot_json: String,
    pub visible_session_ids: Vec<String>,
    pub session_sections: ClientSessionSectionsProjection,
    pub session_index: ClientSessionIndexProjection,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientOptimisticModeProjection {
    pub did_update: bool,
    pub visible_snapshot_json: String,
    pub visible_detail_json: String,
    pub has_detail: bool,
    pub visible_session_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientDetailCacheProjection {
    pub detail_by_session_id_json: String,
    pub visible_session_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientDetailModeProjection {
    pub did_update: bool,
    pub detail_json: String,
    pub has_detail: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientAssistantSurfaceSelection {
    pub did_change: bool,
    pub has_user_selected_assistant_surface: bool,
    pub selected_assistant_surface: String,
    pub has_pending_assistant_surface_save: bool,
    pub pending_assistant_surface: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, uniffi::Record)]
pub struct ClientSessionSectionsProjection {
    pub active_indexes: Vec<u32>,
    pub running_indexes: Vec<u32>,
    pub waiting_indexes: Vec<u32>,
    pub stopped_indexes: Vec<u32>,
    pub needs_attention_indexes: Vec<u32>,
    pub archived_indexes: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientSessionIndexEntry {
    pub surface: String,
    pub session_index: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientSessionIndexProjection {
    pub entries: Vec<ClientSessionIndexEntry>,
    pub identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientSiriSessionEntityProjection {
    pub entries: Vec<ClientSessionIndexEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotDocument {
    #[serde(default)]
    revision: Option<String>,
    #[serde(rename = "globalSettings")]
    global_settings: GlobalSettingsDocument,
    #[serde(default)]
    sessions: Vec<SessionDocument>,
    #[serde(rename = "surfaceSessions", default)]
    surface_sessions: BTreeMap<String, Vec<SessionDocument>>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GlobalSettingsDocument {
    #[serde(rename = "assistantSurface", default = "default_assistant_surface")]
    assistant_surface: String,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionDocument {
    id: String,
    #[serde(rename = "ref", default)]
    session_ref: String,
    #[serde(default = "default_session_status")]
    status: String,
    #[serde(rename = "effectiveMode")]
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_mode: Option<String>,
    #[serde(rename = "lastActivityAtMs", default)]
    last_activity_at_ms: Option<i64>,
    #[serde(rename = "lastActivityAt", default)]
    last_activity_at: String,
    #[serde(rename = "isArchived", default)]
    is_archived: bool,
    #[serde(default)]
    goal: Option<GoalDocument>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct GoalDocument {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    lifecycle: String,
    #[serde(default)]
    running: bool,
    #[serde(rename = "tokenBudget", default)]
    token_budget: Option<i64>,
    #[serde(rename = "tokensUsed", default)]
    tokens_used: Option<i64>,
    #[serde(rename = "timeUsedSeconds", default)]
    time_used_seconds: Option<i64>,
    #[serde(rename = "updatedAtMs", default)]
    updated_at_ms: Option<i64>,
}

struct SortableSession {
    original_index: u32,
    session: SessionDocument,
}

struct SortableSessionIndexEntry {
    surface: String,
    surface_index: u32,
    session: SessionDocument,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DetailDocument {
    #[serde(rename = "effectiveMode")]
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_mode: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

#[uniffi::export]
pub fn reduce_mobile_snapshot_projection(
    snapshot_json: String,
    preferred_assistant_surface: String,
    has_user_selected_assistant_surface: bool,
    current_selected_assistant_surface: String,
    assistant_surface_order: Vec<String>,
) -> Result<ClientSnapshotProjection, ClientCoreError> {
    let snapshot = parse_snapshot(&snapshot_json)?;
    let selected_assistant_surface = selected_assistant_surface(
        &snapshot,
        &preferred_assistant_surface,
        has_user_selected_assistant_surface,
        &current_selected_assistant_surface,
    );
    project_snapshot(
        snapshot,
        selected_assistant_surface,
        assistant_surface_order,
    )
}

#[uniffi::export]
pub fn reduce_mobile_snapshot_optimistic_mode(
    snapshot_json: String,
    detail_json: String,
    session_id: String,
    preset: String,
    selected_assistant_surface: String,
) -> Result<ClientOptimisticModeProjection, ClientCoreError> {
    let mut snapshot = parse_snapshot(&snapshot_json)?;
    let mut did_update = false;

    update_mode_in_sessions(
        &mut snapshot.sessions,
        &session_id,
        optional_preset(&preset),
        &mut did_update,
    );
    for sessions in snapshot.surface_sessions.values_mut() {
        update_mode_in_sessions(
            sessions,
            &session_id,
            optional_preset(&preset),
            &mut did_update,
        );
    }

    let (visible_detail_json, has_detail) = reduce_detail_mode(detail_json, &preset)?;
    let projection = project_snapshot(
        snapshot,
        selected_assistant_surface,
        default_assistant_surface_order(),
    )?;

    Ok(ClientOptimisticModeProjection {
        did_update: did_update || has_detail,
        visible_snapshot_json: projection.visible_snapshot_json,
        visible_detail_json,
        has_detail,
        visible_session_ids: projection.visible_session_ids,
    })
}

#[uniffi::export]
pub fn reduce_mobile_snapshot_detail_cache(
    visible_snapshot_json: String,
    detail_by_session_id_json: String,
) -> Result<ClientDetailCacheProjection, ClientCoreError> {
    let snapshot = parse_snapshot(&visible_snapshot_json)?;
    let mut detail_by_session_id: BTreeMap<String, Value> =
        serde_json::from_str(&detail_by_session_id_json)
            .map_err(|_| ClientCoreError::InvalidDetailJson)?;
    let visible_session_ids = snapshot
        .sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();

    detail_by_session_id.retain(|session_id, _| {
        snapshot
            .sessions
            .iter()
            .any(|session| session.id == *session_id)
    });

    for session in &snapshot.sessions {
        if let Some(detail) = detail_by_session_id.get_mut(&session.id) {
            sync_detail_from_session(detail, session);
        }
    }

    let detail_by_session_id_json = serde_json::to_string(&detail_by_session_id)
        .map_err(|_| ClientCoreError::InvalidDetailJson)?;
    Ok(ClientDetailCacheProjection {
        detail_by_session_id_json,
        visible_session_ids,
    })
}

#[uniffi::export]
pub fn reduce_session_detail_optimistic_mode(
    detail_json: String,
    preset: String,
) -> Result<ClientDetailModeProjection, ClientCoreError> {
    let (detail_json, has_detail) = reduce_detail_mode(detail_json, &preset)?;
    Ok(ClientDetailModeProjection {
        did_update: has_detail,
        detail_json,
        has_detail,
    })
}

#[uniffi::export]
pub fn reduce_assistant_surface_selection(
    current_selected_assistant_surface: String,
    requested_assistant_surface: String,
    has_user_selected_assistant_surface: bool,
) -> ClientAssistantSurfaceSelection {
    if current_selected_assistant_surface == requested_assistant_surface {
        return ClientAssistantSurfaceSelection {
            did_change: false,
            has_user_selected_assistant_surface,
            selected_assistant_surface: current_selected_assistant_surface,
            has_pending_assistant_surface_save: false,
            pending_assistant_surface: String::new(),
        };
    }

    ClientAssistantSurfaceSelection {
        did_change: true,
        has_user_selected_assistant_surface: true,
        selected_assistant_surface: requested_assistant_surface.clone(),
        has_pending_assistant_surface_save: true,
        pending_assistant_surface: requested_assistant_surface,
    }
}

#[uniffi::export]
pub fn reduce_session_sections(
    sessions_json: String,
) -> Result<ClientSessionSectionsProjection, ClientCoreError> {
    let sessions: Vec<SessionDocument> =
        serde_json::from_str(&sessions_json).map_err(|_| ClientCoreError::InvalidSnapshotJson)?;
    Ok(project_session_sections(sessions))
}

#[uniffi::export]
pub fn reduce_session_index(
    snapshot_json: String,
    assistant_surface_order: Vec<String>,
) -> Result<ClientSessionIndexProjection, ClientCoreError> {
    let snapshot = parse_snapshot(&snapshot_json)?;
    Ok(project_session_index(&snapshot, assistant_surface_order))
}

#[uniffi::export]
pub fn reduce_siri_session_entities(
    snapshot_json: String,
    assistant_surface_order: Vec<String>,
) -> Result<ClientSiriSessionEntityProjection, ClientCoreError> {
    let snapshot = parse_snapshot(&snapshot_json)?;
    let mut entries_by_entity_id = BTreeMap::<(String, String), SortableSessionIndexEntry>::new();

    for surface in assistant_surface_order {
        for (surface_index, session) in sessions_for_surface(&snapshot, &surface)
            .into_iter()
            .enumerate()
        {
            if session.is_archived {
                continue;
            }

            entries_by_entity_id.insert(
                (surface.clone(), session.id.clone()),
                SortableSessionIndexEntry {
                    surface: surface.clone(),
                    surface_index: surface_index as u32,
                    session,
                },
            );
        }
    }

    let mut sortable_entries = entries_by_entity_id.into_values().collect::<Vec<_>>();
    sort_session_index_entries(&mut sortable_entries);
    let entries = sortable_entries
        .into_iter()
        .map(|entry| ClientSessionIndexEntry {
            surface: entry.surface,
            session_index: entry.surface_index,
        })
        .collect();

    Ok(ClientSiriSessionEntityProjection { entries })
}

impl SessionDocument {
    fn has_blocked_goal(&self) -> bool {
        self.goal
            .as_ref()
            .is_some_and(|goal| normalized_status(&goal.status) == "blocked")
    }

    fn has_running_goal(&self) -> bool {
        self.goal.as_ref().is_some_and(|goal| goal.running)
    }
}

fn session_index_identity(
    snapshot: &SnapshotDocument,
    entries: &[SortableSessionIndexEntry],
) -> String {
    let revision = snapshot
        .revision
        .as_deref()
        .map(str::trim)
        .filter(|revision| !revision.is_empty())
        .unwrap_or("no-revision");
    let mut parts = vec![revision.to_owned(), entries.len().to_string()];
    parts.extend(entries.iter().map(|entry| {
        let session = &entry.session;
        let goal = session.goal.as_ref();
        [
            session.id.clone(),
            session.status.clone(),
            session.last_activity_at.clone(),
            session_extra_string(session, "lastMessageAt").to_owned(),
            goal.map(|goal| goal.id.clone()).unwrap_or_default(),
            goal.map(|goal| goal.status.clone()).unwrap_or_default(),
            goal.map(|goal| goal.lifecycle.clone()).unwrap_or_default(),
            if goal.is_some_and(|goal| goal.running) {
                "goal-running".to_owned()
            } else {
                "goal-idle".to_owned()
            },
            goal.and_then(|goal| goal.updated_at_ms)
                .unwrap_or_default()
                .to_string(),
            if session.is_archived {
                "archived".to_owned()
            } else {
                "visible".to_owned()
            },
        ]
        .join(":")
    }));
    parts.join("|")
}

fn session_extra_string<'a>(session: &'a SessionDocument, key: &str) -> &'a str {
    session.extra.get(key).and_then(Value::as_str).unwrap_or("")
}

fn sort_session_index_entries(entries: &mut [SortableSessionIndexEntry]) {
    entries.sort_by(|left, right| {
        if is_newer_or_lower_ref(&left.session, &right.session) {
            std::cmp::Ordering::Less
        } else if is_newer_or_lower_ref(&right.session, &left.session) {
            std::cmp::Ordering::Greater
        } else {
            left.surface
                .cmp(&right.surface)
                .then(left.surface_index.cmp(&right.surface_index))
        }
    });
}

fn sort_sessions_by_freshness(sessions: &mut [SortableSession]) {
    sessions.sort_by(|left, right| {
        if is_newer_or_lower_ref(&left.session, &right.session) {
            std::cmp::Ordering::Less
        } else if is_newer_or_lower_ref(&right.session, &left.session) {
            std::cmp::Ordering::Greater
        } else {
            left.original_index.cmp(&right.original_index)
        }
    });
}

fn is_newer_or_lower_ref(left: &SessionDocument, right: &SessionDocument) -> bool {
    if let (Some(left_activity_ms), Some(right_activity_ms)) =
        (left.last_activity_at_ms, right.last_activity_at_ms)
        && left_activity_ms != right_activity_ms
    {
        return left_activity_ms > right_activity_ms;
    }

    if let (Some(left_activity), Some(right_activity)) = (
        parse_iso8601_timestamp_nanos(&left.last_activity_at),
        parse_iso8601_timestamp_nanos(&right.last_activity_at),
    ) && left_activity != right_activity
    {
        return left_activity > right_activity;
    }

    if left.last_activity_at != right.last_activity_at {
        return left.last_activity_at > right.last_activity_at;
    }

    left.session_ref < right.session_ref
}

fn normalized_status(status: &str) -> String {
    status.trim().to_lowercase().replace('_', "-")
}

fn parse_iso8601_timestamp_nanos(value: &str) -> Option<i128> {
    let (date, time_and_zone) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let (time, offset_seconds) = split_iso8601_time_and_offset(time_and_zone)?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second_and_fraction = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }

    let (second, fraction_nanos) = parse_second_and_fraction(second_and_fraction)?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let days = days_from_civil(year, month, day)?;
    let local_seconds =
        days * 86_400 + i128::from(hour) * 3_600 + i128::from(minute) * 60 + i128::from(second);
    let utc_seconds = local_seconds - i128::from(offset_seconds);
    Some(utc_seconds * 1_000_000_000 + i128::from(fraction_nanos))
}

fn split_iso8601_time_and_offset(value: &str) -> Option<(&str, i32)> {
    if let Some(time) = value.strip_suffix('Z') {
        return Some((time, 0));
    }

    let offset_index = value[1..]
        .rfind(['+', '-'])
        .map(|relative_index| relative_index + 1)?;
    let (time, offset) = value.split_at(offset_index);
    Some((time, parse_timezone_offset_seconds(offset)?))
}

fn parse_timezone_offset_seconds(offset: &str) -> Option<i32> {
    let sign = match offset.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let offset = &offset[1..];
    let mut parts = offset.split(':');
    let hours = parts.next()?.parse::<i32>().ok()?;
    let minutes = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() || hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * (hours * 3_600 + minutes * 60))
}

fn parse_second_and_fraction(value: &str) -> Option<(u32, u32)> {
    let (second, fraction) = value.split_once('.').unwrap_or((value, ""));
    let second = second.parse::<u32>().ok()?;
    let mut fraction_nanos = 0_u32;
    let mut scale = 100_000_000_u32;
    for digit in fraction.bytes().take(9) {
        if !digit.is_ascii_digit() {
            return None;
        }
        fraction_nanos += u32::from(digit - b'0') * scale;
        scale /= 10;
    }
    Some((second, fraction_nanos))
}

fn days_from_civil(year: i32, month: u32, day: u32) -> Option<i128> {
    if month == 0 || month > 12 || day == 0 || day > days_in_month(year, month) {
        return None;
    }

    let year = year - i32::from(month <= 2);
    let era = div_floor(year, 400);
    let year_of_era = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(i128::from(era * 146_097 + day_of_era - 719_468))
}

fn div_floor(value: i32, divisor: i32) -> i32 {
    let quotient = value / divisor;
    let remainder = value % divisor;
    if remainder != 0 && ((remainder > 0) != (divisor > 0)) {
        quotient - 1
    } else {
        quotient
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn selected_assistant_surface(
    snapshot: &SnapshotDocument,
    preferred_assistant_surface: &str,
    has_user_selected_assistant_surface: bool,
    current_selected_assistant_surface: &str,
) -> String {
    if !preferred_assistant_surface.trim().is_empty() {
        return preferred_assistant_surface.to_owned();
    }
    if has_user_selected_assistant_surface {
        return current_selected_assistant_surface.to_owned();
    }
    snapshot.global_settings.assistant_surface.clone()
}

fn project_snapshot(
    mut snapshot: SnapshotDocument,
    selected_assistant_surface: String,
    assistant_surface_order: Vec<String>,
) -> Result<ClientSnapshotProjection, ClientCoreError> {
    let visible_sessions = sessions_for_surface(&snapshot, &selected_assistant_surface);
    let visible_session_ids = visible_sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();

    snapshot.global_settings.assistant_surface = selected_assistant_surface.clone();
    snapshot.sessions = visible_sessions;
    let session_sections = project_session_sections(snapshot.sessions.clone());
    let session_index = project_session_index(&snapshot, assistant_surface_order);

    Ok(ClientSnapshotProjection {
        selected_assistant_surface,
        visible_snapshot_json: serialize_snapshot(&snapshot)?,
        visible_session_ids,
        session_sections,
        session_index,
    })
}

fn project_session_sections(sessions: Vec<SessionDocument>) -> ClientSessionSectionsProjection {
    let mut sortable_sessions = sessions
        .into_iter()
        .enumerate()
        .map(|(index, session)| SortableSession {
            original_index: index as u32,
            session,
        })
        .collect::<Vec<_>>();
    sort_sessions_by_freshness(&mut sortable_sessions);

    let mut projection = ClientSessionSectionsProjection::default();
    for sortable_session in sortable_sessions {
        let index = sortable_session.original_index;
        let session = sortable_session.session;

        if session.is_archived {
            projection.archived_indexes.push(index);
            continue;
        }

        projection.active_indexes.push(index);

        if session.has_blocked_goal() {
            projection.needs_attention_indexes.push(index);
            continue;
        }

        match normalized_status(&session.status).as_str() {
            "active" => projection.running_indexes.push(index),
            "waiting" => {
                projection.waiting_indexes.push(index);
                projection.needs_attention_indexes.push(index);
            }
            "stopped" => {
                if session.has_running_goal() {
                    projection.running_indexes.push(index);
                } else {
                    projection.stopped_indexes.push(index);
                }
            }
            "archived" => projection.archived_indexes.push(index),
            _ => projection.stopped_indexes.push(index),
        }
    }

    projection
}

fn project_session_index(
    snapshot: &SnapshotDocument,
    assistant_surface_order: Vec<String>,
) -> ClientSessionIndexProjection {
    let surface_order = if assistant_surface_order.is_empty() {
        default_assistant_surface_order()
    } else {
        assistant_surface_order
    };
    let mut entries_by_id = BTreeMap::<String, SortableSessionIndexEntry>::new();

    for surface in surface_order {
        for (surface_index, session) in sessions_for_surface(snapshot, &surface)
            .into_iter()
            .enumerate()
        {
            match entries_by_id.get(&session.id) {
                Some(existing) if !is_newer_or_lower_ref(&session, &existing.session) => {}
                _ => {
                    entries_by_id.insert(
                        session.id.clone(),
                        SortableSessionIndexEntry {
                            surface: surface.clone(),
                            surface_index: surface_index as u32,
                            session,
                        },
                    );
                }
            }
        }
    }

    let mut sortable_entries = entries_by_id.into_values().collect::<Vec<_>>();
    sort_session_index_entries(&mut sortable_entries);

    let identity = session_index_identity(snapshot, &sortable_entries);
    let entries = sortable_entries
        .into_iter()
        .map(|entry| ClientSessionIndexEntry {
            surface: entry.surface,
            session_index: entry.surface_index,
        })
        .collect();

    ClientSessionIndexProjection { entries, identity }
}

fn default_assistant_surface_order() -> Vec<String> {
    vec![
        DEFAULT_ASSISTANT_SURFACE.to_owned(),
        "claudeCode".to_owned(),
        "devin".to_owned(),
        "grokBuild".to_owned(),
        "zed".to_owned(),
    ]
}

fn sessions_for_surface(
    snapshot: &SnapshotDocument,
    selected_assistant_surface: &str,
) -> Vec<SessionDocument> {
    if let Some(sessions) = snapshot.surface_sessions.get(selected_assistant_surface) {
        return sessions.clone();
    }

    if snapshot.global_settings.assistant_surface == selected_assistant_surface {
        snapshot.sessions.clone()
    } else {
        Vec::new()
    }
}

fn update_mode_in_sessions(
    sessions: &mut [SessionDocument],
    session_id: &str,
    preset: Option<String>,
    did_update: &mut bool,
) {
    for session in sessions {
        if session.id == session_id {
            session.effective_mode = preset.clone();
            *did_update = true;
        }
    }
}

fn reduce_detail_mode(
    detail_json: String,
    preset: &str,
) -> Result<(String, bool), ClientCoreError> {
    if detail_json.trim().is_empty() {
        return Ok((String::new(), false));
    }

    let mut detail: DetailDocument =
        serde_json::from_str(&detail_json).map_err(|_| ClientCoreError::InvalidDetailJson)?;
    detail.effective_mode = optional_preset(preset);
    let detail_json =
        serde_json::to_string(&detail).map_err(|_| ClientCoreError::InvalidDetailJson)?;
    Ok((detail_json, true))
}

fn sync_detail_from_session(detail: &mut Value, session: &SessionDocument) {
    let Some(detail_object) = detail.as_object_mut() else {
        return;
    };

    sync_detail_string_field(detail_object, "status", &session.status);
    sync_detail_mode(detail_object, session);
    sync_detail_field(detail_object, session, "lastUpdatedAt");
    sync_detail_string_field(detail_object, "lastActivityAt", &session.last_activity_at);
    sync_detail_field(detail_object, session, "lastMessageAt");
    sync_detail_field(detail_object, session, "assistantPreview");
    detail_object.insert("isArchived".to_owned(), Value::Bool(session.is_archived));
    sync_detail_field(detail_object, session, "metadata");
}

fn sync_detail_string_field(
    detail_object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: &str,
) {
    if value.is_empty() {
        detail_object.remove(key);
    } else {
        detail_object.insert(key.to_owned(), Value::String(value.to_owned()));
    }
}

fn sync_detail_field(
    detail_object: &mut serde_json::Map<String, Value>,
    session: &SessionDocument,
    key: &str,
) {
    if let Some(value) = session.extra.get(key) {
        detail_object.insert(key.to_owned(), value.clone());
    } else {
        detail_object.remove(key);
    }
}

fn sync_detail_mode(detail_object: &mut serde_json::Map<String, Value>, session: &SessionDocument) {
    if let Some(mode) = &session.effective_mode {
        detail_object.insert("effectiveMode".to_owned(), Value::String(mode.clone()));
    } else {
        detail_object.remove("effectiveMode");
    }
}

fn parse_snapshot(snapshot_json: &str) -> Result<SnapshotDocument, ClientCoreError> {
    serde_json::from_str(snapshot_json).map_err(|_| ClientCoreError::InvalidSnapshotJson)
}

fn serialize_snapshot(snapshot: &SnapshotDocument) -> Result<String, ClientCoreError> {
    serde_json::to_string(snapshot).map_err(|_| ClientCoreError::InvalidSnapshotJson)
}

fn optional_preset(preset: &str) -> Option<String> {
    let preset = preset.trim();
    if preset.is_empty() {
        None
    } else {
        Some(preset.to_owned())
    }
}

fn default_assistant_surface() -> String {
    DEFAULT_ASSISTANT_SURFACE.to_owned()
}

fn default_session_status() -> String {
    ACTIVE_STATUS.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX: &str = "codex";
    const DEVIN: &str = "devin";
    const THREAD_ID: &str = "thread-main";

    fn test_surface_order() -> Vec<String> {
        vec![CODEX.to_owned(), DEVIN.to_owned()]
    }

    #[test]
    fn snapshot_projection_uses_global_surface_until_user_selects() {
        let projection = reduce_mobile_snapshot_projection(
            snapshot_json(CODEX),
            String::new(),
            false,
            DEVIN.to_owned(),
            test_surface_order(),
        )
        .expect("project snapshot");
        let visible = parse_snapshot(&projection.visible_snapshot_json).expect("visible snapshot");

        assert_eq!(projection.selected_assistant_surface, CODEX);
        assert_eq!(projection.visible_session_ids, vec!["codex-thread"]);
        assert_eq!(projection.session_sections.active_indexes, vec![0]);
        assert!(
            projection
                .session_index
                .entries
                .iter()
                .any(|entry| entry.surface == CODEX)
        );
        assert!(!projection.session_index.identity.is_empty());
        assert_eq!(visible.global_settings.assistant_surface, CODEX);
        assert_eq!(visible.sessions[0].id, "codex-thread");
    }

    #[test]
    fn snapshot_projection_honors_selected_surface_and_preserves_unknown_fields() {
        let projection = reduce_mobile_snapshot_projection(
            snapshot_json(CODEX),
            DEVIN.to_owned(),
            true,
            CODEX.to_owned(),
            test_surface_order(),
        )
        .expect("project snapshot");
        let visible_value: Value =
            serde_json::from_str(&projection.visible_snapshot_json).expect("json");

        assert_eq!(projection.selected_assistant_surface, DEVIN);
        assert_eq!(projection.visible_session_ids, vec![THREAD_ID]);
        assert_eq!(projection.session_sections.active_indexes, vec![0]);
        assert_eq!(projection.session_index.entries[0].surface, DEVIN);
        assert_eq!(visible_value["host"]["name"], "Looper");
        assert_eq!(visible_value["sessions"][0]["ref"], "D1");
        assert_eq!(visible_value["globalSettings"]["defaultPrompt"], "Continue");
    }

    #[test]
    fn assistant_surface_selection_noops_without_pending_save() {
        let selection =
            reduce_assistant_surface_selection(CODEX.to_owned(), CODEX.to_owned(), false);

        assert!(!selection.did_change);
        assert!(!selection.has_user_selected_assistant_surface);
        assert_eq!(selection.selected_assistant_surface, CODEX);
        assert!(!selection.has_pending_assistant_surface_save);
        assert!(selection.pending_assistant_surface.is_empty());

        let user_selected_selection =
            reduce_assistant_surface_selection(CODEX.to_owned(), CODEX.to_owned(), true);

        assert!(!user_selected_selection.did_change);
        assert!(user_selected_selection.has_user_selected_assistant_surface);
        assert!(!user_selected_selection.has_pending_assistant_surface_save);
    }

    #[test]
    fn assistant_surface_selection_marks_pending_save_on_change() {
        let selection =
            reduce_assistant_surface_selection(CODEX.to_owned(), DEVIN.to_owned(), false);

        assert!(selection.did_change);
        assert!(selection.has_user_selected_assistant_surface);
        assert_eq!(selection.selected_assistant_surface, DEVIN);
        assert!(selection.has_pending_assistant_surface_save);
        assert_eq!(selection.pending_assistant_surface, DEVIN);
    }

    #[test]
    fn session_sections_classify_and_sort_in_rust() {
        let projection =
            reduce_session_sections(session_sections_json()).expect("project sections");

        assert_eq!(projection.active_indexes, vec![1, 2, 3, 4]);
        assert_eq!(projection.running_indexes, vec![4]);
        assert_eq!(projection.waiting_indexes, vec![1]);
        assert_eq!(projection.stopped_indexes, vec![3]);
        assert_eq!(projection.needs_attention_indexes, vec![1, 2]);
        assert_eq!(projection.archived_indexes, vec![0]);
    }

    #[test]
    fn session_sections_sort_fractional_seconds_before_string_fallback() {
        let projection = reduce_session_sections(
            r#"[
                {
                    "id":"whole",
                    "ref":"S2",
                    "status":"active",
                    "lastActivityAt":"2026-06-16T08:00:00Z",
                    "isArchived":false
                },
                {
                    "id":"fractional",
                    "ref":"S1",
                    "status":"active",
                    "lastActivityAt":"2026-06-16T08:00:00.500Z",
                    "isArchived":false
                }
            ]"#
            .to_owned(),
        )
        .expect("project sections");

        assert_eq!(projection.active_indexes, vec![1, 0]);
        assert_eq!(projection.running_indexes, vec![1, 0]);
    }

    #[test]
    fn session_index_dedupes_sorts_and_reports_best_surface() {
        let projection = reduce_session_index(
            session_index_json(),
            vec![CODEX.to_owned(), DEVIN.to_owned()],
        )
        .expect("project session index");

        assert_eq!(
            projection.entries,
            vec![
                ClientSessionIndexEntry {
                    surface: DEVIN.to_owned(),
                    session_index: 0,
                },
                ClientSessionIndexEntry {
                    surface: CODEX.to_owned(),
                    session_index: 1,
                },
            ]
        );
        assert_eq!(
            projection.identity,
            "revision-index|2|thread-main:waiting:2026-06-16T08:02:00.321Z:2026-06-16T08:01:00Z:goal-main:blocked:blocked:goal-idle:1781596920321:visible|codex-thread:active:2026-06-16T08:00:00Z:2026-06-16T07:59:00Z::::goal-idle:0:visible"
        );
    }

    #[test]
    fn session_index_uses_global_sessions_as_surface_fallback() {
        let projection = reduce_session_index(
            snapshot_json(CODEX),
            vec![CODEX.to_owned(), DEVIN.to_owned()],
        )
        .expect("project session index");

        assert_eq!(
            projection.entries,
            vec![
                ClientSessionIndexEntry {
                    surface: DEVIN.to_owned(),
                    session_index: 0,
                },
                ClientSessionIndexEntry {
                    surface: CODEX.to_owned(),
                    session_index: 0,
                },
            ]
        );
        assert_eq!(
            projection.identity.split('|').take(2).collect::<Vec<_>>(),
            vec!["rev-1", "2"]
        );
    }

    #[test]
    fn siri_session_entities_keep_surface_entities_and_sort_in_rust() {
        let projection = reduce_siri_session_entities(
            session_index_json(),
            vec![CODEX.to_owned(), DEVIN.to_owned()],
        )
        .expect("project siri entities");

        assert_eq!(
            projection.entries,
            vec![
                ClientSessionIndexEntry {
                    surface: DEVIN.to_owned(),
                    session_index: 0,
                },
                ClientSessionIndexEntry {
                    surface: CODEX.to_owned(),
                    session_index: 0,
                },
                ClientSessionIndexEntry {
                    surface: CODEX.to_owned(),
                    session_index: 1,
                },
            ]
        );
    }

    #[test]
    fn siri_session_entities_exclude_archived_sessions() {
        let projection = reduce_siri_session_entities(
            r#"{
                "revision":"siri-archived",
                "globalSettings":{"assistantSurface":"codex"},
                "sessions":[],
                "surfaceSessions":{
                    "codex":[
                        {
                            "id":"archived",
                            "ref":"S1",
                            "status":"archived",
                            "lastActivityAtMs":1781596920321,
                            "lastActivityAt":"2026-06-16T08:02:00.321Z",
                            "isArchived":true
                        },
                        {
                            "id":"visible",
                            "ref":"S2",
                            "status":"active",
                            "lastActivityAtMs":1781596920000,
                            "lastActivityAt":"2026-06-16T08:00:00Z",
                            "isArchived":false
                        }
                    ]
                }
            }"#
            .to_owned(),
            vec![CODEX.to_owned()],
        )
        .expect("project siri entities");

        assert_eq!(
            projection.entries,
            vec![ClientSessionIndexEntry {
                surface: CODEX.to_owned(),
                session_index: 1,
            }]
        );
    }

    #[test]
    fn optimistic_mode_updates_visible_snapshot_and_detail() {
        let projection = reduce_mobile_snapshot_optimistic_mode(
            snapshot_json(DEVIN),
            detail_json(Some("await-reply")),
            THREAD_ID.to_owned(),
            "send".to_owned(),
            DEVIN.to_owned(),
        )
        .expect("optimistic mode");
        let snapshot = parse_snapshot(&projection.visible_snapshot_json).expect("snapshot");
        let detail: DetailDocument =
            serde_json::from_str(&projection.visible_detail_json).expect("detail");

        assert!(projection.did_update);
        assert!(projection.has_detail);
        assert_eq!(snapshot.sessions[0].effective_mode.as_deref(), Some("send"));
        assert_eq!(detail.effective_mode.as_deref(), Some("send"));
    }

    #[test]
    fn optimistic_mode_can_restore_global_default_mode() {
        let projection = reduce_mobile_snapshot_optimistic_mode(
            snapshot_json(DEVIN),
            detail_json(Some("send")),
            THREAD_ID.to_owned(),
            String::new(),
            DEVIN.to_owned(),
        )
        .expect("optimistic mode");
        let snapshot = parse_snapshot(&projection.visible_snapshot_json).expect("snapshot");
        let detail: DetailDocument =
            serde_json::from_str(&projection.visible_detail_json).expect("detail");

        assert!(projection.did_update);
        assert_eq!(snapshot.sessions[0].effective_mode, None);
        assert_eq!(detail.effective_mode, None);
    }

    #[test]
    fn optimistic_mode_preserves_goal_payload_fields() {
        let projection = reduce_mobile_snapshot_optimistic_mode(
            r#"{
                "revision":"rev-goal",
                "globalSettings":{"assistantSurface":"codex"},
                "sessions":[{
                    "id":"goal-thread",
                    "ref":"G1",
                    "status":"active",
                    "lastUpdatedAt":"2026-06-16T08:00:00Z",
                    "lastActivityAt":"2026-06-16T08:00:00Z",
                    "isArchived":false,
                    "goal":{
                        "id":"goal-1",
                        "title":"Ship realtime",
                        "status":"blocked",
                        "lifecycle":"blocked",
                        "running":false,
                        "tokenBudget":1000,
                        "tokensUsed":250,
                        "timeUsedSeconds":60,
                        "updatedAtMs":1781596920321
                    }
                }],
                "surfaceSessions":{"codex":[{
                    "id":"goal-thread",
                    "ref":"G1",
                    "status":"active",
                    "lastUpdatedAt":"2026-06-16T08:00:00Z",
                    "lastActivityAt":"2026-06-16T08:00:00Z",
                    "isArchived":false,
                    "goal":{
                        "id":"goal-1",
                        "title":"Ship realtime",
                        "status":"blocked",
                        "lifecycle":"blocked",
                        "running":false,
                        "tokenBudget":1000,
                        "tokensUsed":250,
                        "timeUsedSeconds":60,
                        "updatedAtMs":1781596920321
                    }
                }]}
            }"#
            .to_owned(),
            String::new(),
            "goal-thread".to_owned(),
            "await-reply".to_owned(),
            CODEX.to_owned(),
        )
        .expect("optimistic mode");
        let snapshot: Value =
            serde_json::from_str(&projection.visible_snapshot_json).expect("snapshot json");
        let goal = &snapshot["sessions"][0]["goal"];

        assert_eq!(goal["title"], "Ship realtime");
        assert_eq!(goal["tokenBudget"], 1000);
        assert_eq!(goal["tokensUsed"], 250);
        assert_eq!(goal["timeUsedSeconds"], 60);
    }

    #[test]
    fn detail_cache_filters_and_syncs_visible_session_fields() {
        let projection = reduce_mobile_snapshot_projection(
            snapshot_json(DEVIN),
            DEVIN.to_owned(),
            true,
            CODEX.to_owned(),
            test_surface_order(),
        )
        .expect("project snapshot");
        let detail_cache = format!(
            r#"{{
                "{THREAD_ID}":{{
                    "id":"{THREAD_ID}",
                    "status":"stopped",
                    "effectiveMode":"send",
                    "lastUpdatedAt":"old",
                    "lastActivityAt":"old",
                    "lastMessageAt":"old",
                    "assistantPreview":"old",
                    "isArchived":true,
                    "metadata":{{"source":"old"}}
                }},
                "codex-thread":{{"id":"codex-thread","status":"active"}}
            }}"#
        );
        let cache_projection =
            reduce_mobile_snapshot_detail_cache(projection.visible_snapshot_json, detail_cache)
                .expect("detail cache");
        let details: BTreeMap<String, Value> =
            serde_json::from_str(&cache_projection.detail_by_session_id_json).expect("details");
        let detail = details.get(THREAD_ID).expect("visible detail");

        assert_eq!(cache_projection.visible_session_ids, vec![THREAD_ID]);
        assert_eq!(details.len(), 1);
        assert_eq!(detail["status"], "active");
        assert_eq!(detail["effectiveMode"], "await-reply");
        assert_eq!(detail["lastUpdatedAt"], "2026-06-16T08:02:00Z");
        assert_eq!(detail["lastActivityAt"], "2026-06-16T08:02:00Z");
        assert_eq!(detail["lastMessageAt"], "2026-06-16T08:01:00Z");
        assert_eq!(detail["assistantPreview"], "Ready");
        assert_eq!(detail["isArchived"], false);
        assert_eq!(detail["metadata"]["source"], "devin");
    }

    #[test]
    fn detail_only_optimistic_mode_uses_same_reducer_rules() {
        let projection =
            reduce_session_detail_optimistic_mode(detail_json(Some("await-reply")), String::new())
                .expect("detail mode");
        let detail: DetailDocument = serde_json::from_str(&projection.detail_json).expect("detail");

        assert!(projection.did_update);
        assert!(projection.has_detail);
        assert_eq!(detail.effective_mode, None);
    }

    fn snapshot_json(global_surface: &str) -> String {
        format!(
            r#"{{
                "revision":"rev-1",
                "host":{{"name":"Looper"}},
                "globalSettings":{{"assistantSurface":"{global_surface}","defaultPrompt":"Continue"}},
                "sessions":[{{
                    "id":"codex-thread",
                    "ref":"C1",
                    "status":"active",
                    "effectiveMode":"send",
                    "lastUpdatedAt":"2026-06-16T08:00:00Z",
                    "lastActivityAt":"2026-06-16T08:00:00Z",
                    "lastMessageAt":"2026-06-16T07:59:00Z",
                    "assistantPreview":"Codex ready",
                    "isArchived":false,
                    "metadata":{{"source":"codex"}}
                }}],
                "surfaceSessions":{{
                    "codex":[{{
                        "id":"codex-thread",
                        "ref":"C1",
                        "status":"active",
                        "effectiveMode":"send",
                        "lastUpdatedAt":"2026-06-16T08:00:00Z",
                        "lastActivityAt":"2026-06-16T08:00:00Z",
                        "lastMessageAt":"2026-06-16T07:59:00Z",
                        "assistantPreview":"Codex ready",
                        "isArchived":false,
                        "metadata":{{"source":"codex"}}
                    }}],
                    "devin":[{{
                        "id":"thread-main",
                        "ref":"D1",
                        "status":"active",
                        "effectiveMode":"await-reply",
                        "lastUpdatedAt":"2026-06-16T08:02:00Z",
                        "lastActivityAt":"2026-06-16T08:02:00Z",
                        "lastMessageAt":"2026-06-16T08:01:00Z",
                        "assistantPreview":"Ready",
                        "isArchived":false,
                        "metadata":{{"source":"devin"}}
                    }}]
                }},
                "notifications":[],
                "completionChecks":[]
            }}"#
        )
    }

    fn detail_json(effective_mode: Option<&str>) -> String {
        match effective_mode {
            Some(mode) => format!(r#"{{"id":"{THREAD_ID}","effectiveMode":"{mode}"}}"#),
            None => format!(r#"{{"id":"{THREAD_ID}","effectiveMode":null}}"#),
        }
    }

    fn session_sections_json() -> String {
        r#"[
            {
                "id":"archived",
                "ref":"A1",
                "status":"active",
                "lastActivityAtMs":1781596920000,
                "lastActivityAt":"2026-06-16T08:02:00Z",
                "isArchived":true
            },
            {
                "id":"waiting",
                "ref":"S2",
                "status":"waiting",
                "lastActivityAtMs":1781596920321,
                "lastActivityAt":"2026-06-16T08:02:00.321Z",
                "isArchived":false
            },
            {
                "id":"blocked",
                "ref":"S1",
                "status":"stopped",
                "lastActivityAtMs":1781596920320,
                "lastActivityAt":"2026-06-16T08:02:00.320Z",
                "isArchived":false,
                "goal":{"status":"blocked","running":false}
            },
            {
                "id":"stopped",
                "ref":"S3",
                "status":"stopped",
                "lastActivityAtMs":1781596920319,
                "lastActivityAt":"2026-06-16T08:02:00.319Z",
                "isArchived":false
            },
            {
                "id":"running-goal",
                "ref":"S4",
                "status":"stopped",
                "lastActivityAtMs":1781596920318,
                "lastActivityAt":"2026-06-16T08:02:00.318Z",
                "isArchived":false,
                "goal":{"status":"pursuing","running":true}
            }
        ]"#
        .to_owned()
    }

    fn session_index_json() -> String {
        r#"{
            "revision":"revision-index",
            "globalSettings":{"assistantSurface":"codex"},
            "sessions":[],
            "surfaceSessions":{
                "codex":[
                    {
                        "id":"thread-main",
                        "ref":"S1",
                        "status":"active",
                        "lastActivityAtMs":1781596920123,
                        "lastActivityAt":"2026-06-16T08:00:00.123Z",
                        "lastMessageAt":"2026-06-16T07:58:00Z",
                        "isArchived":false
                    },
                    {
                        "id":"codex-thread",
                        "ref":"C1",
                        "status":"active",
                        "lastActivityAtMs":1781596920000,
                        "lastActivityAt":"2026-06-16T08:00:00Z",
                        "lastMessageAt":"2026-06-16T07:59:00Z",
                        "isArchived":false
                    }
                ],
                "devin":[
                    {
                        "id":"thread-main",
                        "ref":"S2",
                        "status":"waiting",
                        "lastActivityAtMs":1781596920321,
                        "lastActivityAt":"2026-06-16T08:02:00.321Z",
                        "lastMessageAt":"2026-06-16T08:01:00Z",
                        "isArchived":false,
                        "goal":{
                            "id":"goal-main",
                            "status":"blocked",
                            "lifecycle":"blocked",
                            "running":false,
                            "updatedAtMs":1781596920321
                        }
                    }
                ]
            }
        }"#
        .to_owned()
    }
}

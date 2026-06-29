use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::error::ClientCoreError;
use crate::model::{ClientPendingCommand, ClientPendingCommandKind, ClientStateMini};

const DEFAULT_ASSISTANT_SURFACE: &str = "codex";
const DEFAULT_PROMPT: &str = "Continue";
const DEFAULT_SESSION_STATUS: &str = "stopped";
const GLOBAL_SCOPE: &str = "global";
const HOST_ID: &str = "local-session-mini-cache";
const HOST_NAME: &str = "Looper";
const REVISION_PREFIX: &str = "mini:";
const REVISION_SURFACE_FIELD_PREFIX: &str = "surface=";
const STATUS_ARCHIVED: &str = "archived";

const KNOWN_ASSISTANT_SURFACES: [&str; 5] = ["codex", "claude-code", "devin", "grok-build", "zed"];
const CODEX_SURFACE_ASSISTANT_CLIENTS: [&str; 4] = [
    DEFAULT_ASSISTANT_SURFACE,
    "cursor",
    "super-engineering",
    "openclaw",
];

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSnapshotProjection {
    pub has_snapshot: bool,
    pub snapshot_json: String,
}

#[uniffi::export]
pub fn reduce_state_minis_mobile_snapshot(
    latest_seq: i64,
    sessions: Vec<ClientStateMini>,
    server_time: String,
) -> Result<ClientMobileSnapshotProjection, ClientCoreError> {
    let sessions = decodable_sessions(&sessions);
    if sessions.is_empty() {
        return Ok(ClientMobileSnapshotProjection {
            has_snapshot: false,
            snapshot_json: String::new(),
        });
    }

    let mut sessions_by_surface: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut latest_global_settings: Option<(i64, Value)> = None;
    for (mini, session) in &sessions {
        if let Some(settings) = session.get("globalSettings").cloned() {
            if latest_global_settings
                .as_ref()
                .map(|(seq, _)| mini.seq >= *seq)
                .unwrap_or(true)
            {
                latest_global_settings = Some((mini.seq, settings));
            }
        }
        sessions_by_surface
            .entry(surface_bucket_for_assistant_client(&mini.assistant_surface).to_owned())
            .or_default()
            .push(session.clone());
    }
    for sessions in sessions_by_surface.values_mut() {
        sort_sessions_by_freshness(sessions);
    }

    let selected_surface = selected_surface(&sessions);
    let visible_sessions = sessions_by_surface
        .get(&selected_surface)
        .cloned()
        .unwrap_or_default();
    let global_settings = global_settings(
        latest_global_settings.map(|(_, settings)| settings),
        &selected_surface,
    );

    let snapshot = json!({
        "revision": revision(latest_seq, &sessions),
        "host": {
            "id": HOST_ID,
            "name": HOST_NAME,
            "address": "",
            "isReachable": false,
            "lastSyncedAt": server_time,
        },
        "globalSettings": global_settings,
        "sessions": visible_sessions,
        "surfaceSessions": sessions_by_surface,
        "notifications": [],
        "completionChecks": [],
    });

    let snapshot_json =
        serde_json::to_string(&snapshot).map_err(|_| ClientCoreError::InvalidSnapshotJson)?;
    Ok(ClientMobileSnapshotProjection {
        has_snapshot: true,
        snapshot_json,
    })
}

#[uniffi::export]
pub fn reduce_state_minis_mobile_snapshot_with_pending_commands(
    latest_seq: i64,
    sessions: Vec<ClientStateMini>,
    pending_commands: Vec<ClientPendingCommand>,
    server_time: String,
) -> Result<ClientMobileSnapshotProjection, ClientCoreError> {
    let mut projection = reduce_state_minis_mobile_snapshot(latest_seq, sessions, server_time)?;
    if !projection.has_snapshot || pending_commands.is_empty() {
        return Ok(projection);
    }

    let mut snapshot = serde_json::from_str::<Value>(&projection.snapshot_json)
        .map_err(|_| ClientCoreError::InvalidSnapshotJson)?;
    apply_pending_commands(&mut snapshot, &pending_commands);
    projection.snapshot_json =
        serde_json::to_string(&snapshot).map_err(|_| ClientCoreError::InvalidSnapshotJson)?;
    Ok(projection)
}

fn apply_pending_commands(snapshot: &mut Value, commands: &[ClientPendingCommand]) {
    for command in commands {
        match command.kind {
            ClientPendingCommandKind::SetSessionMode => {
                apply_pending_mode(snapshot, &command.thread_id, &command.preset);
            }
            ClientPendingCommandKind::SetSiriCurrentSession => apply_pending_siri_session(
                snapshot,
                "siriCurrentSessionId",
                "siriCurrentAssistantSurface",
                &command.thread_id,
                &command.assistant_surface,
            ),
            ClientPendingCommandKind::SetSiriDefaultSession => apply_pending_siri_session(
                snapshot,
                "siriDefaultSessionId",
                "siriDefaultAssistantSurface",
                &command.thread_id,
                &command.assistant_surface,
            ),
            ClientPendingCommandKind::SaveDefaultPrompt => {
                apply_pending_default_prompt(snapshot, &command.prompt);
            }
            ClientPendingCommandKind::SetSessionArchived => {
                apply_pending_archive(snapshot, &command.thread_id, command.archived);
            }
            ClientPendingCommandKind::DeleteSession => {
                apply_pending_delete(snapshot, &command.thread_id);
            }
            ClientPendingCommandKind::SendSessionPrompt
            | ClientPendingCommandKind::SubmitNotificationReply
            | ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SetDefaultNotificationTargets
            | ClientPendingCommandKind::MuteSession => {}
        }
    }
    refresh_visible_sessions_from_selected_surface(snapshot);
}

fn apply_pending_mode(snapshot: &mut Value, session_id: &str, preset: &str) {
    let session_id = normalized_pending_text(session_id);
    if session_id.is_empty() {
        return;
    }
    let mode = pending_session_mode_value(preset);
    update_pending_sessions(snapshot, &session_id, &mut |session| {
        session.insert("effectiveMode".to_owned(), mode.clone());
    });
}

fn apply_pending_siri_session(
    snapshot: &mut Value,
    session_field: &str,
    surface_field: &str,
    session_id: &str,
    assistant_surface: &str,
) {
    let normalized_session_id = normalized_pending_text(session_id);
    let Some(settings) = global_settings_mut(snapshot) else {
        return;
    };
    if normalized_session_id.is_empty() {
        settings.insert(session_field.to_owned(), Value::Null);
        settings.insert(surface_field.to_owned(), Value::Null);
        if session_field == "siriCurrentSessionId" {
            settings.insert("siriCurrentUpdatedAtMs".to_owned(), Value::Null);
        }
        return;
    }
    settings.insert(
        session_field.to_owned(),
        Value::String(normalized_session_id),
    );
    settings.insert(
        surface_field.to_owned(),
        pending_assistant_surface_value(assistant_surface),
    );
}

fn apply_pending_default_prompt(snapshot: &mut Value, prompt: &str) {
    let prompt = normalized_pending_text(prompt);
    if prompt.is_empty() {
        return;
    }
    let Some(settings) = global_settings_mut(snapshot) else {
        return;
    };
    settings.insert("defaultPrompt".to_owned(), Value::String(prompt));
}

fn apply_pending_archive(snapshot: &mut Value, session_id: &str, archived: bool) {
    let session_id = normalized_pending_text(session_id);
    if session_id.is_empty() {
        return;
    }
    update_pending_sessions(snapshot, &session_id, &mut |session| {
        session.insert("isArchived".to_owned(), Value::Bool(archived));
        if archived {
            session.insert(
                "status".to_owned(),
                Value::String(STATUS_ARCHIVED.to_owned()),
            );
        }
    });
}

fn apply_pending_delete(snapshot: &mut Value, session_id: &str) {
    let session_id = normalized_pending_text(session_id);
    if session_id.is_empty() {
        return;
    }
    if let Some(surface_sessions) = snapshot
        .get_mut("surfaceSessions")
        .and_then(Value::as_object_mut)
    {
        for sessions in surface_sessions.values_mut() {
            remove_session_from_array(sessions, &session_id);
        }
    } else if let Some(sessions) = snapshot.get_mut("sessions") {
        remove_session_from_array(sessions, &session_id);
    }
    clear_pending_siri_targets(snapshot, &session_id);
}

fn update_pending_sessions(
    snapshot: &mut Value,
    session_id: &str,
    mutate: &mut impl FnMut(&mut serde_json::Map<String, Value>),
) {
    if let Some(surface_sessions) = snapshot
        .get_mut("surfaceSessions")
        .and_then(Value::as_object_mut)
    {
        for sessions in surface_sessions.values_mut() {
            update_session_array(sessions, session_id, mutate);
        }
        return;
    }

    if let Some(sessions) = snapshot.get_mut("sessions") {
        update_session_array(sessions, session_id, mutate);
    }
}

fn update_session_array(
    sessions: &mut Value,
    session_id: &str,
    mutate: &mut impl FnMut(&mut serde_json::Map<String, Value>),
) {
    let Some(sessions) = sessions.as_array_mut() else {
        return;
    };
    for session in sessions {
        let Some(session) = session.as_object_mut() else {
            continue;
        };
        if session
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == session_id)
        {
            mutate(session);
        }
    }
}

fn remove_session_from_array(sessions: &mut Value, session_id: &str) {
    let Some(sessions) = sessions.as_array_mut() else {
        return;
    };
    sessions.retain(|session| {
        session
            .get("id")
            .and_then(Value::as_str)
            .map(|id| id != session_id)
            .unwrap_or(true)
    });
}

fn clear_pending_siri_targets(snapshot: &mut Value, session_id: &str) {
    let Some(settings) = global_settings_mut(snapshot) else {
        return;
    };
    if settings
        .get("siriCurrentSessionId")
        .and_then(Value::as_str)
        .is_some_and(|current| current == session_id)
    {
        settings.insert("siriCurrentSessionId".to_owned(), Value::Null);
        settings.insert("siriCurrentAssistantSurface".to_owned(), Value::Null);
        settings.insert("siriCurrentUpdatedAtMs".to_owned(), Value::Null);
    }
    if settings
        .get("siriDefaultSessionId")
        .and_then(Value::as_str)
        .is_some_and(|default| default == session_id)
    {
        settings.insert("siriDefaultSessionId".to_owned(), Value::Null);
        settings.insert("siriDefaultAssistantSurface".to_owned(), Value::Null);
    }
}

fn refresh_visible_sessions_from_selected_surface(snapshot: &mut Value) {
    let selected_surface = snapshot
        .get("globalSettings")
        .and_then(|settings| settings.get("assistantSurface"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_ASSISTANT_SURFACE)
        .to_owned();
    let Some(surface_sessions) = snapshot
        .get("surfaceSessions")
        .and_then(Value::as_object)
        .filter(|surface_sessions| !surface_sessions.is_empty())
    else {
        return;
    };
    let sessions = surface_sessions
        .get(&selected_surface)
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    if let Some(snapshot) = snapshot.as_object_mut() {
        snapshot.insert("sessions".to_owned(), sessions);
    }
}

fn global_settings_mut(snapshot: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    snapshot
        .get_mut("globalSettings")
        .and_then(Value::as_object_mut)
}

fn pending_session_mode_value(preset: &str) -> Value {
    let preset = normalized_pending_text(preset);
    if is_known_session_mode(&preset) {
        Value::String(preset)
    } else {
        Value::Null
    }
}

fn pending_assistant_surface_value(assistant_surface: &str) -> Value {
    let assistant_surface = normalized_pending_text(assistant_surface);
    if is_known_assistant_surface(&assistant_surface) {
        Value::String(assistant_surface)
    } else {
        Value::Null
    }
}

fn is_known_session_mode(mode: &str) -> bool {
    matches!(
        mode,
        "infinite"
            | "await-reply"
            | "completion-checks"
            | "max-turns-1"
            | "max-turns-2"
            | "max-turns-3"
    )
}

fn normalized_pending_text(value: &str) -> String {
    value.trim().to_owned()
}

fn decodable_sessions(sessions: &[ClientStateMini]) -> Vec<(&ClientStateMini, Value)> {
    sessions
        .iter()
        .filter_map(|mini| {
            decode_session_payload(mini)
                .ok()
                .map(|session| (mini, session))
        })
        .collect()
}

fn global_settings(settings: Option<Value>, selected_surface: &str) -> Value {
    let mut merged = json!({
        "defaultPrompt": DEFAULT_PROMPT,
        "scope": GLOBAL_SCOPE,
        "completionCheckWaitForReply": false,
        "assistantSurface": selected_surface,
    });
    let Some(Value::Object(settings)) = settings else {
        return merged;
    };
    let Some(merged_object) = merged.as_object_mut() else {
        return merged;
    };
    for (key, value) in settings {
        merged_object.insert(key, value);
    }
    if !merged_object
        .get("assistantSurface")
        .and_then(Value::as_str)
        .is_some_and(|surface| !surface.trim().is_empty())
    {
        merged_object.insert(
            "assistantSurface".to_owned(),
            Value::String(selected_surface.to_owned()),
        );
    }
    merged
}

fn decode_session_payload(mini: &ClientStateMini) -> Result<Value, ClientCoreError> {
    let mut session: Value = serde_json::from_str(&mini.payload_json)
        .map_err(|_| ClientCoreError::InvalidStateMiniPayloadJson)?;
    let session = session
        .as_object_mut()
        .ok_or(ClientCoreError::InvalidStateMiniPayloadJson)?;

    reject_mismatched_session_id(session.get("id"), &mini.session_id)?;
    reject_mismatched_session_id(session.get("sessionId"), &mini.session_id)?;
    session.insert("id".to_owned(), Value::String(mini.session_id.clone()));
    session.insert(
        "sessionId".to_owned(),
        Value::String(mini.session_id.clone()),
    );
    let assistant_client = fallback_assistant_client(&mini.assistant_surface);
    insert_string_default(session, "assistantSurface", assistant_client);
    insert_string_default(session, "assistantClient", assistant_client);
    insert_string_default(session, "ref", &mini.session_id);
    insert_string_default(session, "title", &mini.session_id);
    insert_string_default(session, "status", DEFAULT_SESSION_STATUS);
    insert_string_default(session, "lastUpdatedAt", "");
    insert_string_default(session, "lastActivityAt", "");
    insert_bool_default(session, "isArchived", false);
    insert_bool_default(session, "canSendPrompt", true);
    session
        .entry("metadata".to_owned())
        .or_insert_with(|| json!({}));
    repair_git_repository_metadata(session);

    Ok(Value::Object(session.clone()))
}

fn reject_mismatched_session_id(
    value: Option<&Value>,
    session_id: &str,
) -> Result<(), ClientCoreError> {
    let Some(value) = value.and_then(Value::as_str) else {
        return Ok(());
    };
    if value == session_id {
        Ok(())
    } else {
        Err(ClientCoreError::StateMiniSessionIdMismatch)
    }
}

fn insert_string_default(session: &mut serde_json::Map<String, Value>, key: &str, fallback: &str) {
    let needs_default = session
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    if needs_default {
        session.insert(key.to_owned(), Value::String(fallback.to_owned()));
    }
}

fn insert_bool_default(session: &mut serde_json::Map<String, Value>, key: &str, fallback: bool) {
    if !session.get(key).is_some_and(Value::is_boolean) {
        session.insert(key.to_owned(), Value::Bool(fallback));
    }
}

fn repair_git_repository_metadata(session: &mut serde_json::Map<String, Value>) {
    let fallback_name = session
        .get("metadata")
        .and_then(|metadata| metadata.get("projectName"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| {
            session
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        });
    let Some(repository) = session
        .get_mut("metadata")
        .and_then(|metadata| metadata.as_object_mut())
        .and_then(|metadata| metadata.get_mut("gitRepository"))
        .and_then(|repository| repository.as_object_mut())
    else {
        return;
    };
    insert_string_default(repository, "repositoryName", &fallback_name);
    insert_string_default(repository, "repositoryPath", "");
}

fn fallback_assistant_client(client: &str) -> &str {
    if client.trim().is_empty() {
        DEFAULT_ASSISTANT_SURFACE
    } else {
        client
    }
}

fn selected_surface(sessions: &[(&ClientStateMini, Value)]) -> String {
    if let Some(surface) = selected_surface_from_revision(sessions) {
        return surface;
    }

    selected_surface_from_sessions(sessions)
}

fn selected_surface_from_revision(sessions: &[(&ClientStateMini, Value)]) -> Option<String> {
    sessions
        .iter()
        .filter_map(|(session, _)| {
            revision_assistant_surface(&session.revision)
                .map(|surface| (session.seq, session.session_id.as_str(), surface))
        })
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, _, surface)| surface)
}

fn revision_assistant_surface(revision: &str) -> Option<String> {
    revision.split(':').find_map(|part| {
        let surface = part
            .trim()
            .strip_prefix(REVISION_SURFACE_FIELD_PREFIX)?
            .trim();
        is_known_assistant_surface(surface).then(|| surface.to_owned())
    })
}

fn selected_surface_from_sessions(sessions: &[(&ClientStateMini, Value)]) -> String {
    let mut candidates = sessions
        .iter()
        .filter(|(session, _)| {
            is_known_assistant_surface(surface_bucket_for_assistant_client(
                &session.assistant_surface,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .0
            .seq
            .cmp(&left.0.seq)
            .then_with(|| left.0.session_id.cmp(&right.0.session_id))
    });
    candidates
        .first()
        .map(|(session, _)| {
            surface_bucket_for_assistant_client(&session.assistant_surface).to_owned()
        })
        .unwrap_or_else(|| DEFAULT_ASSISTANT_SURFACE.to_owned())
}

fn is_known_assistant_surface(surface: &str) -> bool {
    KNOWN_ASSISTANT_SURFACES.contains(&surface)
}

fn surface_bucket_for_assistant_client(client: &str) -> &str {
    let assistant_client = fallback_assistant_client(client);
    if CODEX_SURFACE_ASSISTANT_CLIENTS.contains(&assistant_client) {
        DEFAULT_ASSISTANT_SURFACE
    } else {
        assistant_client
    }
}

fn revision(latest_seq: i64, sessions: &[(&ClientStateMini, Value)]) -> String {
    sessions
        .iter()
        .filter(|(session, _)| !session.revision.trim().is_empty())
        .max_by(|left, right| {
            left.0
                .seq
                .cmp(&right.0.seq)
                .then_with(|| left.0.assistant_surface.cmp(&right.0.assistant_surface))
                .then_with(|| left.0.session_id.cmp(&right.0.session_id))
        })
        .map(|(session, _)| session.revision.trim().to_owned())
        .unwrap_or_else(|| format!("{REVISION_PREFIX}{latest_seq}"))
}

fn sort_sessions_by_freshness(sessions: &mut [Value]) {
    sessions.sort_by(compare_session_freshness);
}

fn compare_session_freshness(left: &Value, right: &Value) -> Ordering {
    match (
        field_i64(left, "lastActivityAtMs"),
        field_i64(right, "lastActivityAtMs"),
    ) {
        (Some(left_ms), Some(right_ms)) if left_ms != right_ms => {
            return right_ms.cmp(&left_ms);
        }
        _ => {}
    }

    let left_activity = field_string(left, "lastActivityAt");
    let right_activity = field_string(right, "lastActivityAt");
    if left_activity != right_activity {
        return right_activity.cmp(&left_activity);
    }

    field_string(left, "ref").cmp(&field_string(right, "ref"))
}

fn field_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn field_string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SERVER_TIME: &str = "2026-06-25T00:00:02Z";

    #[test]
    fn empty_minis_return_no_mobile_snapshot() {
        let projection =
            reduce_state_minis_mobile_snapshot(0, vec![], String::new()).expect("empty projection");

        assert!(!projection.has_snapshot);
        assert!(projection.snapshot_json.is_empty());
    }

    #[test]
    fn mobile_snapshot_groups_surfaces_and_selects_latest_known_surface() {
        let projection = reduce_state_minis_mobile_snapshot(
            12,
            vec![
                mini(
                    "thread-codex",
                    "codex",
                    10,
                    "rev-10",
                    "C1",
                    1_781_596_920_123,
                ),
                mini(
                    "thread-devin",
                    "devin",
                    12,
                    "rev-12",
                    "D1",
                    1_781_596_920_321,
                ),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");

        assert!(projection.has_snapshot);
        assert_eq!(snapshot["revision"], "rev-12");
        assert_eq!(snapshot["host"]["id"], HOST_ID);
        assert_eq!(snapshot["host"]["lastSyncedAt"], SERVER_TIME);
        assert_eq!(snapshot["globalSettings"]["assistantSurface"], "devin");
        assert_eq!(snapshot["sessions"][0]["id"], "thread-devin");
        assert_eq!(
            snapshot["surfaceSessions"]["codex"][0]["id"],
            "thread-codex"
        );
    }

    #[test]
    fn mobile_snapshot_selects_surface_from_compact_revision() {
        let projection = reduce_state_minis_mobile_snapshot(
            42,
            vec![mini(
                "thread-codex",
                "codex",
                42,
                "threads=thread-codex:surface=claude-code:mobile-state=hash",
                "C1",
                1_781_596_920_123,
            )],
            SERVER_TIME.to_owned(),
        )
        .expect("projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");

        assert!(projection.has_snapshot);
        assert_eq!(
            snapshot["globalSettings"]["assistantSurface"],
            "claude-code"
        );
        assert!(
            snapshot["sessions"]
                .as_array()
                .expect("sessions")
                .is_empty()
        );
        assert_eq!(
            snapshot["surfaceSessions"]["codex"][0]["id"],
            "thread-codex"
        );
    }

    #[test]
    fn mobile_snapshot_sorts_sessions_by_activity_then_ref() {
        let projection = reduce_state_minis_mobile_snapshot(
            14,
            vec![
                mini("older", "codex", 11, "rev-11", "S2", 100),
                mini("tie-lower-ref", "codex", 12, "rev-12", "S1", 200),
                mini("newer", "codex", 13, "rev-13", "S3", 300),
                mini("tie-higher-ref", "codex", 14, "rev-14", "S4", 200),
            ],
            String::new(),
        )
        .expect("projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");
        let session_ids = snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert_eq!(
            session_ids,
            vec!["newer", "tie-lower-ref", "tie-higher-ref", "older"]
        );
    }

    #[test]
    fn mobile_snapshot_groups_codex_compatible_clients_under_codex_surface() {
        let projection = reduce_state_minis_mobile_snapshot(
            31,
            vec![
                mini("thread-cursor", "cursor", 30, "rev-30", "C1", 300),
                mini("thread-super", "super-engineering", 31, "rev-31", "S1", 200),
                mini("thread-codex", "codex", 29, "rev-29", "Z1", 100),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");
        let codex_sessions = snapshot["surfaceSessions"]["codex"]
            .as_array()
            .expect("codex surface sessions");
        let visible_session_ids = snapshot["sessions"]
            .as_array()
            .expect("visible sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert_eq!(snapshot["globalSettings"]["assistantSurface"], "codex");
        assert!(snapshot["surfaceSessions"].get("cursor").is_none());
        assert!(
            snapshot["surfaceSessions"]
                .get("super-engineering")
                .is_none()
        );
        assert_eq!(codex_sessions.len(), 3);
        assert_eq!(
            visible_session_ids,
            vec!["thread-cursor", "thread-super", "thread-codex"]
        );
        assert_eq!(codex_sessions[0]["assistantClient"], "cursor");
        assert_eq!(codex_sessions[1]["assistantClient"], "super-engineering");
        assert_eq!(codex_sessions[2]["assistantClient"], "codex");
    }

    #[test]
    fn mobile_snapshot_skips_stale_invalid_minis() {
        let projection = reduce_state_minis_mobile_snapshot(
            2,
            vec![
                ClientStateMini {
                    session_id: "envelope-id".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 1,
                    revision: "rev-1".to_owned(),
                    payload_json: session_json("payload-id", "S1", 100),
                },
                mini("thread-valid", "codex", 2, "rev-2", "S2", 200),
            ],
            String::new(),
        )
        .expect("projection skips invalid mini");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");

        assert!(projection.has_snapshot);
        assert_eq!(snapshot["sessions"].as_array().expect("sessions").len(), 1);
        assert_eq!(snapshot["sessions"][0]["id"], "thread-valid");
        assert_eq!(snapshot["revision"], "rev-2");
    }

    #[test]
    fn mobile_snapshot_returns_empty_when_all_minis_are_invalid() {
        let projection = reduce_state_minis_mobile_snapshot(
            1,
            vec![ClientStateMini {
                session_id: "envelope-id".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 1,
                revision: "rev-1".to_owned(),
                payload_json: session_json("payload-id", "S1", 100),
            }],
            String::new(),
        )
        .expect("projection");

        assert!(!projection.has_snapshot);
        assert!(projection.snapshot_json.is_empty());
    }

    #[test]
    fn mobile_snapshot_accepts_compact_minis_with_model_defaults() {
        let projection = reduce_state_minis_mobile_snapshot(
            21,
            vec![ClientStateMini {
                session_id: "thread-compact".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 21,
                revision: "rev-21".to_owned(),
                payload_json: json!({
                    "sessionId": "thread-compact",
                    "assistantSurface": "codex",
                    "effectiveMode": "await-reply",
                })
                .to_string(),
            }],
            SERVER_TIME.to_owned(),
        )
        .expect("compact projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");
        let session = &snapshot["sessions"][0];

        assert!(projection.has_snapshot);
        assert_eq!(session["id"], "thread-compact");
        assert_eq!(session["sessionId"], "thread-compact");
        assert_eq!(session["assistantSurface"], "codex");
        assert_eq!(session["assistantClient"], "codex");
        assert_eq!(session["ref"], "thread-compact");
        assert_eq!(session["title"], "thread-compact");
        assert_eq!(session["status"], DEFAULT_SESSION_STATUS);
        assert_eq!(session["lastUpdatedAt"], "");
        assert_eq!(session["lastActivityAt"], "");
        assert_eq!(session["isArchived"], false);
        assert_eq!(session["canSendPrompt"], true);
    }

    #[test]
    fn mobile_snapshot_applies_pending_commands_in_rust_projection() {
        let projection = reduce_state_minis_mobile_snapshot_with_pending_commands(
            31,
            vec![mini("thread-main", "codex", 31, "rev-31", "S31", 300)],
            vec![
                pending_command(
                    ClientPendingCommandKind::SetSessionMode,
                    "thread-main",
                    "max-turns-2",
                ),
                pending_command(
                    ClientPendingCommandKind::SetSiriCurrentSession,
                    "thread-main",
                    "codex",
                ),
                pending_command(
                    ClientPendingCommandKind::SaveDefaultPrompt,
                    "",
                    "Keep going",
                ),
                pending_command(
                    ClientPendingCommandKind::SetSessionArchived,
                    "thread-main",
                    "",
                ),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("pending projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");
        let session = &snapshot["sessions"][0];

        assert!(projection.has_snapshot);
        assert_eq!(session["effectiveMode"], "max-turns-2");
        assert_eq!(session["isArchived"], true);
        assert_eq!(session["status"], STATUS_ARCHIVED);
        assert_eq!(snapshot["globalSettings"]["defaultPrompt"], "Keep going");
        assert_eq!(
            snapshot["globalSettings"]["siriCurrentSessionId"],
            "thread-main"
        );
        assert_eq!(
            snapshot["globalSettings"]["siriCurrentAssistantSurface"],
            "codex"
        );
    }

    #[test]
    fn mobile_snapshot_applies_pending_delete_across_surfaces_in_rust_projection() {
        let projection = reduce_state_minis_mobile_snapshot_with_pending_commands(
            42,
            vec![
                mini("thread-delete", "codex", 42, "rev-42", "S42", 400),
                mini("thread-keep", "codex", 41, "rev-41", "S41", 300),
            ],
            vec![
                pending_command(
                    ClientPendingCommandKind::SetSiriDefaultSession,
                    "thread-delete",
                    "codex",
                ),
                pending_command(ClientPendingCommandKind::DeleteSession, "thread-delete", ""),
            ],
            SERVER_TIME.to_owned(),
        )
        .expect("pending delete projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");
        let session_ids = snapshot["surfaceSessions"]["codex"]
            .as_array()
            .expect("codex sessions")
            .iter()
            .map(|session| session["id"].as_str().expect("id"))
            .collect::<Vec<_>>();

        assert_eq!(session_ids, vec!["thread-keep"]);
        assert!(snapshot["globalSettings"]["siriDefaultSessionId"].is_null());
        assert!(snapshot["globalSettings"]["siriDefaultAssistantSurface"].is_null());
    }

    #[test]
    fn mobile_snapshot_repairs_partial_git_repository_metadata() {
        let projection = reduce_state_minis_mobile_snapshot(
            22,
            vec![ClientStateMini {
                session_id: "thread-partial-git".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 22,
                revision: "rev-22".to_owned(),
                payload_json: json!({
                    "id": "thread-partial-git",
                    "ref": "S22",
                    "title": "Partial Git",
                    "status": "active",
                    "lastUpdatedAt": "2026-06-16T08:02:00Z",
                    "lastActivityAt": "2026-06-16T08:02:00Z",
                    "metadata": {
                        "projectName": "looper",
                        "gitRepository": {
                            "repositoryName": "looper",
                            "branch": "main",
                            "remoteURL": null
                        }
                    }
                })
                .to_string(),
            }],
            SERVER_TIME.to_owned(),
        )
        .expect("partial git metadata projection");
        let snapshot: Value = serde_json::from_str(&projection.snapshot_json).expect("snapshot");
        let repository = &snapshot["sessions"][0]["metadata"]["gitRepository"];

        assert!(projection.has_snapshot);
        assert_eq!(repository["repositoryName"], "looper");
        assert_eq!(repository["repositoryPath"], "");
        assert_eq!(repository["branch"], "main");
    }

    fn mini(
        session_id: &str,
        assistant_surface: &str,
        seq: i64,
        revision: &str,
        ref_id: &str,
        activity_ms: i64,
    ) -> ClientStateMini {
        ClientStateMini {
            session_id: session_id.to_owned(),
            assistant_surface: assistant_surface.to_owned(),
            seq,
            revision: revision.to_owned(),
            payload_json: session_json(session_id, ref_id, activity_ms),
        }
    }

    fn pending_command(
        kind: ClientPendingCommandKind,
        thread_id: &str,
        value: &str,
    ) -> ClientPendingCommand {
        ClientPendingCommand {
            kind,
            client_mutation_id: format!("pending-{kind:?}-{thread_id}"),
            thread_id: thread_id.to_owned(),
            preset: if kind == ClientPendingCommandKind::SetSessionMode {
                value.to_owned()
            } else {
                String::new()
            },
            assistant_surface: if matches!(
                kind,
                ClientPendingCommandKind::SetSiriCurrentSession
                    | ClientPendingCommandKind::SetSiriDefaultSession
            ) {
                value.to_owned()
            } else {
                String::new()
            },
            prompt_intent: String::new(),
            prompt: if kind == ClientPendingCommandKind::SaveDefaultPrompt {
                value.to_owned()
            } else {
                String::new()
            },
            notification_id: String::new(),
            notification_target_ids: Vec::new(),
            archived: kind == ClientPendingCommandKind::SetSessionArchived,
            attempt_count: 0,
        }
    }

    fn session_json(session_id: &str, ref_id: &str, activity_ms: i64) -> String {
        json!({
            "id": session_id,
            "ref": ref_id,
            "title": session_id,
            "status": "active",
            "lastUpdatedAt": "2026-06-16T08:02:00Z",
            "lastActivityAt": "2026-06-16T08:02:00Z",
            "lastActivityAtMs": activity_ms,
            "lastMessageAt": "2026-06-16T08:01:00Z",
            "lastMessageAtMs": activity_ms - 1,
            "metadata": {"source": "test"},
        })
        .to_string()
    }
}

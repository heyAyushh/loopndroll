use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ClientCoreError;

const DEFAULT_ASSISTANT_SURFACE: &str = "codex";

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientSnapshotProjection {
    pub selected_assistant_surface: String,
    pub visible_snapshot_json: String,
    pub visible_session_ids: Vec<String>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotDocument {
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
    #[serde(rename = "effectiveMode")]
    #[serde(skip_serializing_if = "Option::is_none")]
    effective_mode: Option<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
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
) -> Result<ClientSnapshotProjection, ClientCoreError> {
    let snapshot = parse_snapshot(&snapshot_json)?;
    let selected_assistant_surface = selected_assistant_surface(
        &snapshot,
        &preferred_assistant_surface,
        has_user_selected_assistant_surface,
        &current_selected_assistant_surface,
    );
    project_snapshot(snapshot, selected_assistant_surface)
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
    let projection = project_snapshot(snapshot, selected_assistant_surface)?;

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
) -> Result<ClientSnapshotProjection, ClientCoreError> {
    let visible_sessions = sessions_for_surface(&snapshot, &selected_assistant_surface);
    let visible_session_ids = visible_sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();

    snapshot.global_settings.assistant_surface = selected_assistant_surface.clone();
    snapshot.sessions = visible_sessions;

    Ok(ClientSnapshotProjection {
        selected_assistant_surface,
        visible_snapshot_json: serialize_snapshot(&snapshot)?,
        visible_session_ids,
    })
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

    sync_detail_field(detail_object, session, "status");
    sync_detail_mode(detail_object, session);
    sync_detail_field(detail_object, session, "lastUpdatedAt");
    sync_detail_field(detail_object, session, "lastActivityAt");
    sync_detail_field(detail_object, session, "lastMessageAt");
    sync_detail_field(detail_object, session, "assistantPreview");
    sync_detail_field(detail_object, session, "isArchived");
    sync_detail_field(detail_object, session, "metadata");
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

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX: &str = "codex";
    const DEVIN: &str = "devin";
    const THREAD_ID: &str = "thread-main";

    #[test]
    fn snapshot_projection_uses_global_surface_until_user_selects() {
        let projection = reduce_mobile_snapshot_projection(
            snapshot_json(CODEX),
            String::new(),
            false,
            DEVIN.to_owned(),
        )
        .expect("project snapshot");
        let visible = parse_snapshot(&projection.visible_snapshot_json).expect("visible snapshot");

        assert_eq!(projection.selected_assistant_surface, CODEX);
        assert_eq!(projection.visible_session_ids, vec!["codex-thread"]);
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
        )
        .expect("project snapshot");
        let visible_value: Value =
            serde_json::from_str(&projection.visible_snapshot_json).expect("json");

        assert_eq!(projection.selected_assistant_surface, DEVIN);
        assert_eq!(projection.visible_session_ids, vec![THREAD_ID]);
        assert_eq!(visible_value["host"]["name"], "Looper");
        assert_eq!(visible_value["sessions"][0]["ref"], "D1");
        assert_eq!(visible_value["globalSettings"]["defaultPrompt"], "Continue");
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
    fn detail_cache_filters_and_syncs_visible_session_fields() {
        let projection = reduce_mobile_snapshot_projection(
            snapshot_json(DEVIN),
            DEVIN.to_owned(),
            true,
            CODEX.to_owned(),
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
}

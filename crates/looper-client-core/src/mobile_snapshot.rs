use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::error::ClientCoreError;
use crate::model::ClientStateMini;

const DEFAULT_ASSISTANT_SURFACE: &str = "codex";
const DEFAULT_PROMPT: &str = "Continue";
const DEFAULT_SESSION_STATUS: &str = "stopped";
const GLOBAL_SCOPE: &str = "global";
const HOST_ID: &str = "local-session-mini-cache";
const HOST_NAME: &str = "Looper";
const REVISION_PREFIX: &str = "mini:";

const KNOWN_ASSISTANT_SURFACES: [&str; 5] = ["codex", "claude-code", "devin", "grok-build", "zed"];

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
    if sessions.is_empty() {
        return Ok(ClientMobileSnapshotProjection {
            has_snapshot: false,
            snapshot_json: String::new(),
        });
    }

    let mut sessions_by_surface: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut latest_global_settings: Option<(i64, Value)> = None;
    for mini in &sessions {
        let session = decode_session_payload(mini)?;
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
            .entry(mini.assistant_surface.clone())
            .or_default()
            .push(session);
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
    insert_string_default(
        session,
        "assistantSurface",
        fallback_assistant_surface(&mini.assistant_surface),
    );
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

fn fallback_assistant_surface(surface: &str) -> &str {
    if surface.trim().is_empty() {
        DEFAULT_ASSISTANT_SURFACE
    } else {
        surface
    }
}

fn selected_surface(sessions: &[ClientStateMini]) -> String {
    let mut candidates = sessions
        .iter()
        .filter(|session| is_known_assistant_surface(&session.assistant_surface))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| {
        right
            .seq
            .cmp(&left.seq)
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    candidates
        .first()
        .map(|session| session.assistant_surface.clone())
        .unwrap_or_else(|| DEFAULT_ASSISTANT_SURFACE.to_owned())
}

fn is_known_assistant_surface(surface: &str) -> bool {
    KNOWN_ASSISTANT_SURFACES.contains(&surface)
}

fn revision(latest_seq: i64, sessions: &[ClientStateMini]) -> String {
    sessions
        .iter()
        .filter(|session| !session.revision.trim().is_empty())
        .max_by(|left, right| {
            left.seq
                .cmp(&right.seq)
                .then_with(|| left.assistant_surface.cmp(&right.assistant_surface))
                .then_with(|| left.session_id.cmp(&right.session_id))
        })
        .map(|session| session.revision.trim().to_owned())
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
    fn mobile_snapshot_rejects_mismatched_payload_session_id() {
        let err = reduce_state_minis_mobile_snapshot(
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
        .expect_err("mismatch");

        assert_eq!(err, ClientCoreError::StateMiniSessionIdMismatch);
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
        assert_eq!(session["ref"], "thread-compact");
        assert_eq!(session["title"], "thread-compact");
        assert_eq!(session["status"], DEFAULT_SESSION_STATUS);
        assert_eq!(session["lastUpdatedAt"], "");
        assert_eq!(session["lastActivityAt"], "");
        assert_eq!(session["isArchived"], false);
        assert_eq!(session["canSendPrompt"], true);
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

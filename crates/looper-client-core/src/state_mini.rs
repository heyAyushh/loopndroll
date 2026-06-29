#[cfg(test)]
use crate::model::ClientStateMiniDelta;
use crate::{error::ClientCoreError, model::ClientStateMini};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const INITIAL_SEQUENCE: i64 = 0;
pub(crate) const DEFAULT_ACCOUNT_ID: &str = "local-account";
pub(crate) const DEFAULT_NODE_ID: &str = "local-node";
pub(crate) const FRESHNESS_SOURCE_LOCAL: &str = "local";
pub(crate) const FRESHNESS_SOURCE_RECOVERY: &str = "recovery";
pub(crate) const FRESHNESS_SOURCE_STREAM: &str = "stream";
const ACCOUNT_ID_FIELD: &str = "accountId";
const ACCOUNT_ID_ALIAS_FIELD: &str = "accountID";
const ACCOUNT_ID_SNAKE_FIELD: &str = "account_id";
const NODE_ID_FIELD: &str = "nodeId";
const NODE_ID_ALIAS_FIELD: &str = "nodeID";
const NODE_ID_SNAKE_FIELD: &str = "node_id";
const SESSION_ID_FIELD: &str = "sessionId";
const SESSION_ID_ALIAS_FIELD: &str = "sessionID";
const PAYLOAD_ID_FIELD: &str = "id";
const ASSISTANT_SURFACE_FIELD: &str = "assistantSurface";
const REVISION_FIELD: &str = "revision";
const EFFECTIVE_MODE_FIELD: &str = "effectiveMode";
const MODE_FIELD: &str = "mode";
const REPLYABLE_FIELD: &str = "replyable";
const CAN_SEND_PROMPT_FIELD: &str = "canSendPrompt";
const BLOCKED_GOAL_FIELD: &str = "blockedGoal";
const QUEUE_COUNT_FIELD: &str = "queueCount";
const LIFECYCLE_FIELD: &str = "lifecycle";
const NOTIFICATION_STATUS_FIELD: &str = "notificationStatus";
const NOTIFICATION_ENABLED_FIELD: &str = "enabled";
const NOTIFICATION_KNOWN_FIELD: &str = "known";
const NOTIFICATION_TARGET_IDS_FIELD: &str = "targetIds";
const NOTIFICATION_USES_DEFAULT_FIELD: &str = "usesDefault";
const FRESHNESS_SOURCE_FIELD: &str = "freshnessSource";
const ROUTE_ENDPOINT_FIELD: &str = "routeEndpoint";

pub(crate) fn require_valid_sequence(sequence: i64) -> Result<(), ClientCoreError> {
    if sequence < INITIAL_SEQUENCE {
        Err(ClientCoreError::InvalidSequence)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_state_minis(sessions: &[ClientStateMini]) -> Result<(), ClientCoreError> {
    for session in sessions {
        validate_state_mini(session)?;
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_state_mini_delta(
    delta: &ClientStateMiniDelta,
) -> Result<(), ClientCoreError> {
    require_valid_sequence(delta.seq)?;
    require_valid_sequence(delta.latest_seq)?;
    if delta.has_session {
        validate_state_mini(&delta.session)?;
    }
    validate_state_minis(&delta.sessions)
}

pub(crate) fn validate_state_mini(session: &ClientStateMini) -> Result<(), ClientCoreError> {
    require_present(&session.session_id, ClientCoreError::EmptySessionId)?;
    require_valid_sequence(session.seq)
}

pub(crate) fn normalize_state_minis(sessions: Vec<ClientStateMini>) -> Vec<ClientStateMini> {
    normalize_state_minis_for_source(sessions, None, None)
}

pub(crate) fn normalize_state_minis_for_source(
    sessions: Vec<ClientStateMini>,
    freshness_source: Option<&str>,
    route_endpoint: Option<&str>,
) -> Vec<ClientStateMini> {
    let mut normalized = Vec::with_capacity(sessions.len());
    for session in sessions {
        let session = normalize_state_mini_for_source(session, freshness_source, route_endpoint);
        if let Some(index) = normalized
            .iter()
            .position(|current| same_state_mini_key(current, &session))
        {
            if session.seq >= normalized[index].seq {
                normalized[index] = session;
            }
        } else {
            normalized.push(session);
        }
    }
    sort_state_minis(&mut normalized);
    normalized
}

pub(crate) fn sort_state_minis(sessions: &mut [ClientStateMini]) {
    sessions.sort_by(|lhs, rhs| {
        lhs.seq
            .cmp(&rhs.seq)
            .then_with(|| state_mini_account_id(lhs).cmp(&state_mini_account_id(rhs)))
            .then_with(|| state_mini_node_id(lhs).cmp(&state_mini_node_id(rhs)))
            .then_with(|| lhs.assistant_surface.cmp(&rhs.assistant_surface))
            .then_with(|| lhs.session_id.cmp(&rhs.session_id))
    });
}

pub(crate) fn same_state_mini_key(lhs: &ClientStateMini, rhs: &ClientStateMini) -> bool {
    state_mini_account_id(lhs) == state_mini_account_id(rhs)
        && state_mini_node_id(lhs) == state_mini_node_id(rhs)
        && lhs.session_id == rhs.session_id
}

pub(crate) fn latest_state_mini_revision(sessions: &[ClientStateMini]) -> Option<String> {
    sessions
        .iter()
        .filter(|session| !session.revision.is_empty())
        .max_by(|lhs, rhs| {
            lhs.seq
                .cmp(&rhs.seq)
                .then_with(|| lhs.assistant_surface.cmp(&rhs.assistant_surface))
                .then_with(|| lhs.session_id.cmp(&rhs.session_id))
        })
        .map(|session| session.revision.clone())
}

pub(crate) fn normalize_state_mini_for_source(
    mut session: ClientStateMini,
    freshness_source: Option<&str>,
    route_endpoint: Option<&str>,
) -> ClientStateMini {
    let mut payload = payload_object(&session.payload_json);
    let account_id = first_nonblank_string(
        &payload,
        &[
            ACCOUNT_ID_FIELD,
            ACCOUNT_ID_ALIAS_FIELD,
            ACCOUNT_ID_SNAKE_FIELD,
        ],
    )
    .unwrap_or_else(|| DEFAULT_ACCOUNT_ID.to_owned());
    let node_id = first_nonblank_string(
        &payload,
        &[NODE_ID_FIELD, NODE_ID_ALIAS_FIELD, NODE_ID_SNAKE_FIELD],
    )
    .unwrap_or_else(|| DEFAULT_NODE_ID.to_owned());
    let session_id = first_nonblank_string(&payload, &[SESSION_ID_FIELD, SESSION_ID_ALIAS_FIELD])
        .or_else(|| first_nonblank_string(&payload, &[PAYLOAD_ID_FIELD]))
        .unwrap_or_else(|| session.session_id.clone());
    let assistant_surface = nonblank_string(&session.assistant_surface)
        .or_else(|| first_nonblank_string(&payload, &[ASSISTANT_SURFACE_FIELD]))
        .unwrap_or_default();
    let revision = nonblank_string(&session.revision)
        .or_else(|| first_nonblank_string(&payload, &[REVISION_FIELD]))
        .unwrap_or_else(|| format!("mini:{}", session.seq));

    session.session_id = session_id;
    session.assistant_surface = assistant_surface.clone();
    session.revision = revision.clone();

    payload.insert(ACCOUNT_ID_FIELD.to_owned(), Value::String(account_id));
    payload.insert(NODE_ID_FIELD.to_owned(), Value::String(node_id));
    payload.insert(
        SESSION_ID_FIELD.to_owned(),
        Value::String(session.session_id.clone()),
    );
    payload.insert(
        PAYLOAD_ID_FIELD.to_owned(),
        Value::String(session.session_id.clone()),
    );
    payload.insert(
        ASSISTANT_SURFACE_FIELD.to_owned(),
        Value::String(assistant_surface),
    );
    payload.insert(REVISION_FIELD.to_owned(), Value::String(revision));
    match first_nonblank_string(&payload, &[EFFECTIVE_MODE_FIELD, MODE_FIELD]) {
        Some(mode) => {
            payload.insert(EFFECTIVE_MODE_FIELD.to_owned(), Value::String(mode));
        }
        None => ensure_string_field(&mut payload, EFFECTIVE_MODE_FIELD, ""),
    }
    let replyable = payload
        .get(REPLYABLE_FIELD)
        .and_then(Value::as_bool)
        .or_else(|| payload.get(CAN_SEND_PROMPT_FIELD).and_then(Value::as_bool))
        .unwrap_or(false);
    payload.insert(REPLYABLE_FIELD.to_owned(), Value::Bool(replyable));
    payload.insert(CAN_SEND_PROMPT_FIELD.to_owned(), Value::Bool(replyable));
    payload
        .entry(BLOCKED_GOAL_FIELD.to_owned())
        .or_insert(Value::Null);
    if payload
        .get(QUEUE_COUNT_FIELD)
        .and_then(Value::as_i64)
        .is_none()
    {
        payload.insert(QUEUE_COUNT_FIELD.to_owned(), Value::from(0));
    }
    ensure_string_field(&mut payload, LIFECYCLE_FIELD, "");
    if !payload
        .get(NOTIFICATION_STATUS_FIELD)
        .is_some_and(Value::is_object)
    {
        payload.insert(
            NOTIFICATION_STATUS_FIELD.to_owned(),
            json!({
                NOTIFICATION_ENABLED_FIELD: false,
                NOTIFICATION_KNOWN_FIELD: false,
                NOTIFICATION_TARGET_IDS_FIELD: [],
                NOTIFICATION_USES_DEFAULT_FIELD: false,
            }),
        );
    }
    match freshness_source.and_then(nonblank_string) {
        Some(source) => {
            payload.insert(FRESHNESS_SOURCE_FIELD.to_owned(), Value::String(source));
        }
        None => ensure_string_field(&mut payload, FRESHNESS_SOURCE_FIELD, FRESHNESS_SOURCE_LOCAL),
    }
    match route_endpoint.and_then(nonblank_string) {
        Some(endpoint) => {
            payload.insert(ROUTE_ENDPOINT_FIELD.to_owned(), Value::String(endpoint));
        }
        None => ensure_string_field(&mut payload, ROUTE_ENDPOINT_FIELD, ""),
    }

    session.payload_json = serde_json::to_string(&Value::Object(payload)).unwrap_or_default();
    session
}

pub(crate) fn state_mini_node_id(session: &ClientStateMini) -> String {
    let payload = payload_object(&session.payload_json);
    first_nonblank_string(
        &payload,
        &[NODE_ID_FIELD, NODE_ID_ALIAS_FIELD, NODE_ID_SNAKE_FIELD],
    )
    .unwrap_or_else(|| DEFAULT_NODE_ID.to_owned())
}

pub(crate) fn state_mini_account_id(session: &ClientStateMini) -> String {
    let payload = payload_object(&session.payload_json);
    first_nonblank_string(
        &payload,
        &[
            ACCOUNT_ID_FIELD,
            ACCOUNT_ID_ALIAS_FIELD,
            ACCOUNT_ID_SNAKE_FIELD,
        ],
    )
    .unwrap_or_else(|| DEFAULT_ACCOUNT_ID.to_owned())
}

pub(crate) fn last_seq_by_node_from_minis(sessions: &[ClientStateMini]) -> BTreeMap<String, i64> {
    let mut last_seq_by_node = BTreeMap::new();
    for session in sessions {
        let node_id = state_mini_node_id(session);
        let entry = last_seq_by_node.entry(node_id).or_insert(INITIAL_SEQUENCE);
        *entry = (*entry).max(session.seq);
    }
    last_seq_by_node
}

fn payload_object(payload_json: &str) -> Map<String, Value> {
    serde_json::from_str::<Value>(payload_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn first_nonblank_string(payload: &Map<String, Value>, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| payload.get(*field).and_then(Value::as_str))
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

fn ensure_string_field(payload: &mut Map<String, Value>, field: &str, fallback: &str) {
    let needs_default = payload
        .get(field)
        .and_then(Value::as_str)
        .map(|value| value.trim().is_empty())
        .unwrap_or(true);
    if needs_default {
        payload.insert(field.to_owned(), Value::String(fallback.to_owned()));
    }
}

fn require_present(value: &str, error: ClientCoreError) -> Result<(), ClientCoreError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

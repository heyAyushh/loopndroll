#[cfg(test)]
use crate::model::ClientStateMiniDelta;
use crate::{error::ClientCoreError, model::ClientStateMini};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct StateMiniKey {
    account_id: String,
    node_id: String,
    assistant_surface: String,
    session_id: String,
}

impl StateMiniKey {
    pub(crate) fn node_id(&self) -> &str {
        &self.node_id
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct StateMiniSortKey {
    seq: i64,
    account_id: String,
    node_id: String,
    assistant_surface: String,
    session_id: String,
}

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
    let mut normalized: Vec<ClientStateMini> = Vec::with_capacity(sessions.len());
    let mut index_by_key: HashMap<StateMiniKey, usize> = HashMap::with_capacity(sessions.len());
    for session in sessions {
        let (session, key) =
            normalize_state_mini_for_source_with_key(session, freshness_source, route_endpoint);
        if let Some(index) = index_by_key.get(&key).copied() {
            if session.seq >= normalized[index].seq {
                normalized[index] = session;
            }
        } else {
            index_by_key.insert(key, normalized.len());
            normalized.push(session);
        }
    }
    sort_state_minis(&mut normalized);
    normalized
}

pub(crate) fn sort_state_minis(sessions: &mut [ClientStateMini]) {
    sessions.sort_by_cached_key(state_mini_sort_key);
}

pub(crate) fn same_state_mini_key(lhs: &ClientStateMini, rhs: &ClientStateMini) -> bool {
    state_mini_key(lhs) == state_mini_key(rhs)
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
    session: ClientStateMini,
    freshness_source: Option<&str>,
    route_endpoint: Option<&str>,
) -> ClientStateMini {
    normalize_state_mini_for_source_with_key(session, freshness_source, route_endpoint).0
}

fn normalize_state_mini_for_source_with_key(
    mut session: ClientStateMini,
    freshness_source: Option<&str>,
    route_endpoint: Option<&str>,
) -> (ClientStateMini, StateMiniKey) {
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
    let key = StateMiniKey {
        account_id: account_id.clone(),
        node_id: node_id.clone(),
        assistant_surface: assistant_surface.clone(),
        session_id: session.session_id.clone(),
    };

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
    (session, key)
}

pub(crate) fn state_mini_key(session: &ClientStateMini) -> StateMiniKey {
    let payload = payload_object(&session.payload_json);
    StateMiniKey {
        account_id: first_nonblank_string(
            &payload,
            &[
                ACCOUNT_ID_FIELD,
                ACCOUNT_ID_ALIAS_FIELD,
                ACCOUNT_ID_SNAKE_FIELD,
            ],
        )
        .unwrap_or_else(|| DEFAULT_ACCOUNT_ID.to_owned()),
        node_id: first_nonblank_string(
            &payload,
            &[NODE_ID_FIELD, NODE_ID_ALIAS_FIELD, NODE_ID_SNAKE_FIELD],
        )
        .unwrap_or_else(|| DEFAULT_NODE_ID.to_owned()),
        assistant_surface: nonblank_string(&session.assistant_surface)
            .or_else(|| first_nonblank_string(&payload, &[ASSISTANT_SURFACE_FIELD]))
            .unwrap_or_default(),
        session_id: session.session_id.clone(),
    }
}

fn state_mini_sort_key(session: &ClientStateMini) -> StateMiniSortKey {
    let key = state_mini_key(session);
    StateMiniSortKey {
        seq: session.seq,
        account_id: key.account_id,
        node_id: key.node_id,
        assistant_surface: key.assistant_surface,
        session_id: key.session_id,
    }
}

pub(crate) fn state_mini_node_id(session: &ClientStateMini) -> String {
    let payload = payload_object(&session.payload_json);
    first_nonblank_string(
        &payload,
        &[NODE_ID_FIELD, NODE_ID_ALIAS_FIELD, NODE_ID_SNAKE_FIELD],
    )
    .unwrap_or_else(|| DEFAULT_NODE_ID.to_owned())
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

pub(crate) fn state_mini_snapshot_is_stale_for_all_nodes(
    current_latest_seq: i64,
    current_last_seq_by_node: &BTreeMap<String, i64>,
    current_sessions: &[ClientStateMini],
    incoming_latest_seq: i64,
    incoming_sessions: &[ClientStateMini],
) -> bool {
    let current_last_seq_by_node = merged_current_last_seq_by_node(
        current_latest_seq,
        current_last_seq_by_node,
        current_sessions,
    );
    let incoming_last_seq_by_node = last_seq_by_node_from_minis(incoming_sessions);
    if incoming_last_seq_by_node.is_empty() {
        return incoming_latest_seq <= current_latest_seq && current_latest_seq > INITIAL_SEQUENCE;
    }

    fresh_state_mini_snapshot_node_ids(
        current_latest_seq,
        &current_last_seq_by_node,
        current_sessions,
        incoming_sessions,
    )
    .is_empty()
}

pub(crate) fn fresh_state_mini_snapshot_node_ids(
    current_latest_seq: i64,
    current_last_seq_by_node: &BTreeMap<String, i64>,
    current_sessions: &[ClientStateMini],
    incoming_sessions: &[ClientStateMini],
) -> BTreeSet<String> {
    let current_last_seq_by_node = merged_current_last_seq_by_node(
        current_latest_seq,
        current_last_seq_by_node,
        current_sessions,
    );
    last_seq_by_node_from_minis(incoming_sessions)
        .into_iter()
        .filter_map(|(node_id, incoming_seq)| {
            let current_seq = current_last_seq_by_node
                .get(&node_id)
                .copied()
                .unwrap_or(INITIAL_SEQUENCE);
            if incoming_seq > current_seq {
                Some(node_id)
            } else {
                None
            }
        })
        .collect()
}

pub(crate) fn fresh_state_mini_snapshot_covered_node_ids(
    current_latest_seq: i64,
    current_last_seq_by_node: &BTreeMap<String, i64>,
    current_sessions: &[ClientStateMini],
    incoming_latest_seq: i64,
    incoming_sessions: &[ClientStateMini],
) -> BTreeSet<String> {
    let current_last_seq_by_node = merged_current_last_seq_by_node(
        current_latest_seq,
        current_last_seq_by_node,
        current_sessions,
    );
    let fresh_node_ids = fresh_state_mini_snapshot_node_ids(
        current_latest_seq,
        &current_last_seq_by_node,
        current_sessions,
        incoming_sessions,
    );
    let current_default_seq = current_last_seq_by_node
        .get(DEFAULT_NODE_ID)
        .copied()
        .unwrap_or(INITIAL_SEQUENCE);
    if fresh_node_ids.is_empty()
        && incoming_sessions.is_empty()
        && incoming_latest_seq > current_default_seq
    {
        BTreeSet::from([DEFAULT_NODE_ID.to_owned()])
    } else {
        fresh_node_ids
    }
}

pub(crate) fn state_mini_snapshot_last_seq_by_node(
    snapshot_latest_seq: i64,
    sessions: &[ClientStateMini],
    node_ids: &BTreeSet<String>,
) -> BTreeMap<String, i64> {
    let incoming_last_seq_by_node = last_seq_by_node_from_minis(sessions);
    node_ids
        .iter()
        .filter_map(|node_id| {
            let seq = incoming_last_seq_by_node
                .get(node_id)
                .copied()
                .unwrap_or(snapshot_latest_seq);
            (seq > INITIAL_SEQUENCE).then(|| (node_id.clone(), seq))
        })
        .collect()
}

fn merged_current_last_seq_by_node(
    current_latest_seq: i64,
    current_last_seq_by_node: &BTreeMap<String, i64>,
    current_sessions: &[ClientStateMini],
) -> BTreeMap<String, i64> {
    let mut merged = last_seq_by_node_from_minis(current_sessions);
    for (node_id, seq) in current_last_seq_by_node {
        let entry = merged.entry(node_id.clone()).or_insert(INITIAL_SEQUENCE);
        *entry = (*entry).max(*seq);
    }
    if merged.is_empty() && current_latest_seq > INITIAL_SEQUENCE {
        merged.insert(DEFAULT_NODE_ID.to_owned(), current_latest_seq);
    }
    merged
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

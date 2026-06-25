use crate::control_plane::ControlPlane;
use crate::events::{
    MobileCommandAckInput, MobileCommandAckRecord, MobileCommandAckResult,
    MobileCommandReservationResult, MobileStateEventInput,
};
use crate::mobile::events::{MobileEventInput, MobileEventKind, mobile_event_now};

const ACK_PAYLOAD_DETAIL: &str = "command-ack";

pub(crate) enum CommandReservation {
    Reserved,
    Replay(MobileCommandAckRecord),
}

#[derive(Debug)]
pub(crate) enum CommandAckError {
    AlreadyExists(String),
    InFlight(String),
    Internal(String),
}

pub(crate) fn command_request_hash(
    command_kind: &str,
    payload: serde_json::Value,
) -> Result<String, CommandAckError> {
    use sha2::{Digest, Sha256};

    let body = serde_json::json!({
        "commandKind": command_kind,
        "payload": payload,
    });
    let encoded =
        serde_json::to_vec(&body).map_err(|error| CommandAckError::Internal(error.to_string()))?;
    let digest = Sha256::digest(encoded);
    Ok(format!("sha256:{digest:x}"))
}

pub(crate) fn existing_command_ack(
    control_plane: &ControlPlane,
    command_kind: &str,
    client_mutation_id: &str,
    request_hash: &str,
) -> Result<Option<MobileCommandAckRecord>, CommandAckError> {
    let Some(record) = control_plane
        .store()
        .mobile_command_ack(command_kind, client_mutation_id)
        .map_err(|error| CommandAckError::Internal(error.to_string()))?
    else {
        return Ok(None);
    };
    if record.request_hash == request_hash {
        if record.ack_seq == 0 {
            return Ok(None);
        }
        return Ok(Some(record));
    }
    Err(CommandAckError::AlreadyExists(format!(
        "client_mutation_id already used for {command_kind}"
    )))
}

pub(crate) fn reserve_command_ack(
    control_plane: &ControlPlane,
    command_kind: &str,
    client_mutation_id: &str,
    request_hash: &str,
) -> Result<CommandReservation, CommandAckError> {
    match control_plane
        .store()
        .reserve_mobile_command_ack(command_kind, client_mutation_id, request_hash)
        .map_err(|error| CommandAckError::Internal(error.to_string()))?
    {
        MobileCommandReservationResult::Reserved(_) => Ok(CommandReservation::Reserved),
        MobileCommandReservationResult::Duplicate(record) => Ok(CommandReservation::Replay(record)),
        MobileCommandReservationResult::Conflict(_) => Err(CommandAckError::AlreadyExists(
            format!("client_mutation_id already used for {command_kind}"),
        )),
        MobileCommandReservationResult::InFlight(_) => Err(CommandAckError::InFlight(format!(
            "client_mutation_id already in flight for {command_kind}"
        ))),
    }
}

pub(crate) fn release_command_reservation(
    control_plane: &ControlPlane,
    command_kind: &str,
    client_mutation_id: &str,
    request_hash: &str,
) -> Result<(), CommandAckError> {
    control_plane
        .store()
        .clear_mobile_command_reservation(command_kind, client_mutation_id, request_hash)
        .map(|_| ())
        .map_err(|error| CommandAckError::Internal(error.to_string()))
}

pub(crate) fn record_command_ack(
    control_plane: &ControlPlane,
    command_kind: &str,
    client_mutation_id: &str,
    request_hash: &str,
    response_json: serde_json::Value,
    state_event: MobileStateEventInput,
) -> Result<MobileCommandAckResult, CommandAckError> {
    match control_plane
        .store()
        .record_mobile_command_ack(MobileCommandAckInput {
            command_kind: command_kind.to_owned(),
            client_mutation_id: client_mutation_id.to_owned(),
            request_hash: request_hash.to_owned(),
            response_json,
            state_event,
        })
        .map_err(|error| CommandAckError::Internal(error.to_string()))?
    {
        MobileCommandAckResult::Conflict(_) => Err(CommandAckError::AlreadyExists(format!(
            "client_mutation_id already used for {command_kind}"
        ))),
        result => Ok(result),
    }
}

pub(crate) fn command_ack_state_event(
    entity_id: &str,
    revision: &str,
    server_time: &str,
) -> MobileStateEventInput {
    MobileStateEventInput {
        entity_id: entity_id.to_owned(),
        kind: MobileEventKind::SessionChanged,
        revision: revision.to_owned(),
        server_time: server_time.to_owned(),
        payload_json: serde_json::json!({
            "threadId": entity_id,
            "detail": ACK_PAYLOAD_DETAIL,
        }),
        client_mutation_id: None,
        command_kind: None,
        command_request_hash: None,
        command_response_json: None,
    }
}

pub(crate) fn publish_command_ack_event(control_plane: &ControlPlane, entity_id: &str) {
    control_plane.publish_mobile_session_event_without_persisting(MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: Some(entity_id.to_owned()),
        prompt_id: None,
        detail: Some(ACK_PAYLOAD_DETAIL.to_owned()),
    });
}

pub(crate) fn ack_response_value(record: &MobileCommandAckRecord) -> serde_json::Value {
    serde_json::from_str(&record.response_json).unwrap_or_else(|_| serde_json::json!({}))
}

pub(crate) fn json_string(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

pub(crate) fn current_mobile_revision(
    control_plane: &ControlPlane,
) -> Result<String, CommandAckError> {
    let records = control_plane
        .store()
        .mobile_session_minis()
        .map_err(|error| CommandAckError::Internal(error.to_string()))?;
    if let Some(revision) = crate::mobile::api::latest_session_mini_revision(&records) {
        return Ok(revision);
    }

    Err(CommandAckError::Internal(
        "state mini revision is required before recording a command ACK".to_owned(),
    ))
}

pub(crate) fn command_ack_server_time() -> String {
    mobile_event_now()
}

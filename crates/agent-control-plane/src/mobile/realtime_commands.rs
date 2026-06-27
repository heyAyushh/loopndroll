use serde::Serialize;

use crate::control_plane::ControlPlane;
use crate::control_plane::reducer::session_state_for_thread;
use crate::control_plane::session_fsm::{
    SessionCommand, SessionReject, next as next_session_state,
};
use crate::events::{MobileCommandAckRecord, MobileCommandAckResult};
use crate::mobile::api::session_mini_records_contain_session;
use crate::mobile::prompt_delivery::{
    PromptIntent, accept_session_prompt, dispatch_session_prompt_after_ack, prompt_dispatch_fields,
};
use crate::mobile::realtime_ack::{
    CommandAckError, CommandReservation, ack_response_value, command_ack_server_time,
    command_ack_state_event, command_request_hash, current_mobile_revision, existing_command_ack,
    json_string, publish_command_ack_event, record_command_ack, release_command_reservation,
    reserve_command_ack,
};
use crate::mobile::session::{ASSISTANT_SURFACES, MobileSessionError};

const COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY: &str = "SubmitNotificationReply";

pub(crate) struct SubmitNotificationReplyInput<'a> {
    pub(crate) notification_id: &'a str,
    pub(crate) thread_id: &'a str,
    pub(crate) prompt: &'a str,
    pub(crate) assistant_surface: Option<&'a str>,
    pub(crate) client_mutation_id: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationReplyCommandResponse {
    pub(crate) accepted: bool,
    pub(crate) dispatch_kind: String,
    pub(crate) prompt_id: String,
    pub(crate) server_time: String,
    pub(crate) client_mutation_id: String,
    pub(crate) ack_seq: i64,
    pub(crate) entity_id: String,
    pub(crate) revision: String,
    pub(crate) idempotent_replay: bool,
    pub(crate) notification_id: String,
}

#[derive(Debug)]
pub(crate) enum RealtimeCommandError {
    InvalidArgument(String),
    AlreadyExists(String),
    NotFound(String),
    MobileSession(MobileSessionError),
    SessionRejected(SessionReject),
    Internal(String),
}

impl From<CommandAckError> for RealtimeCommandError {
    fn from(error: CommandAckError) -> Self {
        match error {
            CommandAckError::AlreadyExists(message) => Self::AlreadyExists(message),
            CommandAckError::InFlight(message) => Self::AlreadyExists(message),
            CommandAckError::Internal(message) => Self::Internal(message),
        }
    }
}

pub(crate) fn submit_notification_reply_command(
    control_plane: &ControlPlane,
    input: SubmitNotificationReplyInput<'_>,
) -> Result<NotificationReplyCommandResponse, RealtimeCommandError> {
    let notification_id = normalized_required_value(input.notification_id, "notification_id")?;
    let thread_id = normalized_required_value(input.thread_id, "thread_id")?;
    let prompt = input.prompt;
    let client_mutation_id =
        normalized_required_value(input.client_mutation_id, "client_mutation_id")?;
    let assistant_surface = normalized_assistant_surface(input.assistant_surface.unwrap_or(""))?;
    let request_hash = command_request_hash(
        COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
        serde_json::json!({
            "notificationId": notification_id,
            "threadId": thread_id,
            "prompt": prompt,
            "assistantSurface": assistant_surface,
        }),
    )?;

    if let Some(record) = existing_command_ack(
        control_plane,
        COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
        client_mutation_id,
        &request_hash,
    )? {
        return Ok(notification_reply_response_from_record(
            notification_id,
            &record,
            true,
        ));
    }
    let reservation = reserve_command_ack(
        control_plane,
        COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
        client_mutation_id,
        &request_hash,
    )?;
    if let CommandReservation::Replay(record) = reservation {
        return Ok(notification_reply_response_from_record(
            notification_id,
            &record,
            true,
        ));
    }

    if let Err(error) =
        ensure_mobile_session_visible_from_minis(control_plane, thread_id, assistant_surface)
    {
        release_command_reservation(
            control_plane,
            COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
            client_mutation_id,
            &request_hash,
        )?;
        return Err(error);
    }
    if let Err(error) = ensure_session_fsm_allows(
        control_plane,
        thread_id,
        assistant_surface,
        client_mutation_id,
    ) {
        release_command_reservation(
            control_plane,
            COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
            client_mutation_id,
            &request_hash,
        )?;
        return Err(error);
    }
    let accepted_delivery = match accept_session_prompt(
        control_plane,
        thread_id,
        assistant_surface,
        prompt,
        PromptIntent::Queue,
    ) {
        Ok(delivery) => delivery,
        Err(error) => {
            release_command_reservation(
                control_plane,
                COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
                client_mutation_id,
                &request_hash,
            )?;
            return Err(RealtimeCommandError::MobileSession(error));
        }
    };
    let dispatch = accepted_delivery.dispatch.clone();
    let after_ack = accepted_delivery.after_ack;
    let server_time = command_ack_server_time();
    let revision = current_mobile_revision(control_plane)?;
    let entity_id = thread_id.to_owned();
    let (dispatch_kind, prompt_id) = prompt_dispatch_fields(&dispatch);
    let response_json = serde_json::json!({
        "accepted": true,
        "dispatchKind": dispatch_kind,
        "promptId": prompt_id,
        "serverTime": server_time,
        "entityId": entity_id,
        "revision": revision,
        "notificationId": notification_id,
    });
    let ack_result = match record_command_ack(
        control_plane,
        COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
        client_mutation_id,
        &request_hash,
        response_json,
        command_ack_state_event(&entity_id, &revision, &server_time),
    ) {
        Ok(result) => result,
        Err(error) => {
            release_command_reservation(
                control_plane,
                COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
                client_mutation_id,
                &request_hash,
            )?;
            return Err(error.into());
        }
    };
    if matches!(ack_result, MobileCommandAckResult::Recorded(_)) {
        publish_command_ack_event(control_plane, &entity_id);
        dispatch_session_prompt_after_ack(control_plane.clone(), after_ack);
    }

    Ok(notification_reply_response_from_ack_result(
        notification_id,
        &ack_result,
    ))
}

fn normalized_required_value<'a>(
    value: &'a str,
    field_name: &str,
) -> Result<&'a str, RealtimeCommandError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(RealtimeCommandError::InvalidArgument(format!(
            "{field_name} is required"
        )));
    }
    Ok(value)
}

fn normalized_assistant_surface(value: &str) -> Result<Option<&str>, RealtimeCommandError> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if ASSISTANT_SURFACES.contains(&value) {
        return Ok(Some(value));
    }
    Err(RealtimeCommandError::InvalidArgument(
        "invalid assistant surface".to_owned(),
    ))
}

fn ensure_mobile_session_visible_from_minis(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<(), RealtimeCommandError> {
    match session_mini_visibility(control_plane, thread_id, assistant_surface)? {
        Some(true) => Ok(()),
        Some(false) => Err(RealtimeCommandError::NotFound(
            "session not found".to_owned(),
        )),
        None => Err(RealtimeCommandError::InvalidArgument(
            "state mini cache is required before replying to a notification".to_owned(),
        )),
    }
}

fn session_mini_visibility(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<Option<bool>, RealtimeCommandError> {
    let records = control_plane
        .store()
        .mobile_session_minis()
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
    Ok(session_mini_records_contain_session(
        &records,
        thread_id,
        assistant_surface,
    ))
}

fn ensure_session_fsm_allows(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    client_mutation_id: &str,
) -> Result<(), RealtimeCommandError> {
    let events = control_plane
        .store()
        .mobile_state_events()
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
    let minis = control_plane
        .store()
        .mobile_session_minis()
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
    let state = session_state_for_thread(&events, &minis, thread_id, assistant_surface);
    next_session_state(
        state,
        SessionCommand::SubmitNotificationReply {
            client_mutation_id: client_mutation_id.to_owned(),
        },
    )
    .map(|_| ())
    .map_err(RealtimeCommandError::SessionRejected)
}

fn notification_reply_response_from_ack_result(
    notification_id: &str,
    result: &MobileCommandAckResult,
) -> NotificationReplyCommandResponse {
    notification_reply_response_from_record(
        notification_id,
        result.record(),
        matches!(result, MobileCommandAckResult::Duplicate(_)),
    )
}

fn notification_reply_response_from_record(
    notification_id: &str,
    record: &MobileCommandAckRecord,
    idempotent_replay: bool,
) -> NotificationReplyCommandResponse {
    let value = ack_response_value(record);
    NotificationReplyCommandResponse {
        accepted: true,
        dispatch_kind: json_string(&value, "dispatchKind"),
        prompt_id: json_string(&value, "promptId"),
        server_time: json_string(&value, "serverTime"),
        client_mutation_id: record.client_mutation_id.clone(),
        ack_seq: record.ack_seq,
        entity_id: json_string(&value, "entityId"),
        revision: json_string(&value, "revision"),
        idempotent_replay,
        notification_id: notification_id.to_owned(),
    }
}

use serde::Serialize;
use tonic::Status;

use crate::control_plane::ControlPlane;
use crate::control_plane::reducer::session_state_for_thread;
use crate::control_plane::session_fsm::{
    SessionCommand, SessionMode, SessionReject, SessionRejectCode, SessionState,
    next as next_session_state,
};
use crate::events::{MobileCommandAckRecord, MobileCommandAckResult};
use crate::mobile::api::{
    session_mini_projection_inputs_with_mobile_state, session_mini_projection_inputs_with_mode,
    session_mini_records_contain_session,
};
use crate::mobile::events::{MobileEventInput, MobileEventKind};
use crate::mobile::prompt_delivery::{
    PromptIntent, accept_session_prompt, dispatch_session_prompt_after_ack,
    invalidate_delivery_action_cache, prompt_dispatch_fields, prompt_intent_from_str,
};
use crate::mobile::realtime_ack::{
    COMMAND_ACK_ACCOUNT_ID, COMMAND_ACK_NODE_ID, CommandAckError, CommandReservation,
    ack_response_value, command_ack_server_time, command_ack_state_event, command_request_hash,
    current_mobile_revision, existing_command_ack, json_string, publish_command_ack_event,
    record_command_ack, release_command_reservation, reserve_command_ack,
};
use crate::mobile::session::{
    ASSISTANT_SURFACES, MobileSessionError, UpsertMobileNotificationRoute,
};

pub(crate) const COMMAND_KIND_SET_SESSION_MODE: &str = "SetSessionMode";
pub(crate) const COMMAND_KIND_SEND_SESSION_PROMPT: &str = "SendSessionPrompt";
const COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY: &str = "SubmitNotificationReply";
pub(crate) const COMMAND_KIND_SET_SIRI_CURRENT_SESSION: &str = "SetSiriCurrentSession";
pub(crate) const COMMAND_KIND_SET_SIRI_DEFAULT_SESSION: &str = "SetSiriDefaultSession";
pub(crate) const COMMAND_KIND_SAVE_DEFAULT_PROMPT: &str = "SaveDefaultPrompt";
pub(crate) const COMMAND_KIND_SET_SESSION_ARCHIVED: &str = "SetSessionArchived";
pub(crate) const COMMAND_KIND_DELETE_SESSION: &str = "DeleteSession";
pub(crate) const COMMAND_KIND_MUTE_SESSION: &str = "MuteSession";
pub(crate) const COMMAND_KIND_SET_SCOPE: &str = "SetScope";
pub(crate) const COMMAND_KIND_SET_ASSISTANT_SURFACE: &str = "SetAssistantSurface";
pub(crate) const COMMAND_KIND_SET_GLOBAL_PRESET: &str = "SetGlobalPreset";
pub(crate) const COMMAND_KIND_SET_GLOBAL_NOTIFICATION: &str = "SetGlobalNotification";
pub(crate) const COMMAND_KIND_SET_DEFAULT_NOTIFICATION_TARGETS: &str =
    "SetDefaultNotificationTargets";
pub(crate) const COMMAND_KIND_SET_GLOBAL_COMPLETION_CHECK: &str = "SetGlobalCompletionCheck";
pub(crate) const COMMAND_KIND_UPSERT_NOTIFICATION_ROUTE: &str = "UpsertNotificationRoute";
pub(crate) const COMMAND_KIND_DELETE_NOTIFICATION_ROUTE: &str = "DeleteNotificationRoute";
pub(crate) const COMMAND_KIND_UPSERT_COMPLETION_CHECK: &str = "UpsertCompletionCheck";
pub(crate) const COMMAND_KIND_DELETE_COMPLETION_CHECK: &str = "DeleteCompletionCheck";
pub(crate) const COMMAND_KIND_SET_SESSION_NOTIFICATIONS: &str = "SetSessionNotifications";
pub(crate) const COMMAND_KIND_SET_SESSION_COMPLETION_CHECK: &str = "SetSessionCompletionCheck";
const MOBILE_SETTINGS_ENTITY_ID: &str = "mobile-settings";
const MODE_CLEARED_DETAIL: &str = "mode-cleared";
const MODE_UPDATED_DETAIL: &str = "mode-updated";

pub(crate) struct SubmitNotificationReplyInput<'a> {
    pub(crate) notification_id: &'a str,
    pub(crate) thread_id: &'a str,
    pub(crate) prompt: &'a str,
    pub(crate) assistant_surface: Option<&'a str>,
    pub(crate) client_mutation_id: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) enum SiriSessionTarget {
    Current,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionCommandAckResponse {
    pub(crate) accepted: bool,
    pub(crate) account_id: String,
    pub(crate) node_id: String,
    pub(crate) server_time: String,
    pub(crate) client_mutation_id: String,
    pub(crate) ack_seq: i64,
    pub(crate) entity_id: String,
    pub(crate) revision: String,
    pub(crate) idempotent_replay: bool,
    pub(crate) error_code: String,
    pub(crate) reject_reason: String,
    pub(crate) current_state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NotificationReplyCommandResponse {
    pub(crate) accepted: bool,
    pub(crate) account_id: String,
    pub(crate) node_id: String,
    pub(crate) dispatch_kind: String,
    pub(crate) prompt_id: String,
    pub(crate) server_time: String,
    pub(crate) client_mutation_id: String,
    pub(crate) ack_seq: i64,
    pub(crate) entity_id: String,
    pub(crate) revision: String,
    pub(crate) idempotent_replay: bool,
    pub(crate) notification_id: String,
    pub(crate) error_code: String,
    pub(crate) reject_reason: String,
    pub(crate) current_state: String,
}

#[derive(Debug)]
pub(crate) enum RealtimeCommandError {
    InvalidArgument(String),
    AlreadyExists(String),
    InFlight(String),
    NotFound(String),
    FailedPrecondition(String),
    MobileSession(MobileSessionError),
    SessionRejected(SessionReject),
    Internal(String),
}

impl From<CommandAckError> for RealtimeCommandError {
    fn from(error: CommandAckError) -> Self {
        match error {
            CommandAckError::AlreadyExists(message) => Self::AlreadyExists(message),
            CommandAckError::InFlight(message) => Self::InFlight(message),
            CommandAckError::Internal(message) => Self::Internal(message),
        }
    }
}

impl RealtimeCommandError {
    pub(crate) fn into_status(self) -> Status {
        match self {
            Self::InvalidArgument(message) => Status::invalid_argument(message),
            Self::AlreadyExists(message) => Status::already_exists(message),
            Self::InFlight(message) => Status::aborted(message),
            Self::NotFound(message) => Status::not_found(message),
            Self::FailedPrecondition(message) => Status::failed_precondition(message),
            Self::MobileSession(error) => mobile_session_status(error),
            Self::SessionRejected(reject) => session_reject_status(reject),
            Self::Internal(message) => Status::internal(message),
        }
    }
}

pub(crate) fn set_session_mode_command(
    control_plane: &ControlPlane,
    thread_id: String,
    preset: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    let preset = normalized_optional_value(&preset);
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, None)?;
    let mode =
        SessionMode::parse_optional(preset).map_err(RealtimeCommandError::SessionRejected)?;
    ensure_session_fsm_allows(
        control_plane,
        &thread_id,
        None,
        SessionCommand::SetMode { mode },
    )?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_SESSION_MODE,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
            "preset": preset,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_session_preset(&thread_id, preset)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_session_mode_changed(control_plane, &thread_id, preset);
            let response_preset = preset.unwrap_or_default().to_owned();
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": thread_id,
                    "preset": response_preset,
                    "serverTime": server_time,
                    "entityId": thread_id,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn send_session_prompt_command(
    control_plane: &ControlPlane,
    thread_id: String,
    prompt: String,
    assistant_surface: String,
    prompt_intent: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    let assistant_surface = normalized_assistant_surface(&assistant_surface)?;
    let prompt_intent =
        prompt_intent_from_str(&prompt_intent).map_err(RealtimeCommandError::MobileSession)?;
    let prompt_intent_value = prompt_intent_wire_value(prompt_intent);
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, assistant_surface)?;
    let fsm_command = match prompt_intent {
        PromptIntent::Queue => SessionCommand::SendPrompt {
            client_mutation_id: client_mutation_id.to_owned(),
        },
        PromptIntent::Steer => SessionCommand::SteerPrompt {
            client_mutation_id: client_mutation_id.to_owned(),
        },
    };
    ensure_session_fsm_allows(control_plane, &thread_id, assistant_surface, fsm_command)?;

    let mut after_ack = None;
    let response = command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SEND_SESSION_PROMPT,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
            "prompt": prompt,
            "assistantSurface": assistant_surface,
            "promptIntent": prompt_intent_value,
        }),
        |server_time| {
            let accepted_delivery = accept_session_prompt(
                control_plane,
                &thread_id,
                assistant_surface,
                &prompt,
                prompt_intent,
            )
            .map_err(RealtimeCommandError::MobileSession)?;
            let dispatch = accepted_delivery.dispatch.clone();
            after_ack = Some(accepted_delivery.after_ack);
            let revision = current_mobile_revision(control_plane)?;
            let (dispatch_kind, prompt_id) = prompt_dispatch_fields(&dispatch);
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "dispatchKind": dispatch_kind,
                    "promptId": prompt_id,
                    "serverTime": server_time,
                    "entityId": thread_id,
                    "revision": revision,
                }),
            ))
        },
    )?;
    if !response.idempotent_replay {
        publish_command_ack_event(control_plane, &thread_id);
        if let Some(after_ack) = after_ack {
            dispatch_session_prompt_after_ack(control_plane.clone(), after_ack);
        }
    }
    Ok(response)
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
        SessionCommand::SubmitNotificationReply {
            client_mutation_id: client_mutation_id.to_owned(),
        },
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
    let response_json = finality_response_json(
        serde_json::json!({
            "dispatchKind": dispatch_kind,
            "promptId": prompt_id,
            "notificationId": notification_id,
        }),
        true,
        &entity_id,
        &revision,
        &server_time,
        "",
        "",
        "",
    );
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

pub(crate) fn set_siri_session_command(
    control_plane: &ControlPlane,
    command_kind: &str,
    thread_id: String,
    assistant_surface: String,
    client_mutation_id: &str,
    target: SiriSessionTarget,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let assistant_surface = normalized_assistant_surface(&assistant_surface)?;
    let normalized_thread_id = normalized_optional_string(&thread_id);
    if let Some(thread_id) = normalized_thread_id.as_deref() {
        ensure_mobile_session_visible_from_minis(control_plane, thread_id, assistant_surface)?;
    }
    let entity_id = normalized_thread_id
        .clone()
        .unwrap_or_else(|| MOBILE_SETTINGS_ENTITY_ID.to_owned());

    command_ack_with_idempotency(
        control_plane,
        command_kind,
        client_mutation_id,
        &entity_id,
        serde_json::json!({
            "threadId": normalized_thread_id,
            "assistantSurface": assistant_surface,
        }),
        |server_time| {
            match target {
                SiriSessionTarget::Current => control_plane
                    .mobile_session_service()
                    .set_siri_current_session(normalized_thread_id.as_deref(), assistant_surface)
                    .map_err(RealtimeCommandError::MobileSession)?,
                SiriSessionTarget::Default => control_plane
                    .mobile_session_service()
                    .set_siri_default_session(normalized_thread_id.as_deref(), assistant_surface)
                    .map_err(RealtimeCommandError::MobileSession)?,
            }
            emit_all_mobile_sessions_changed(control_plane, siri_session_detail(target));
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": normalized_thread_id,
                    "entityId": entity_id,
                    "assistantSurface": assistant_surface,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_assistant_surface_command(
    control_plane: &ControlPlane,
    assistant_surface: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let assistant_surface = normalized_assistant_surface(&assistant_surface)?.ok_or_else(|| {
        RealtimeCommandError::InvalidArgument("assistant surface is required".to_owned())
    })?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_ASSISTANT_SURFACE,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "assistantSurface": assistant_surface,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_assistant_surface(assistant_surface)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "assistant-surface-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "assistantSurface": assistant_surface,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn save_default_prompt_command(
    control_plane: &ControlPlane,
    prompt: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SAVE_DEFAULT_PROMPT,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "prompt": prompt,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .save_default_prompt(&prompt)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "default-prompt-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_scope_command(
    control_plane: &ControlPlane,
    scope: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let scope = normalized_required_string(scope, "scope")?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_SCOPE,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "scope": scope,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_scope(&scope)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "scope-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "scope": scope,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_global_preset_command(
    control_plane: &ControlPlane,
    preset: Option<String>,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let preset = preset.as_deref().and_then(normalized_optional_value);
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_GLOBAL_PRESET,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "preset": preset,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_global_preset(preset)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "global-preset-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "preset": preset,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_global_notification_command(
    control_plane: &ControlPlane,
    notification_id: Option<String>,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let notification_id = notification_id
        .as_deref()
        .and_then(normalized_optional_value);
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_GLOBAL_NOTIFICATION,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "notificationId": notification_id,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_global_notification(notification_id)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "global-notification-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "notificationId": notification_id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_default_notification_targets_command(
    control_plane: &ControlPlane,
    notification_target_ids: Vec<String>,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let notification_target_ids = normalized_string_list(notification_target_ids);
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_DEFAULT_NOTIFICATION_TARGETS,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "notificationTargetIds": notification_target_ids,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_default_notification_targets(&notification_target_ids)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_default_notification_targets_changed(control_plane)?;
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "notificationTargetIds": notification_target_ids,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_global_completion_check_command(
    control_plane: &ControlPlane,
    completion_check_id: Option<String>,
    wait_for_reply_after_completion: bool,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let completion_check_id = completion_check_id
        .as_deref()
        .and_then(normalized_optional_value);
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_GLOBAL_COMPLETION_CHECK,
        client_mutation_id,
        MOBILE_SETTINGS_ENTITY_ID,
        serde_json::json!({
            "completionCheckId": completion_check_id,
            "waitForReplyAfterCompletion": wait_for_reply_after_completion,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_global_completion_check(completion_check_id, wait_for_reply_after_completion)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "global-completion-check-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": MOBILE_SETTINGS_ENTITY_ID,
                    "completionCheckId": completion_check_id,
                    "waitForReplyAfterCompletion": wait_for_reply_after_completion,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn upsert_notification_route_command(
    control_plane: &ControlPlane,
    input: UpsertMobileNotificationRoute,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let entity_id = input
        .id
        .as_deref()
        .and_then(normalized_optional_value)
        .ok_or_else(|| RealtimeCommandError::InvalidArgument("notification id is required".into()))?
        .to_owned();
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_UPSERT_NOTIFICATION_ROUTE,
        client_mutation_id,
        &entity_id,
        serde_json::json!({
            "id": entity_id,
            "label": input.label.clone(),
            "channel": input.channel.clone(),
            "webhookUrl": input.webhook_url.clone(),
            "chatId": input.chat_id.clone(),
            "botToken": input.bot_token.clone(),
            "chatUsername": input.chat_username.clone(),
            "chatDisplayName": input.chat_display_name.clone(),
        }),
        |server_time| {
            let route = control_plane
                .mobile_session_service()
                .upsert_notification_route(input)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "notification-route-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": route.id,
                    "notificationId": route.id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn delete_notification_route_command(
    control_plane: &ControlPlane,
    notification_id: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let notification_id = normalized_required_string(notification_id, "notification_id")?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_DELETE_NOTIFICATION_ROUTE,
        client_mutation_id,
        &notification_id,
        serde_json::json!({
            "notificationId": notification_id,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .delete_notification_route(&notification_id)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "notification-route-deleted");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": notification_id,
                    "notificationId": notification_id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn upsert_completion_check_command(
    control_plane: &ControlPlane,
    completion_check_id: String,
    label: String,
    commands: Vec<String>,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let completion_check_id =
        normalized_required_string(completion_check_id, "completion_check_id")?;
    let label = normalized_required_string(label, "label")?;
    let commands = normalized_string_list(commands);
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_UPSERT_COMPLETION_CHECK,
        client_mutation_id,
        &completion_check_id,
        serde_json::json!({
            "completionCheckId": completion_check_id,
            "label": label,
            "commands": commands,
        }),
        |server_time| {
            let check = control_plane
                .mobile_session_service()
                .upsert_completion_check(&completion_check_id, &label, &commands)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "completion-check-updated");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": check.id,
                    "completionCheckId": check.id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn delete_completion_check_command(
    control_plane: &ControlPlane,
    completion_check_id: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let completion_check_id =
        normalized_required_string(completion_check_id, "completion_check_id")?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_DELETE_COMPLETION_CHECK,
        client_mutation_id,
        &completion_check_id,
        serde_json::json!({
            "completionCheckId": completion_check_id,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .delete_completion_check(&completion_check_id)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "completion-check-deleted");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "entityId": completion_check_id,
                    "completionCheckId": completion_check_id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_session_notifications_command(
    control_plane: &ControlPlane,
    thread_id: String,
    notification_ids: Vec<String>,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    let notification_ids = normalized_string_list(notification_ids);
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, None)?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_SESSION_NOTIFICATIONS,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
            "notificationIds": notification_ids,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_session_notifications(&thread_id, &notification_ids)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_mobile_session_changed(
                control_plane,
                Some(&thread_id),
                Some("notifications-updated"),
            );
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": thread_id,
                    "entityId": thread_id,
                    "notificationIds": notification_ids,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_session_completion_check_command(
    control_plane: &ControlPlane,
    thread_id: String,
    completion_check_id: Option<String>,
    wait_for_reply_after_completion: bool,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    let completion_check_id = completion_check_id
        .as_deref()
        .and_then(normalized_optional_value);
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, None)?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_SESSION_COMPLETION_CHECK,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
            "completionCheckId": completion_check_id,
            "waitForReplyAfterCompletion": wait_for_reply_after_completion,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_session_completion_check(
                    &thread_id,
                    completion_check_id,
                    wait_for_reply_after_completion,
                )
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_mobile_session_changed(
                control_plane,
                Some(&thread_id),
                Some("completion-check-updated"),
            );
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": thread_id,
                    "entityId": thread_id,
                    "completionCheckId": completion_check_id,
                    "waitForReplyAfterCompletion": wait_for_reply_after_completion,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn set_session_archived_command(
    control_plane: &ControlPlane,
    thread_id: String,
    archived: bool,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, None)?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_SET_SESSION_ARCHIVED,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
            "archived": archived,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .set_session_archived(&thread_id, archived)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_mobile_session_changed(
                control_plane,
                Some(&thread_id),
                Some(if archived { "archived" } else { "unarchived" }),
            );
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": thread_id,
                    "entityId": thread_id,
                    "archived": archived,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn delete_session_command(
    control_plane: &ControlPlane,
    thread_id: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, None)?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_DELETE_SESSION,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .delete_session(&thread_id)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_all_mobile_sessions_changed(control_plane, "deleted");
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": thread_id,
                    "entityId": thread_id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
}

pub(crate) fn mute_session_command(
    control_plane: &ControlPlane,
    thread_id: String,
    client_mutation_id: &str,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let thread_id = normalized_required_string(thread_id, "thread_id")?;
    ensure_mobile_session_visible_from_minis(control_plane, &thread_id, None)?;
    command_ack_with_idempotency(
        control_plane,
        COMMAND_KIND_MUTE_SESSION,
        client_mutation_id,
        &thread_id,
        serde_json::json!({
            "threadId": thread_id,
        }),
        |server_time| {
            control_plane
                .mobile_session_service()
                .mute_session(&thread_id)
                .map_err(RealtimeCommandError::MobileSession)?;
            emit_mobile_session_changed(control_plane, Some(&thread_id), Some("muted"));
            let revision = current_mobile_revision(control_plane)?;
            Ok((
                revision.clone(),
                serde_json::json!({
                    "accepted": true,
                    "threadId": thread_id,
                    "entityId": thread_id,
                    "serverTime": server_time,
                    "revision": revision,
                }),
            ))
        },
    )
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

fn normalized_required_string(
    value: String,
    field_name: &str,
) -> Result<String, RealtimeCommandError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(RealtimeCommandError::InvalidArgument(format!(
            "{field_name} is required"
        )));
    }
    Ok(value)
}

fn normalized_optional_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn normalized_optional_value(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn normalized_string_list(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect()
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
        None => Err(RealtimeCommandError::FailedPrecondition(
            "state mini cache is required before sending a session command".to_owned(),
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
        .mobile_session_minis_for_session(thread_id)
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
    if records.is_empty() {
        let has_projection = control_plane
            .store()
            .has_mobile_session_minis()
            .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
        if has_projection {
            return Ok(Some(false));
        }
    }
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
    command: SessionCommand,
) -> Result<(), RealtimeCommandError> {
    let state = current_session_fsm_state(control_plane, thread_id, assistant_surface)?;
    next_session_state(state, command)
        .map(|_| ())
        .map_err(RealtimeCommandError::SessionRejected)
}

fn current_session_fsm_state(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<SessionState, RealtimeCommandError> {
    let minis = control_plane
        .store()
        .mobile_session_minis_for_session(thread_id)
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
    let events = control_plane
        .store()
        .mobile_state_events_for_entity(thread_id)
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?;
    Ok(session_state_for_thread(
        &events,
        &minis,
        thread_id,
        assistant_surface,
    ))
}

fn command_ack_with_idempotency(
    control_plane: &ControlPlane,
    command_kind: &str,
    client_mutation_id: &str,
    entity_id: &str,
    request_payload: serde_json::Value,
    apply: impl FnOnce(&str) -> Result<(String, serde_json::Value), RealtimeCommandError>,
) -> Result<SessionCommandAckResponse, RealtimeCommandError> {
    let client_mutation_id = normalized_required_value(client_mutation_id, "client_mutation_id")?;
    let request_hash = command_request_hash(command_kind, request_payload)?;
    if let Some(record) = existing_command_ack(
        control_plane,
        command_kind,
        client_mutation_id,
        &request_hash,
    )? {
        return Ok(command_ack_response_from_record(&record, true));
    }
    let reservation = reserve_command_ack(
        control_plane,
        command_kind,
        client_mutation_id,
        &request_hash,
    )?;
    if let CommandReservation::Replay(record) = reservation {
        return Ok(command_ack_response_from_record(&record, true));
    }

    let server_time = command_ack_server_time();
    let (revision, response_json) = match apply(&server_time) {
        Ok(result) => result,
        Err(error) => {
            release_command_reservation(
                control_plane,
                command_kind,
                client_mutation_id,
                &request_hash,
            )?;
            return Err(error);
        }
    };
    let response_json = finality_response_json(
        response_json,
        true,
        entity_id,
        &revision,
        &server_time,
        "",
        "",
        "",
    );
    let ack_result = match record_command_ack(
        control_plane,
        command_kind,
        client_mutation_id,
        &request_hash,
        response_json,
        command_ack_state_event(entity_id, &revision, &server_time),
    ) {
        Ok(result) => result,
        Err(error) => {
            release_command_reservation(
                control_plane,
                command_kind,
                client_mutation_id,
                &request_hash,
            )?;
            return Err(error.into());
        }
    };
    Ok(command_ack_response_from_ack_result(&ack_result))
}

pub(crate) fn record_rejected_session_command_ack(
    control_plane: &ControlPlane,
    command_kind: &str,
    client_mutation_id: &str,
    entity_id: &str,
    request_payload: serde_json::Value,
    error_code: &str,
    reject_reason: &str,
    current_state: &str,
) -> Result<Option<SessionCommandAckResponse>, RealtimeCommandError> {
    let Ok(client_mutation_id) =
        normalized_required_value(client_mutation_id, "client_mutation_id")
    else {
        return Ok(None);
    };
    let entity_id = entity_id.trim();
    if entity_id.is_empty() {
        return Ok(None);
    }
    let request_hash = command_request_hash(command_kind, request_payload)?;
    if let Some(record) = existing_command_ack(
        control_plane,
        command_kind,
        client_mutation_id,
        &request_hash,
    )? {
        return Ok(Some(command_ack_response_from_record(&record, true)));
    }
    let reservation = match reserve_command_ack(
        control_plane,
        command_kind,
        client_mutation_id,
        &request_hash,
    ) {
        Ok(reservation) => reservation,
        Err(CommandAckError::AlreadyExists(_) | CommandAckError::InFlight(_)) => return Ok(None),
        Err(CommandAckError::Internal(message)) => {
            return Err(RealtimeCommandError::Internal(message));
        }
    };
    if let CommandReservation::Replay(record) = reservation {
        return Ok(Some(command_ack_response_from_record(&record, true)));
    }

    let server_time = command_ack_server_time();
    let revision = current_mobile_revision(control_plane)?;
    let response_json = finality_response_json(
        serde_json::json!({}),
        false,
        entity_id,
        &revision,
        &server_time,
        error_code,
        reject_reason,
        current_state,
    );
    let ack_result = record_command_ack(
        control_plane,
        command_kind,
        client_mutation_id,
        &request_hash,
        response_json,
        command_ack_state_event(entity_id, &revision, &server_time),
    )?;
    Ok(Some(command_ack_response_from_ack_result(&ack_result)))
}

fn command_ack_response_from_ack_result(
    result: &MobileCommandAckResult,
) -> SessionCommandAckResponse {
    command_ack_response_from_record(
        result.record(),
        matches!(result, MobileCommandAckResult::Duplicate(_)),
    )
}

fn command_ack_response_from_record(
    record: &MobileCommandAckRecord,
    idempotent_replay: bool,
) -> SessionCommandAckResponse {
    let value = ack_response_value(record);
    SessionCommandAckResponse {
        accepted: value
            .get("accepted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        account_id: json_string_or(&value, "accountId", COMMAND_ACK_ACCOUNT_ID),
        node_id: json_string_or(&value, "nodeId", COMMAND_ACK_NODE_ID),
        server_time: json_string(&value, "serverTime"),
        client_mutation_id: record.client_mutation_id.clone(),
        ack_seq: record.ack_seq,
        entity_id: json_string(&value, "entityId"),
        revision: json_string(&value, "revision"),
        idempotent_replay,
        error_code: json_string(&value, "errorCode"),
        reject_reason: json_string(&value, "rejectReason"),
        current_state: json_string(&value, "currentState"),
    }
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
        accepted: value
            .get("accepted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        account_id: json_string_or(&value, "accountId", COMMAND_ACK_ACCOUNT_ID),
        node_id: json_string_or(&value, "nodeId", COMMAND_ACK_NODE_ID),
        dispatch_kind: json_string(&value, "dispatchKind"),
        prompt_id: json_string(&value, "promptId"),
        server_time: json_string(&value, "serverTime"),
        client_mutation_id: record.client_mutation_id.clone(),
        ack_seq: record.ack_seq,
        entity_id: json_string(&value, "entityId"),
        revision: json_string(&value, "revision"),
        idempotent_replay,
        notification_id: notification_id.to_owned(),
        error_code: json_string(&value, "errorCode"),
        reject_reason: json_string(&value, "rejectReason"),
        current_state: json_string(&value, "currentState"),
    }
}

fn finality_response_json(
    mut response_json: serde_json::Value,
    accepted: bool,
    entity_id: &str,
    revision: &str,
    server_time: &str,
    error_code: &str,
    reject_reason: &str,
    current_state: &str,
) -> serde_json::Value {
    if !response_json.is_object() {
        response_json = serde_json::json!({});
    }
    let object = response_json
        .as_object_mut()
        .expect("finality response json object");
    object.insert("accepted".to_owned(), serde_json::json!(accepted));
    object.insert(
        "accountId".to_owned(),
        serde_json::json!(COMMAND_ACK_ACCOUNT_ID),
    );
    object.insert("nodeId".to_owned(), serde_json::json!(COMMAND_ACK_NODE_ID));
    object.insert("entityId".to_owned(), serde_json::json!(entity_id));
    object.insert("revision".to_owned(), serde_json::json!(revision));
    object.insert("serverTime".to_owned(), serde_json::json!(server_time));
    object.insert("errorCode".to_owned(), serde_json::json!(error_code));
    object.insert("rejectReason".to_owned(), serde_json::json!(reject_reason));
    object.insert("currentState".to_owned(), serde_json::json!(current_state));
    response_json
}

fn json_string_or(value: &serde_json::Value, key: &str, fallback: &str) -> String {
    let value = json_string(value, key);
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value
    }
}

fn emit_all_mobile_sessions_changed(control_plane: &ControlPlane, detail: &str) {
    control_plane.emit_mobile_all_sessions_event(MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: None,
        prompt_id: None,
        detail: Some(detail.to_owned()),
    });
}

fn emit_default_notification_targets_changed(
    control_plane: &ControlPlane,
) -> Result<(), RealtimeCommandError> {
    // Reconciling (rather than forcing) is safe now that the reconcile's stored-projection
    // comparison is content-aware: a default-notification-targets change always changes the
    // `notificationStatus` field embedded in each session mini, so the content check below
    // will correctly detect it and write, even when the desktop snapshot's `revision` string
    // does not change. `force` used to be required here to bypass a key-set-only comparison
    // that could not see that in-place change.
    if control_plane
        .reconcile_mobile_session_mini_projection_with_detail(
            "default-notification-targets-updated",
        )
        .map_err(|error| RealtimeCommandError::Internal(error.to_string()))?
    {
        return Ok(());
    }

    emit_all_mobile_sessions_changed(control_plane, "default-notification-targets-updated");
    Ok(())
}

fn emit_mobile_session_changed(
    control_plane: &ControlPlane,
    thread_id: Option<&str>,
    detail: Option<&str>,
) {
    if let Some(thread_id) = thread_id {
        invalidate_delivery_action_cache(control_plane, thread_id);
    }
    let input = MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: thread_id.map(str::to_owned),
        prompt_id: None,
        detail: detail.map(str::to_owned),
    };
    match thread_id {
        Some(thread_id) => {
            emit_mobile_session_event_with_fresh_cached_overlay(control_plane, input, thread_id)
        }
        None => control_plane.emit_mobile_event(input),
    }
}

fn emit_mobile_session_event_with_fresh_cached_overlay(
    control_plane: &ControlPlane,
    input: MobileEventInput,
    thread_id: &str,
) {
    let minis = control_plane
        .store()
        .mobile_session_minis_for_session(thread_id)
        .ok()
        .and_then(|records| {
            control_plane
                .mobile_session_service()
                .state()
                .ok()
                .map(|session_state| {
                    session_mini_projection_inputs_with_mobile_state(
                        &records,
                        thread_id,
                        &session_state,
                    )
                })
        })
        .unwrap_or_default();
    if minis.is_empty() {
        control_plane.emit_mobile_session_event_without_projection(input);
        control_plane.spawn_mobile_session_mini_projection_reconcile_if_due();
    } else {
        control_plane.emit_mobile_session_event_with_cached_minis(input, minis);
    }
}

fn emit_session_mode_changed(control_plane: &ControlPlane, thread_id: &str, preset: Option<&str>) {
    invalidate_delivery_action_cache(control_plane, thread_id);
    let lifecycle_event = MobileEventInput {
        kind: MobileEventKind::LifecycleChanged,
        thread_id: Some(thread_id.to_owned()),
        prompt_id: None,
        detail: Some(preset.unwrap_or(MODE_CLEARED_DETAIL).to_owned()),
    };
    let minis = control_plane
        .store()
        .mobile_session_minis_for_session(thread_id)
        .ok()
        .map(|records| session_mini_projection_inputs_with_mode(&records, thread_id, preset))
        .unwrap_or_default();
    if minis.is_empty() {
        control_plane.emit_mobile_session_event_without_projection(lifecycle_event);
    } else {
        control_plane.emit_mobile_session_event_with_cached_minis(lifecycle_event, minis);
    }
    control_plane.emit_mobile_session_event_without_projection(MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: Some(thread_id.to_owned()),
        prompt_id: None,
        detail: Some(MODE_UPDATED_DETAIL.to_owned()),
    });
    // Re-warm the delivery-action cache we just invalidated: the periodic reconciler
    // only fires on codex source changes, and a mode change is not one, so without
    // this the very next prompt for this thread would hit a cold cache.
    control_plane.spawn_mobile_session_mini_projection_reconcile_if_due();
}

fn siri_session_detail(target: SiriSessionTarget) -> &'static str {
    match target {
        SiriSessionTarget::Current => "siri-current-session-updated",
        SiriSessionTarget::Default => "siri-default-session-updated",
    }
}

fn prompt_intent_wire_value(prompt_intent: PromptIntent) -> &'static str {
    match prompt_intent {
        PromptIntent::Queue => "queue",
        PromptIntent::Steer => "steer",
    }
}

fn session_reject_status(reject: SessionReject) -> Status {
    match reject.code {
        SessionRejectCode::InvalidMode => Status::invalid_argument(reject.status_message()),
        SessionRejectCode::ModeRequired
        | SessionRejectCode::SessionBusy
        | SessionRejectCode::IllegalTransition => {
            Status::failed_precondition(reject.status_message())
        }
    }
}

fn mobile_session_status(error: MobileSessionError) -> Status {
    match error {
        MobileSessionError::SessionNotFound => Status::not_found(error.to_string()),
        MobileSessionError::PromptRequired
        | MobileSessionError::InvalidPromptIntent
        | MobileSessionError::InvalidPreset
        | MobileSessionError::InvalidScope
        | MobileSessionError::InvalidAssistantSurface
        | MobileSessionError::InvalidNotificationChannel
        | MobileSessionError::MissingNotificationConfig
        | MobileSessionError::InvalidCompletionCheck => Status::invalid_argument(error.to_string()),
        MobileSessionError::ModeRequired => Status::failed_precondition(error.to_string()),
        MobileSessionError::SessionArchived
        | MobileSessionError::PromptDeliveryUnavailable
        | MobileSessionError::PromptDeliveryUnavailableReason(_)
        | MobileSessionError::PromptResumeUnavailable(_)
        | MobileSessionError::PromptSnapshotUnavailable(_) => {
            Status::failed_precondition(error.to_string())
        }
        MobileSessionError::NotificationNotFound | MobileSessionError::CompletionCheckNotFound => {
            Status::not_found(error.to_string())
        }
        MobileSessionError::Store(_)
        | MobileSessionError::Filesystem(_)
        | MobileSessionError::TimeFormat(_) => Status::internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use crate::control_plane::{ControlPlane, ControlPlaneConfig, HostEnvironment};
    use crate::events::{MobileSessionMiniProjectionInput, MobileStateEventInput};
    use crate::mobile::events::MobileEventKind;
    use crate::mobile::prompt_delivery::DETAIL_PROMPT_DELIVERY_FAILED;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn realtime_gate_allows_retry_after_prompt_delivery_failure() {
        let temp_dir = TempDir::new().expect("temp dir");
        let control_plane = test_control_plane(&temp_dir);
        let thread_id = "thread-realtime-delivery-failure";
        seed_active_session_mini(&control_plane, thread_id);

        record_prompt_accepted(&control_plane, thread_id, "cmid-first");
        assert!(matches!(
            current_session_fsm_state(&control_plane, thread_id, Some("codex"))
                .expect("current state after accepted prompt"),
            SessionState::Dispatched { .. }
        ));
        let busy = ensure_session_fsm_allows(
            &control_plane,
            thread_id,
            Some("codex"),
            SessionCommand::SendPrompt {
                client_mutation_id: "cmid-before-failure".to_owned(),
            },
        )
        .expect_err("accepted prompt should keep the gate busy before delivery settles");
        assert!(matches!(
            busy,
            RealtimeCommandError::SessionRejected(SessionReject {
                code: SessionRejectCode::SessionBusy,
                ..
            })
        ));

        record_prompt_delivery_failed(&control_plane, thread_id);
        assert_eq!(
            current_session_fsm_state(&control_plane, thread_id, Some("codex"))
                .expect("current state after failed delivery"),
            SessionState::ModeArmed {
                mode: SessionMode::Infinite
            }
        );
        ensure_session_fsm_allows(
            &control_plane,
            thread_id,
            Some("codex"),
            SessionCommand::SendPrompt {
                client_mutation_id: "cmid-retry".to_owned(),
            },
        )
        .expect("delivery failure should allow a retry");
    }

    fn seed_active_session_mini(control_plane: &ControlPlane, thread_id: &str) {
        control_plane
            .store()
            .upsert_mobile_session_mini(
                MobileSessionMiniProjectionInput {
                    session_id: thread_id.to_owned(),
                    assistant_surface: "codex".to_owned(),
                    body_json: serde_json::json!({
                        "sessionId": thread_id,
                        "assistantSurface": "codex",
                        "effectiveMode": "infinite",
                        "lifecycle": "active",
                        "status": "active",
                        "canSendPrompt": true,
                    }),
                },
                0,
                "rev-0",
            )
            .expect("seed mini");
    }

    fn record_prompt_accepted(
        control_plane: &ControlPlane,
        thread_id: &str,
        client_mutation_id: &str,
    ) {
        control_plane
            .store()
            .record_mobile_state_event(MobileStateEventInput {
                entity_id: thread_id.to_owned(),
                kind: MobileEventKind::SessionChanged,
                revision: "rev-prompt".to_owned(),
                server_time: "now".to_owned(),
                payload_json: serde_json::json!({
                    "threadId": thread_id,
                    "detail": "command-ack",
                }),
                client_mutation_id: Some(client_mutation_id.to_owned()),
                command_kind: Some(COMMAND_KIND_SEND_SESSION_PROMPT.to_owned()),
                command_request_hash: Some(format!("hash-{client_mutation_id}")),
                command_response_json: Some(serde_json::json!({
                    "threadId": thread_id,
                    "dispatchKind": "accepted",
                    "entityId": thread_id,
                    "revision": "rev-prompt",
                    "serverTime": "now",
                })),
            })
            .expect("record accepted prompt");
    }

    fn record_prompt_delivery_failed(control_plane: &ControlPlane, thread_id: &str) {
        control_plane
            .store()
            .record_mobile_state_event(MobileStateEventInput {
                entity_id: thread_id.to_owned(),
                kind: MobileEventKind::SessionChanged,
                revision: "rev-failed".to_owned(),
                server_time: "now".to_owned(),
                payload_json: serde_json::json!({
                    "threadId": thread_id,
                    "detail": DETAIL_PROMPT_DELIVERY_FAILED,
                }),
                client_mutation_id: None,
                command_kind: None,
                command_request_hash: None,
                command_response_json: None,
            })
            .expect("record failed delivery");
    }

    fn test_control_plane(temp_dir: &TempDir) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: temp_dir.path().join(".codex"),
            codex_executable: None,
            store_path: temp_dir.path().join("control-plane.sqlite"),
            hook_command: None,
            host_environment: HostEnvironment::hermetic(temp_dir.path().to_path_buf()),
        })
    }
}

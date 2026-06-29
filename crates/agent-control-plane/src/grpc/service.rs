use std::{pin::Pin, time::Duration};

use async_stream::stream;
use futures_core::Stream;
use tonic::{Request, Response, Status};

use crate::control_plane::ControlPlane;
use crate::control_plane::session_fsm::SessionReject;
use crate::events::{MobileSessionMiniRecord, MobileStateEventGap, MobileStateEventRecord};
use crate::grpc::auth::authorize_mobile_api_request_from_peer;
use crate::grpc::frame_limits::{
    SESSION_STATE_DELTA_REPLACEMENT_CHUNK_MAX_BYTES, ensure_client_frame_size,
    ensure_command_text_list_size, ensure_command_text_size, ensure_server_frame_size,
    truncate_control_text,
};
use crate::grpc::proto;
use crate::grpc::proto::looper_realtime_server::LooperRealtime;
use crate::mobile::api::compact_mobile_session_mini_record;
use crate::mobile::events::{
    MobileEvent, MobileEventBroadcast, MobileEventKind, MobileEventRecord, mobile_event_now,
    mobile_event_wire_name,
};
use crate::mobile::realtime_ack::{COMMAND_ACK_ACCOUNT_ID, COMMAND_ACK_NODE_ID, CommandAckError};
use crate::mobile::realtime_commands::{
    COMMAND_KIND_SEND_SESSION_PROMPT, COMMAND_KIND_SET_SESSION_MODE,
    COMMAND_KIND_SET_SIRI_CURRENT_SESSION, COMMAND_KIND_SET_SIRI_DEFAULT_SESSION,
    RealtimeCommandError, SessionCommandAckResponse, SiriSessionTarget,
    SubmitNotificationReplyInput, delete_completion_check_command,
    delete_notification_route_command, delete_session_command, mute_session_command,
    record_rejected_session_command_ack, save_default_prompt_command, send_session_prompt_command,
    set_default_notification_targets_command, set_global_completion_check_command,
    set_global_notification_command, set_global_preset_command, set_scope_command,
    set_session_archived_command, set_session_completion_check_command, set_session_mode_command,
    set_session_notifications_command, set_siri_session_command, submit_notification_reply_command,
    upsert_completion_check_command, upsert_notification_route_command,
};
use crate::mobile::session::UpsertMobileNotificationRoute;

const HEALTH_SERVICE_NAME: &str = "looper-realtime";
const MOBILE_SETTINGS_ENTITY_ID: &str = "mobile-settings";
const SESSION_REPLAY_BATCH_SIZE: usize = 128;
const SESSION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const REJECT_ERROR_CODE_EMPTY_FRAME: &str = "empty_client_frame";
const REJECT_ERROR_CODE_EMPTY_COMMAND: &str = "empty_command";
const COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY: &str = "SubmitNotificationReply";
const STATE_DELTA_NO_PROJECTION_REASON: &str = "projection-missing";
const STATE_DELTA_PROJECTION_READ_FAILED_REASON: &str = "projection-read-failed";
const STATE_DELTA_FRAME_CAP_EXCEEDED_REASON: &str = "projection-frame-cap-exceeded";
const STATE_DELTA_RECOVERY_INSTRUCTION: &str = "session-mini-snapshot";

type SessionFrameStream =
    Pin<Box<dyn Stream<Item = Result<proto::ServerFrame, Status>> + Send + 'static>>;

impl From<CommandAckError> for Status {
    fn from(error: CommandAckError) -> Self {
        match error {
            CommandAckError::AlreadyExists(message) => Status::already_exists(message),
            CommandAckError::InFlight(message) => Status::aborted(message),
            CommandAckError::Internal(message) => Status::internal(message),
        }
    }
}

#[derive(Clone)]
pub struct LooperRealtimeService {
    control_plane: ControlPlane,
}

impl LooperRealtimeService {
    pub fn new(control_plane: ControlPlane) -> Self {
        Self { control_plane }
    }
}

#[tonic::async_trait]
impl LooperRealtime for LooperRealtimeService {
    type SessionStream = SessionFrameStream;

    async fn health(
        &self,
        _request: Request<proto::HealthRequest>,
    ) -> Result<Response<proto::HealthResponse>, Status> {
        let status = self.control_plane.status();
        Ok(Response::new(proto::HealthResponse {
            ok: status.source.health == "healthy",
            service: HEALTH_SERVICE_NAME.to_owned(),
            server_time: mobile_event_now(),
        }))
    }

    async fn session(
        &self,
        request: Request<tonic::Streaming<proto::ClientFrame>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        authorize_mobile_api_request_from_peer(
            &self.control_plane,
            request.metadata(),
            request.remote_addr(),
        )?;
        let mut inbound = request.into_inner();
        let control_plane = self.control_plane.clone();
        let output = stream! {
            let mut last_seq = latest_mobile_state_seq(&control_plane);
            let mut event_receiver = control_plane.mobile_event_hub().subscribe();
            let mut heartbeat = tokio::time::interval_at(
                tokio::time::Instant::now() + SESSION_HEARTBEAT_INTERVAL,
                SESSION_HEARTBEAT_INTERVAL,
            );
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            'session: loop {
                macro_rules! yield_frame {
                    ($frame:expr) => {{
                        match checked_server_frame($frame) {
                            Ok(frame) => yield Ok(frame),
                            Err(status) => {
                                yield Err(status);
                                break 'session;
                            }
                        }
                    }};
                }

                macro_rules! yield_frames {
                    ($frames:expr) => {{
                        for frame in $frames {
                            yield_frame!(frame);
                        }
                    }};
                }

                tokio::select! {
                    biased;

                    received = inbound.message() => {
                        match received {
                            Ok(Some(frame)) => {
                                if let Err(status) = ensure_client_frame_size(&frame) {
                                    yield Err(status);
                                    break;
                                }
                                let batch = handle_session_client_frame(&control_plane, frame, &mut last_seq);
                                yield_frames!(batch.frames);
                                if let Some(status) = batch.terminal_error {
                                    yield Err(status);
                                    break;
                                }
                            }
                            Ok(None) => {
                                match drain_state_delta_frames(&control_plane, &mut last_seq) {
                                    Ok(frames) => {
                                        yield_frames!(frames);
                                    }
                                    Err(status) => {
                                        yield Err(status);
                                    }
                                }
                                break;
                            },
                            Err(status) => {
                                yield Err(status);
                                break;
                            }
                        }
                    }
                    event = event_receiver.recv() => {
                        match event {
                            Ok(MobileEventBroadcast::Persisted(record)) => {
                                yield_frame!(mobile_event_record_frame(&control_plane, &record));
                                match drain_state_delta_frames(&control_plane, &mut last_seq) {
                                    Ok(frames) => {
                                        yield_frames!(frames);
                                    }
                                    Err(status) => {
                                        yield Err(status);
                                        break;
                                    }
                                }
                            }
                            Ok(MobileEventBroadcast::Ephemeral(event)) => {
                                yield_frame!(mobile_event_frame(proto_mobile_event_from_event(&event)));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                match drain_state_delta_frames(&control_plane, &mut last_seq) {
                                    Ok(frames) => {
                                        yield_frames!(frames);
                                    }
                                    Err(status) => {
                                        yield Err(status);
                                        break;
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    _ = heartbeat.tick() => {
                        yield_frame!(heartbeat_frame(&control_plane));
                    }
                }
            }
        };
        Ok(Response::new(Box::pin(output)))
    }
}

fn handle_session_client_frame(
    control_plane: &ControlPlane,
    frame: proto::ClientFrame,
    last_seq: &mut i64,
) -> SessionFrameBatch {
    match frame.frame {
        Some(proto::client_frame::Frame::Command(command)) => {
            handle_session_command(control_plane, command, last_seq)
        }
        Some(proto::client_frame::Frame::Resume(resume)) => {
            match replay_state_delta_frames(control_plane, resume.after_seq, last_seq) {
                Ok(frames) => SessionFrameBatch::frames(frames),
                Err(status) => SessionFrameBatch::terminal_error(status),
            }
        }
        None => SessionFrameBatch::frames(vec![command_ack_frame(rejected_command_ack(
            String::new(),
            String::new(),
            REJECT_ERROR_CODE_EMPTY_FRAME,
            "client frame is empty",
            "",
        ))]),
    }
}

fn handle_session_command(
    control_plane: &ControlPlane,
    command: proto::Command,
    last_seq: &mut i64,
) -> SessionFrameBatch {
    match command.command {
        Some(proto::command::Command::SetSessionMode(request)) => {
            let request_payload = serde_json::json!({
                "threadId": request.thread_id.clone(),
                "preset": optional_proto_string(request.preset.clone()),
            });
            session_command_frames_with_request(
                control_plane,
                last_seq,
                COMMAND_KIND_SET_SESSION_MODE,
                request.client_mutation_id.clone(),
                request.thread_id.clone(),
                request_payload,
                set_session_mode_command(
                    control_plane,
                    request.thread_id,
                    request.preset,
                    &request.client_mutation_id,
                )
                .map(command_ack_from_command)
                .map_err(realtime_command_status),
            )
        }
        Some(proto::command::Command::SendSessionPrompt(request)) => {
            let request_payload = serde_json::json!({
                "threadId": request.thread_id.clone(),
                "prompt": request.prompt.clone(),
                "assistantSurface": optional_proto_string(request.assistant_surface.clone()),
                "promptIntent": prompt_intent_request_value(&request.prompt_intent),
            });
            let result = ensure_command_text_size("prompt", &request.prompt).and_then(|_| {
                send_session_prompt_command(
                    control_plane,
                    request.thread_id.clone(),
                    request.prompt,
                    request.assistant_surface,
                    request.prompt_intent,
                    &request.client_mutation_id,
                )
                .map(command_ack_from_command)
                .map_err(realtime_command_status)
            });
            session_command_frames_with_request(
                control_plane,
                last_seq,
                COMMAND_KIND_SEND_SESSION_PROMPT,
                request.client_mutation_id,
                request.thread_id,
                request_payload,
                result,
            )
        }
        Some(proto::command::Command::SubmitNotificationReply(request)) => {
            let request_payload = serde_json::json!({
                "notificationId": request.notification_id.clone(),
                "threadId": request.thread_id.clone(),
                "prompt": request.prompt.clone(),
                "assistantSurface": optional_proto_string(request.assistant_surface.clone()),
            });
            session_command_frames_with_request(
                control_plane,
                last_seq,
                COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY,
                request.client_mutation_id.clone(),
                request.thread_id.clone(),
                request_payload,
                submit_notification_reply_session_command(
                    control_plane,
                    &request.notification_id,
                    &request.thread_id,
                    &request.prompt,
                    Some(&request.assistant_surface),
                    &request.client_mutation_id,
                ),
            )
        }
        Some(proto::command::Command::SetSiriCurrentSession(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            set_siri_session_command(
                control_plane,
                COMMAND_KIND_SET_SIRI_CURRENT_SESSION,
                request.thread_id,
                request.assistant_surface,
                &request.client_mutation_id,
                SiriSessionTarget::Current,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetSiriDefaultSession(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            set_siri_session_command(
                control_plane,
                COMMAND_KIND_SET_SIRI_DEFAULT_SESSION,
                request.thread_id,
                request.assistant_surface,
                &request.client_mutation_id,
                SiriSessionTarget::Default,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SaveDefaultPrompt(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            save_default_prompt_command(control_plane, request.prompt, &request.client_mutation_id)
                .map(command_ack_from_command)
                .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetSessionArchived(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            set_session_archived_command(
                control_plane,
                request.thread_id,
                request.archived,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::DeleteSession(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            delete_session_command(
                control_plane,
                request.thread_id,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::MuteSession(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            mute_session_command(
                control_plane,
                request.thread_id,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetScope(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            set_scope_command(control_plane, request.scope, &request.client_mutation_id)
                .map(command_ack_from_command)
                .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetGlobalPreset(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            set_global_preset_command(
                control_plane,
                Some(request.preset),
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetGlobalNotification(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            set_global_notification_command(
                control_plane,
                Some(request.notification_id),
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetDefaultNotificationTargets(request)) => {
            session_command_frames(
                control_plane,
                last_seq,
                request.client_mutation_id.clone(),
                MOBILE_SETTINGS_ENTITY_ID.to_owned(),
                set_default_notification_targets_command(
                    control_plane,
                    request.notification_target_ids,
                    &request.client_mutation_id,
                )
                .map(command_ack_from_command)
                .map_err(realtime_command_status),
            )
        }
        Some(proto::command::Command::SetGlobalCompletionCheck(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            set_global_completion_check_command(
                control_plane,
                Some(request.completion_check_id),
                request.wait_for_reply_after_completion,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::UpsertNotificationRoute(request)) => {
            let result =
                ensure_command_text_size("notification label", &request.label).and_then(|_| {
                    upsert_notification_route_command(
                        control_plane,
                        UpsertMobileNotificationRoute {
                            id: Some(request.notification_id.clone()),
                            label: Some(request.label),
                            channel: request.channel,
                            webhook_url: optional_proto_string(request.webhook_url),
                            chat_id: optional_proto_string(request.chat_id),
                            bot_token: optional_proto_string(request.bot_token),
                            chat_username: optional_proto_string(request.chat_username),
                            chat_display_name: optional_proto_string(request.chat_display_name),
                        },
                        &request.client_mutation_id,
                    )
                    .map(command_ack_from_command)
                    .map_err(realtime_command_status)
                });
            session_command_frames(
                control_plane,
                last_seq,
                request.client_mutation_id,
                request.notification_id,
                result,
            )
        }
        Some(proto::command::Command::DeleteNotificationRoute(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.notification_id.clone(),
            delete_notification_route_command(
                control_plane,
                request.notification_id,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::UpsertCompletionCheck(request)) => {
            let result = ensure_command_text_size("completion check label", &request.label)
                .and_then(|_| {
                    ensure_command_text_list_size("completion check command", &request.commands)
                })
                .and_then(|_| {
                    upsert_completion_check_command(
                        control_plane,
                        request.completion_check_id.clone(),
                        request.label,
                        request.commands,
                        &request.client_mutation_id,
                    )
                    .map(command_ack_from_command)
                    .map_err(realtime_command_status)
                });
            session_command_frames(
                control_plane,
                last_seq,
                request.client_mutation_id,
                request.completion_check_id,
                result,
            )
        }
        Some(proto::command::Command::DeleteCompletionCheck(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.completion_check_id.clone(),
            delete_completion_check_command(
                control_plane,
                request.completion_check_id,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetSessionNotifications(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            set_session_notifications_command(
                control_plane,
                request.thread_id,
                request.notification_ids,
                &request.client_mutation_id,
            )
            .map(command_ack_from_command)
            .map_err(realtime_command_status),
        ),
        Some(proto::command::Command::SetSessionCompletionCheck(request)) => {
            session_command_frames(
                control_plane,
                last_seq,
                request.client_mutation_id.clone(),
                request.thread_id.clone(),
                set_session_completion_check_command(
                    control_plane,
                    request.thread_id,
                    Some(request.completion_check_id),
                    request.wait_for_reply_after_completion,
                    &request.client_mutation_id,
                )
                .map(command_ack_from_command)
                .map_err(realtime_command_status),
            )
        }
        None => SessionFrameBatch::frames(vec![command_ack_frame(rejected_command_ack(
            String::new(),
            String::new(),
            REJECT_ERROR_CODE_EMPTY_COMMAND,
            "command frame is empty",
            "",
        ))]),
    }
}

struct SessionFrameBatch {
    frames: Vec<proto::ServerFrame>,
    terminal_error: Option<Status>,
}

impl SessionFrameBatch {
    fn frames(frames: Vec<proto::ServerFrame>) -> Self {
        Self {
            frames,
            terminal_error: None,
        }
    }

    fn terminal_error(status: Status) -> Self {
        Self {
            frames: Vec::new(),
            terminal_error: Some(status),
        }
    }

    fn frames_then_error(frames: Vec<proto::ServerFrame>, status: Status) -> Self {
        Self {
            frames,
            terminal_error: Some(status),
        }
    }
}

fn session_command_frames(
    control_plane: &ControlPlane,
    last_seq: &mut i64,
    client_mutation_id: String,
    entity_id: String,
    result: Result<proto::CommandAck, Status>,
) -> SessionFrameBatch {
    session_command_frames_with_request(
        control_plane,
        last_seq,
        "",
        client_mutation_id,
        entity_id,
        serde_json::json!({}),
        result,
    )
}

fn session_command_frames_with_request(
    control_plane: &ControlPlane,
    last_seq: &mut i64,
    command_kind: &str,
    client_mutation_id: String,
    entity_id: String,
    request_payload: serde_json::Value,
    result: Result<proto::CommandAck, Status>,
) -> SessionFrameBatch {
    match result {
        Ok(ack) => {
            let mut frames = vec![command_ack_frame(ack)];
            match drain_state_delta_frames(control_plane, last_seq) {
                Ok(deltas) => {
                    frames.extend(deltas);
                    SessionFrameBatch::frames(frames)
                }
                Err(status) => SessionFrameBatch::frames_then_error(frames, status),
            }
        }
        Err(status) => {
            let error_code = command_reject_code(&status);
            let reject_reason = command_reject_reason(&status);
            let current_state = command_reject_current_state(&status);
            let ack = if should_record_rejected_command_ack(command_kind, &status) {
                record_rejected_session_command_ack(
                    control_plane,
                    command_kind,
                    &client_mutation_id,
                    &entity_id,
                    request_payload,
                    error_code,
                    &reject_reason,
                    &current_state,
                )
                .ok()
                .flatten()
                .map(command_ack_from_command)
            } else {
                None
            }
            .unwrap_or_else(|| {
                rejected_command_ack(
                    client_mutation_id,
                    entity_id,
                    error_code,
                    &reject_reason,
                    &current_state,
                )
            });
            SessionFrameBatch::frames(vec![command_ack_frame(ack)])
        }
    }
}

fn replay_state_delta_frames(
    control_plane: &ControlPlane,
    after_seq: i64,
    last_seq: &mut i64,
) -> Result<Vec<proto::ServerFrame>, Status> {
    let mut replay_after_seq = control_plane
        .store()
        .latest_mobile_session_mini_replacement_event_seq_after(after_seq)
        .map_err(state_replay_status)?
        .map(|replacement_seq| replacement_seq.saturating_sub(1))
        .unwrap_or(after_seq);
    let mut frames = Vec::new();

    loop {
        let records = control_plane
            .store()
            .mobile_state_events_after_seq(replay_after_seq, SESSION_REPLAY_BATCH_SIZE)
            .map_err(state_replay_status)?;
        if records.is_empty() {
            break;
        }

        let record_count = records.len();
        for record in records {
            replay_after_seq = replay_after_seq.max(record.seq);
            *last_seq = (*last_seq).max(record.seq);
            frames.extend(state_delta_frames(control_plane, &record)?);
        }
        if record_count < SESSION_REPLAY_BATCH_SIZE {
            break;
        }
    }

    *last_seq = (*last_seq).max(after_seq);
    Ok(frames)
}

fn drain_state_delta_frames(
    control_plane: &ControlPlane,
    last_seq: &mut i64,
) -> Result<Vec<proto::ServerFrame>, Status> {
    replay_state_delta_frames(control_plane, *last_seq, last_seq)
}

fn state_replay_status(error: anyhow::Error) -> Status {
    if let Some(gap) = error.downcast_ref::<MobileStateEventGap>() {
        return Status::out_of_range(format!(
            "seq_gap: requested after_seq {} but latest_seq is {}",
            gap.requested_after_seq, gap.latest_seq
        ));
    }
    Status::internal(error.to_string())
}

fn command_ack_frame(ack: proto::CommandAck) -> proto::ServerFrame {
    proto::ServerFrame {
        frame: Some(proto::server_frame::Frame::Ack(ack)),
    }
}

fn checked_server_frame(frame: proto::ServerFrame) -> Result<proto::ServerFrame, Status> {
    ensure_server_frame_size(&frame)?;
    Ok(frame)
}

fn rejected_command_ack(
    client_mutation_id: String,
    entity_id: String,
    error_code: &str,
    reject_reason: &str,
    current_state: &str,
) -> proto::CommandAck {
    proto::CommandAck {
        accepted: false,
        client_mutation_id,
        ack_seq: 0,
        entity_id,
        revision: String::new(),
        server_time: mobile_event_now(),
        idempotent_replay: false,
        error_code: error_code.to_owned(),
        reject_reason: truncate_control_text(reject_reason),
        account_id: COMMAND_ACK_ACCOUNT_ID.to_owned(),
        node_id: COMMAND_ACK_NODE_ID.to_owned(),
        current_state: current_state.to_owned(),
    }
}

fn state_delta_frames(
    control_plane: &ControlPlane,
    record: &MobileStateEventRecord,
) -> Result<Vec<proto::ServerFrame>, Status> {
    let replacement = match control_plane
        .store()
        .mobile_session_minis_replaced_at_seq(record.seq)
    {
        Ok(replacement) => replacement,
        Err(_) => {
            return state_delta_frame(
                record,
                state_delta_control_payload_json(record, STATE_DELTA_PROJECTION_READ_FAILED_REASON),
            )
            .map(|frame| vec![frame]);
        }
    };
    let Ok(minis) = control_plane
        .store()
        .mobile_session_minis_at_seq(record.seq)
    else {
        return state_delta_frame(
            record,
            state_delta_control_payload_json(record, STATE_DELTA_PROJECTION_READ_FAILED_REASON),
        )
        .map(|frame| vec![frame]);
    };
    if replacement {
        return replacement_state_delta_frames(record, &minis);
    }

    let payload_json = if minis.len() == 1 && minis[0].session_id == record.entity_id {
        compact_mobile_session_mini_record(&minis[0]).unwrap_or_else(|| {
            state_delta_control_payload_json(record, STATE_DELTA_NO_PROJECTION_REASON)
        })
    } else {
        state_delta_control_payload_json(record, STATE_DELTA_NO_PROJECTION_REASON)
    };
    state_delta_frame(record, payload_json).map(|frame| vec![frame])
}

fn state_delta_frame(
    record: &MobileStateEventRecord,
    payload_json: String,
) -> Result<proto::ServerFrame, Status> {
    let frame = proto::ServerFrame {
        frame: Some(proto::server_frame::Frame::StateDelta(
            proto::StateMiniDelta {
                seq: record.seq,
                entity_id: record.entity_id.clone(),
                kind: proto_event_name(record.kind).to_owned(),
                revision: record.revision.clone(),
                server_time: record.server_time.clone(),
                payload_json,
            },
        )),
    };
    checked_server_frame(frame)
}

fn replacement_state_delta_frames(
    record: &MobileStateEventRecord,
    minis: &[MobileSessionMiniRecord],
) -> Result<Vec<proto::ServerFrame>, Status> {
    let payloads = minis
        .iter()
        .filter_map(compact_mobile_session_mini_record)
        .collect::<Vec<_>>();
    if replacement_has_oversized_single_mini(record.seq, &payloads) {
        return state_delta_frame(
            record,
            state_delta_recovery_payload_json(record, STATE_DELTA_FRAME_CAP_EXCEEDED_REASON),
        )
        .map(|frame| vec![frame]);
    }

    let mut frames = Vec::new();
    let mut chunk = Vec::new();
    let mut chunk_bytes = replacement_state_delta_payload_overhead(record.seq, true);
    let mut chunk_replaces = true;

    for payload in payloads {
        let candidate_bytes = chunk_bytes + payload.len() + usize::from(!chunk.is_empty());
        if candidate_bytes > SESSION_STATE_DELTA_REPLACEMENT_CHUNK_MAX_BYTES && !chunk.is_empty() {
            let payload_json =
                replacement_state_delta_payload_json(record.seq, &chunk, chunk_replaces);
            frames.push(state_delta_frame(record, payload_json)?);
            chunk.clear();
            chunk_replaces = false;
            chunk_bytes = replacement_state_delta_payload_overhead(record.seq, chunk_replaces);
        }
        chunk_bytes += payload.len() + usize::from(!chunk.is_empty());
        chunk.push(payload);
    }

    if frames.is_empty() || !chunk.is_empty() {
        let payload_json = replacement_state_delta_payload_json(record.seq, &chunk, chunk_replaces);
        frames.push(state_delta_frame(record, payload_json)?);
    }

    Ok(frames)
}

fn replacement_has_oversized_single_mini(latest_seq: i64, payloads: &[String]) -> bool {
    let max_single_payload_bytes = SESSION_STATE_DELTA_REPLACEMENT_CHUNK_MAX_BYTES
        .saturating_sub(replacement_state_delta_payload_overhead(latest_seq, true));
    payloads
        .iter()
        .any(|payload| payload.len() > max_single_payload_bytes)
}

fn replacement_state_delta_payload_json(
    latest_seq: i64,
    sessions: &[String],
    replace: bool,
) -> String {
    let mut payload = format!(
        "{{\"latest_seq\":{latest_seq},\"latestSeq\":{latest_seq},\"replace\":{replace},\"sessions\":["
    );
    payload.push_str(&sessions.join(","));
    payload.push_str("]}");
    payload
}

fn replacement_state_delta_payload_overhead(latest_seq: i64, replace: bool) -> usize {
    replacement_state_delta_payload_json(latest_seq, &[], replace).len()
}

fn state_delta_control_payload_json(record: &MobileStateEventRecord, reason: &str) -> String {
    serde_json::json!({
        "controlOnly": true,
        "reason": reason,
        "entityId": record.entity_id,
        "kind": proto_event_name(record.kind),
        "latestSeq": record.seq,
    })
    .to_string()
}

fn state_delta_recovery_payload_json(record: &MobileStateEventRecord, reason: &str) -> String {
    serde_json::json!({
        "controlOnly": true,
        "reason": reason,
        "entityId": record.entity_id,
        "kind": proto_event_name(record.kind),
        "latestSeq": record.seq,
        "recoveryRequired": true,
        "recovery": STATE_DELTA_RECOVERY_INSTRUCTION,
    })
    .to_string()
}

fn mobile_event_record_frame(
    control_plane: &ControlPlane,
    record: &MobileEventRecord,
) -> proto::ServerFrame {
    mobile_event_frame(proto::MobileEvent {
        kind: proto_event_kind(record.event_type),
        event_name: proto_event_name(record.event_type).to_owned(),
        thread_id: record.thread_id.clone().unwrap_or_default(),
        prompt_id: record.prompt_id.clone().unwrap_or_default(),
        detail: truncate_control_text(&record.detail.clone().unwrap_or_default()),
        server_time: mobile_event_now(),
        revision: mobile_event_frame_revision(control_plane),
    })
}

fn mobile_event_frame_revision(control_plane: &ControlPlane) -> String {
    control_plane
        .store()
        .latest_mobile_session_mini_revision()
        .ok()
        .flatten()
        .unwrap_or_else(|| {
            format!(
                "mobile-state:seq-{}",
                latest_mobile_state_seq(control_plane)
            )
        })
}

fn mobile_event_frame(event: proto::MobileEvent) -> proto::ServerFrame {
    proto::ServerFrame {
        frame: Some(proto::server_frame::Frame::Event(event)),
    }
}

fn proto_mobile_event_from_event(event: &MobileEvent) -> proto::MobileEvent {
    proto::MobileEvent {
        kind: proto_event_kind(event.event_type),
        event_name: proto_event_name(event.event_type).to_owned(),
        thread_id: event.thread_id.clone().unwrap_or_default(),
        prompt_id: event.prompt_id.clone().unwrap_or_default(),
        detail: truncate_control_text(&event.detail.clone().unwrap_or_default()),
        server_time: event.server_time.clone(),
        revision: event.revision.clone().unwrap_or_default(),
    }
}

fn optional_proto_string(value: String) -> Option<String> {
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn prompt_intent_request_value(value: &str) -> &str {
    match value.trim() {
        "" => "queue",
        value => value,
    }
}

fn heartbeat_frame(control_plane: &ControlPlane) -> proto::ServerFrame {
    proto::ServerFrame {
        frame: Some(proto::server_frame::Frame::Heartbeat(proto::Heartbeat {
            server_time: mobile_event_now(),
            latest_seq: latest_mobile_state_seq(control_plane),
        })),
    }
}

fn latest_mobile_state_seq(control_plane: &ControlPlane) -> i64 {
    control_plane
        .store()
        .latest_mobile_state_event_seq()
        .unwrap_or_default()
}

fn proto_event_kind(kind: MobileEventKind) -> i32 {
    match kind {
        MobileEventKind::SessionChanged => 1,
        MobileEventKind::PromptQueued => 2,
        MobileEventKind::PromptDelivered => 3,
        MobileEventKind::LifecycleChanged => 4,
    }
}

fn proto_event_name(kind: MobileEventKind) -> &'static str {
    mobile_event_wire_name(kind)
}

fn status_code_name(code: tonic::Code) -> &'static str {
    match code {
        tonic::Code::Ok => "ok",
        tonic::Code::Cancelled => "cancelled",
        tonic::Code::Unknown => "unknown",
        tonic::Code::InvalidArgument => "invalid_argument",
        tonic::Code::DeadlineExceeded => "deadline_exceeded",
        tonic::Code::NotFound => "not_found",
        tonic::Code::AlreadyExists => "already_exists",
        tonic::Code::PermissionDenied => "permission_denied",
        tonic::Code::ResourceExhausted => "resource_exhausted",
        tonic::Code::FailedPrecondition => "failed_precondition",
        tonic::Code::Aborted => "aborted",
        tonic::Code::OutOfRange => "out_of_range",
        tonic::Code::Unimplemented => "unimplemented",
        tonic::Code::Internal => "internal",
        tonic::Code::Unavailable => "unavailable",
        tonic::Code::DataLoss => "data_loss",
        tonic::Code::Unauthenticated => "unauthenticated",
    }
}

fn command_reject_code(status: &Status) -> &'static str {
    SessionReject::from_status_message(status.message())
        .map(|reject| reject.code_str())
        .unwrap_or_else(|| status_code_name(status.code()))
}

fn command_reject_reason(status: &Status) -> String {
    SessionReject::from_status_message(status.message())
        .map(|reject| reject.wire_reason())
        .unwrap_or_else(|| status.message().to_owned())
}

fn command_reject_current_state(status: &Status) -> String {
    SessionReject::from_status_message(status.message())
        .map(|reject| reject.current_state.label().to_owned())
        .unwrap_or_default()
}

fn should_record_rejected_command_ack(command_kind: &str, status: &Status) -> bool {
    !command_kind.is_empty() && SessionReject::from_status_message(status.message()).is_some()
}

fn command_ack_from_command(response: SessionCommandAckResponse) -> proto::CommandAck {
    proto::CommandAck {
        accepted: response.accepted,
        client_mutation_id: response.client_mutation_id,
        ack_seq: response.ack_seq,
        entity_id: response.entity_id,
        revision: response.revision,
        server_time: response.server_time,
        idempotent_replay: response.idempotent_replay,
        error_code: response.error_code,
        reject_reason: truncate_control_text(&response.reject_reason),
        account_id: response.account_id,
        node_id: response.node_id,
        current_state: response.current_state,
    }
}

fn submit_notification_reply_session_command(
    control_plane: &ControlPlane,
    notification_id: &str,
    thread_id: &str,
    prompt: &str,
    assistant_surface: Option<&str>,
    client_mutation_id: &str,
) -> Result<proto::CommandAck, Status> {
    ensure_command_text_size("prompt", prompt)?;
    submit_notification_reply_command(
        control_plane,
        SubmitNotificationReplyInput {
            notification_id,
            thread_id,
            prompt,
            assistant_surface,
            client_mutation_id,
        },
    )
    .map_err(realtime_command_status)
    .map(|response| {
        command_ack_from_command(SessionCommandAckResponse {
            accepted: response.accepted,
            account_id: response.account_id,
            node_id: response.node_id,
            server_time: response.server_time,
            client_mutation_id: response.client_mutation_id,
            ack_seq: response.ack_seq,
            entity_id: response.entity_id,
            revision: response.revision,
            idempotent_replay: response.idempotent_replay,
            error_code: response.error_code,
            reject_reason: response.reject_reason,
            current_state: response.current_state,
        })
    })
}

fn realtime_command_status(error: RealtimeCommandError) -> Status {
    error.into_status()
}

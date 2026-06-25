use std::{pin::Pin, time::Duration};

use async_stream::stream;
use futures_core::Stream;
use tonic::{Request, Response, Status};

use crate::control_plane::ControlPlane;
use crate::events::{
    MobileCommandAckRecord, MobileCommandAckResult, MobileStateEventGap, MobileStateEventRecord,
};
use crate::grpc::auth::authorize_mobile_api_request;
use crate::grpc::events::{MobileEventStream, mobile_events};
use crate::grpc::proto;
use crate::grpc::proto::looper_realtime_server::LooperRealtime;
use crate::mobile::api::{
    mobile_session_detail, session_mini_projection_inputs_with_mode,
    session_mini_records_contain_session,
};
use crate::mobile::events::{
    MobileEvent, MobileEventBroadcast, MobileEventInput, MobileEventKind, MobileEventRecord,
    mobile_event_now, mobile_event_sse_name,
};
use crate::mobile::prompt_delivery::{
    accept_session_prompt, dispatch_session_prompt_after_ack, invalidate_delivery_action_cache,
    mobile_desktop_snapshot, prompt_dispatch_fields,
};
use crate::mobile::realtime_ack::{
    CommandAckError, CommandReservation, ack_response_value, command_ack_server_time,
    command_ack_state_event, command_request_hash, current_mobile_revision, existing_command_ack,
    json_string, publish_command_ack_event, record_command_ack, release_command_reservation,
    reserve_command_ack,
};
use crate::mobile::realtime_commands::{
    NotificationReplyCommandResponse, RealtimeCommandError, SubmitNotificationReplyInput,
    submit_notification_reply_command,
};
use crate::mobile::session::{ASSISTANT_SURFACES, MobileSessionError};

const HEALTH_SERVICE_NAME: &str = "looper-realtime";
const MODE_CLEARED_DETAIL: &str = "mode-cleared";
const MODE_UPDATED_DETAIL: &str = "mode-updated";
const COMMAND_KIND_SET_SESSION_MODE: &str = "SetSessionMode";
const COMMAND_KIND_SEND_SESSION_PROMPT: &str = "SendSessionPrompt";
const SESSION_REPLAY_BATCH_SIZE: usize = 128;
const SESSION_STATE_POLL_INTERVAL: Duration = Duration::from_millis(250);
const SESSION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const REJECT_ERROR_CODE_EMPTY_FRAME: &str = "empty_client_frame";
const REJECT_ERROR_CODE_EMPTY_COMMAND: &str = "empty_command";

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
    type SubscribeMobileEventsStream = MobileEventStream;
    type SubscribeDesktopEventsStream = MobileEventStream;

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

    async fn set_session_mode(
        &self,
        request: Request<proto::SetSessionModeRequest>,
    ) -> Result<Response<proto::SetSessionModeResponse>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        let request = request.into_inner();

        Ok(Response::new(set_session_mode_command(
            &self.control_plane,
            request.thread_id,
            request.preset,
            &request.client_mutation_id,
            SessionVisibilityPolicy::AllowSnapshotFallback,
        )?))
    }

    async fn send_session_prompt(
        &self,
        request: Request<proto::SendSessionPromptRequest>,
    ) -> Result<Response<proto::SendSessionPromptResponse>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        let request = request.into_inner();

        Ok(Response::new(send_session_prompt_command(
            &self.control_plane,
            request.thread_id,
            request.prompt,
            request.assistant_surface,
            &request.client_mutation_id,
        )?))
    }

    async fn submit_notification_reply(
        &self,
        request: Request<proto::SubmitNotificationReplyRequest>,
    ) -> Result<Response<proto::SubmitNotificationReplyResponse>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        let request = request.into_inner();
        let response = submit_notification_reply_command(
            &self.control_plane,
            SubmitNotificationReplyInput {
                notification_id: &request.notification_id,
                thread_id: &request.thread_id,
                prompt: &request.prompt,
                assistant_surface: Some(&request.assistant_surface),
                client_mutation_id: &request.client_mutation_id,
            },
        )
        .map_err(realtime_command_status)?;

        Ok(Response::new(notification_reply_response_from_command(
            response,
        )))
    }

    async fn session(
        &self,
        request: Request<tonic::Streaming<proto::ClientFrame>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        let mut inbound = request.into_inner();
        let control_plane = self.control_plane.clone();
        let output = stream! {
            let mut last_seq = latest_mobile_state_seq(&control_plane);
            let mut event_receiver = control_plane.mobile_event_hub().subscribe();
            let mut state_poll = tokio::time::interval_at(
                tokio::time::Instant::now() + SESSION_STATE_POLL_INTERVAL,
                SESSION_STATE_POLL_INTERVAL,
            );
            state_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut heartbeat = tokio::time::interval_at(
                tokio::time::Instant::now() + SESSION_HEARTBEAT_INTERVAL,
                SESSION_HEARTBEAT_INTERVAL,
            );
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    biased;

                    received = inbound.message() => {
                        match received {
                            Ok(Some(frame)) => {
                                let batch = handle_session_client_frame(&control_plane, frame, &mut last_seq);
                                for frame in batch.frames {
                                    yield Ok(frame);
                                }
                                if let Some(status) = batch.terminal_error {
                                    yield Err(status);
                                    break;
                                }
                            }
                            Ok(None) => {
                                match drain_state_delta_frames(&control_plane, &mut last_seq) {
                                    Ok(frames) => {
                                        for frame in frames {
                                            yield Ok(frame);
                                        }
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
                                yield Ok(mobile_event_record_frame(&control_plane, &record));
                                match drain_state_delta_frames(&control_plane, &mut last_seq) {
                                    Ok(frames) => {
                                        for frame in frames {
                                            yield Ok(frame);
                                        }
                                    }
                                    Err(status) => {
                                        yield Err(status);
                                        break;
                                    }
                                }
                            }
                            Ok(MobileEventBroadcast::Ephemeral(event)) => {
                                yield Ok(mobile_event_frame(proto_mobile_event_from_event(&event)));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                match drain_state_delta_frames(&control_plane, &mut last_seq) {
                                    Ok(frames) => {
                                        for frame in frames {
                                            yield Ok(frame);
                                        }
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
                    _ = state_poll.tick() => {
                        match drain_state_delta_frames(&control_plane, &mut last_seq) {
                            Ok(frames) => {
                                for frame in frames {
                                    yield Ok(frame);
                                }
                            }
                            Err(status) => {
                                yield Err(status);
                                break;
                            }
                        }
                    }
                    _ = heartbeat.tick() => {
                        yield Ok(heartbeat_frame(&control_plane));
                    }
                }
            }
        };
        Ok(Response::new(Box::pin(output)))
    }

    async fn subscribe_mobile_events(
        &self,
        request: Request<proto::SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeMobileEventsStream>, Status> {
        authorize_mobile_api_request(&self.control_plane, request.metadata())?;
        Ok(Response::new(mobile_events(self.control_plane.clone())))
    }

    async fn subscribe_desktop_events(
        &self,
        _request: Request<proto::SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeDesktopEventsStream>, Status> {
        Ok(Response::new(mobile_events(self.control_plane.clone())))
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
        ))]),
    }
}

fn handle_session_command(
    control_plane: &ControlPlane,
    command: proto::Command,
    last_seq: &mut i64,
) -> SessionFrameBatch {
    match command.command {
        Some(proto::command::Command::SetSessionMode(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            set_session_mode_command(
                control_plane,
                request.thread_id,
                request.preset,
                &request.client_mutation_id,
                SessionVisibilityPolicy::RequireStateMiniCache,
            )
            .and_then(command_ack_from_mode_response),
        ),
        Some(proto::command::Command::SendSessionPrompt(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            send_session_prompt_command(
                control_plane,
                request.thread_id,
                request.prompt,
                request.assistant_surface,
                &request.client_mutation_id,
            )
            .and_then(command_ack_from_prompt_response),
        ),
        Some(proto::command::Command::SubmitNotificationReply(request)) => session_command_frames(
            control_plane,
            last_seq,
            request.client_mutation_id.clone(),
            request.thread_id.clone(),
            submit_notification_reply_command(
                control_plane,
                SubmitNotificationReplyInput {
                    notification_id: &request.notification_id,
                    thread_id: &request.thread_id,
                    prompt: &request.prompt,
                    assistant_surface: Some(&request.assistant_surface),
                    client_mutation_id: &request.client_mutation_id,
                },
            )
            .map(notification_reply_response_from_command)
            .map_err(realtime_command_status)
            .and_then(command_ack_from_notification_reply_response),
        ),
        None => SessionFrameBatch::frames(vec![command_ack_frame(rejected_command_ack(
            String::new(),
            String::new(),
            REJECT_ERROR_CODE_EMPTY_COMMAND,
            "command frame is empty",
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
        Err(status) => SessionFrameBatch::frames(vec![command_ack_frame(rejected_command_ack(
            client_mutation_id,
            entity_id,
            status_code_name(status.code()),
            status.message(),
        ))]),
    }
}

fn command_ack_from_mode_response(
    response: proto::SetSessionModeResponse,
) -> Result<proto::CommandAck, Status> {
    response
        .ack
        .ok_or_else(|| Status::internal("set-session-mode response missing command ACK"))
}

fn command_ack_from_prompt_response(
    response: proto::SendSessionPromptResponse,
) -> Result<proto::CommandAck, Status> {
    response
        .ack
        .ok_or_else(|| Status::internal("send-session-prompt response missing command ACK"))
}

fn command_ack_from_notification_reply_response(
    response: proto::SubmitNotificationReplyResponse,
) -> Result<proto::CommandAck, Status> {
    response
        .ack
        .ok_or_else(|| Status::internal("notification-reply response missing command ACK"))
}

fn replay_state_delta_frames(
    control_plane: &ControlPlane,
    after_seq: i64,
    last_seq: &mut i64,
) -> Result<Vec<proto::ServerFrame>, Status> {
    let records = control_plane
        .store()
        .mobile_state_events_after_seq(after_seq, SESSION_REPLAY_BATCH_SIZE)
        .map_err(state_replay_status)?;
    let mut frames = Vec::with_capacity(records.len());
    for record in records {
        *last_seq = (*last_seq).max(record.seq);
        frames.push(state_delta_frame(&record));
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

fn rejected_command_ack(
    client_mutation_id: String,
    entity_id: String,
    error_code: &str,
    reject_reason: &str,
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
        reject_reason: reject_reason.to_owned(),
    }
}

fn state_delta_frame(record: &MobileStateEventRecord) -> proto::ServerFrame {
    proto::ServerFrame {
        frame: Some(proto::server_frame::Frame::StateDelta(
            proto::StateMiniDelta {
                seq: record.seq,
                entity_id: record.entity_id.clone(),
                kind: proto_event_name(record.kind).to_owned(),
                revision: record.revision.clone(),
                server_time: record.server_time.clone(),
                payload_json: record.payload_json.clone(),
            },
        )),
    }
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
        detail: record.detail.clone().unwrap_or_default(),
        server_time: mobile_event_now(),
        revision: control_plane.mobile_snapshot_revision().unwrap_or_default(),
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
        detail: event.detail.clone().unwrap_or_default(),
        server_time: event.server_time.clone(),
        revision: event.revision.clone().unwrap_or_default(),
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
    mobile_event_sse_name(kind)
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

#[derive(Clone, Copy)]
enum SessionVisibilityPolicy {
    AllowSnapshotFallback,
    RequireStateMiniCache,
}

fn set_session_mode_command(
    control_plane: &ControlPlane,
    thread_id: String,
    preset: String,
    client_mutation_id: &str,
    visibility_policy: SessionVisibilityPolicy,
) -> Result<proto::SetSessionModeResponse, Status> {
    let client_mutation_id = required_client_mutation_id(client_mutation_id)?;
    let request_hash = command_request_hash(
        COMMAND_KIND_SET_SESSION_MODE,
        serde_json::json!({
            "threadId": thread_id,
            "preset": preset,
        }),
    )?;
    if let Some(record) = existing_command_ack(
        control_plane,
        COMMAND_KIND_SET_SESSION_MODE,
        client_mutation_id,
        &request_hash,
    )? {
        return Ok(mode_response_from_record(&record, true));
    }
    let reservation = reserve_command_ack(
        control_plane,
        COMMAND_KIND_SET_SESSION_MODE,
        client_mutation_id,
        &request_hash,
    )?;
    if let CommandReservation::Replay(record) = reservation {
        return Ok(mode_response_from_record(&record, true));
    }

    if let Err(error) = ensure_mobile_session_visible_with_policy(
        control_plane,
        &thread_id,
        None,
        visibility_policy,
    ) {
        release_command_reservation(
            control_plane,
            COMMAND_KIND_SET_SESSION_MODE,
            client_mutation_id,
            &request_hash,
        )?;
        return Err(error);
    }
    let preset = normalized_optional_value(&preset);
    if let Err(error) = control_plane
        .mobile_session_service()
        .set_session_preset(&thread_id, preset)
        .map_err(mobile_session_status)
    {
        release_command_reservation(
            control_plane,
            COMMAND_KIND_SET_SESSION_MODE,
            client_mutation_id,
            &request_hash,
        )?;
        return Err(error);
    }
    emit_session_mode_changed(control_plane, &thread_id, preset);
    let response_preset = preset.unwrap_or_default().to_owned();
    let server_time = command_ack_server_time();
    let revision = current_mobile_revision_or_snapshot(control_plane)?;
    let entity_id = thread_id.clone();
    let response_json = serde_json::json!({
        "accepted": true,
        "threadId": thread_id,
        "preset": response_preset,
        "serverTime": server_time,
        "entityId": entity_id,
        "revision": revision,
    });
    let ack_result = record_command_ack(
        control_plane,
        COMMAND_KIND_SET_SESSION_MODE,
        client_mutation_id,
        &request_hash,
        response_json,
        command_ack_state_event(&entity_id, &revision, &server_time),
    )?;

    Ok(mode_response_from_ack_result(&ack_result))
}

fn send_session_prompt_command(
    control_plane: &ControlPlane,
    thread_id: String,
    prompt: String,
    assistant_surface: String,
    client_mutation_id: &str,
) -> Result<proto::SendSessionPromptResponse, Status> {
    let client_mutation_id = required_client_mutation_id(client_mutation_id)?;
    let assistant_surface = normalized_assistant_surface(&assistant_surface)?;
    let request_hash = command_request_hash(
        COMMAND_KIND_SEND_SESSION_PROMPT,
        serde_json::json!({
            "threadId": thread_id,
            "prompt": prompt,
            "assistantSurface": assistant_surface,
        }),
    )?;
    if let Some(record) = existing_command_ack(
        control_plane,
        COMMAND_KIND_SEND_SESSION_PROMPT,
        client_mutation_id,
        &request_hash,
    )? {
        return Ok(prompt_response_from_record(&record, true));
    }
    let reservation = reserve_command_ack(
        control_plane,
        COMMAND_KIND_SEND_SESSION_PROMPT,
        client_mutation_id,
        &request_hash,
    )?;
    if let CommandReservation::Replay(record) = reservation {
        return Ok(prompt_response_from_record(&record, true));
    }

    if let Err(error) =
        ensure_mobile_session_visible_from_minis(control_plane, &thread_id, assistant_surface)
    {
        release_command_reservation(
            control_plane,
            COMMAND_KIND_SEND_SESSION_PROMPT,
            client_mutation_id,
            &request_hash,
        )?;
        return Err(error);
    }
    let accepted_delivery =
        match accept_session_prompt(control_plane, &thread_id, assistant_surface, &prompt) {
            Ok(delivery) => delivery,
            Err(error) => {
                release_command_reservation(
                    control_plane,
                    COMMAND_KIND_SEND_SESSION_PROMPT,
                    client_mutation_id,
                    &request_hash,
                )?;
                return Err(mobile_session_status(error));
            }
        };
    let dispatch = accepted_delivery.dispatch.clone();
    let after_ack = accepted_delivery.after_ack;
    let server_time = command_ack_server_time();
    let revision = current_mobile_revision(control_plane)?;
    let entity_id = thread_id.clone();
    let (dispatch_kind, prompt_id) = prompt_dispatch_fields(&dispatch);
    let response_json = serde_json::json!({
        "accepted": true,
        "dispatchKind": dispatch_kind,
        "promptId": prompt_id,
        "serverTime": server_time,
        "entityId": entity_id,
        "revision": revision,
    });
    let ack_result = match record_command_ack(
        control_plane,
        COMMAND_KIND_SEND_SESSION_PROMPT,
        client_mutation_id,
        &request_hash,
        response_json,
        command_ack_state_event(&entity_id, &revision, &server_time),
    ) {
        Ok(result) => result,
        Err(error) => {
            release_command_reservation(
                control_plane,
                COMMAND_KIND_SEND_SESSION_PROMPT,
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

    Ok(prompt_response_from_ack_result(&ack_result))
}

fn normalized_optional_value(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn normalized_required_value<'a>(value: &'a str, field_name: &str) -> Result<&'a str, Status> {
    let value = value.trim();
    if value.is_empty() {
        return Err(Status::invalid_argument(format!(
            "{field_name} is required"
        )));
    }
    Ok(value)
}

fn required_client_mutation_id(value: &str) -> Result<&str, Status> {
    normalized_required_value(value, "client_mutation_id")
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
        .mobile_session_minis()
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
}

fn normalized_assistant_surface(value: &str) -> Result<Option<&str>, Status> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if ASSISTANT_SURFACES.contains(&value) {
        return Ok(Some(value));
    }
    Err(Status::invalid_argument("invalid assistant surface"))
}

fn ensure_mobile_session_visible(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<(), Status> {
    if let Some(visible) = session_mini_visibility(control_plane, thread_id, assistant_surface)? {
        if visible {
            return Ok(());
        }
        return Err(Status::not_found("session not found"));
    }

    let snapshot = mobile_desktop_snapshot(control_plane)
        .map_err(|error| Status::internal(error.to_string()))?;
    let session_state = control_plane
        .mobile_session_service()
        .state()
        .map_err(mobile_session_status)?;
    if mobile_session_detail(&snapshot, &session_state, thread_id, assistant_surface).is_some() {
        return Ok(());
    }
    Err(Status::not_found("session not found"))
}

fn ensure_mobile_session_visible_from_minis(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<(), Status> {
    match session_mini_visibility(control_plane, thread_id, assistant_surface)? {
        Some(true) => Ok(()),
        Some(false) => Err(Status::not_found("session not found")),
        None => Err(Status::failed_precondition(
            "state mini cache is required before sending a session command",
        )),
    }
}

fn ensure_mobile_session_visible_with_policy(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    policy: SessionVisibilityPolicy,
) -> Result<(), Status> {
    match policy {
        SessionVisibilityPolicy::AllowSnapshotFallback => {
            ensure_mobile_session_visible(control_plane, thread_id, assistant_surface)
        }
        SessionVisibilityPolicy::RequireStateMiniCache => {
            ensure_mobile_session_visible_from_minis(control_plane, thread_id, assistant_surface)
        }
    }
}

fn session_mini_visibility(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<Option<bool>, Status> {
    let records = control_plane
        .store()
        .mobile_session_minis()
        .map_err(|error| Status::internal(error.to_string()))?;
    Ok(session_mini_records_contain_session(
        &records,
        thread_id,
        assistant_surface,
    ))
}

fn current_mobile_revision_or_snapshot(control_plane: &ControlPlane) -> Result<String, Status> {
    match current_mobile_revision(control_plane) {
        Ok(revision) => Ok(revision),
        Err(CommandAckError::Internal(_)) => control_plane
            .mobile_snapshot_revision()
            .map_err(|error| Status::internal(error.to_string())),
        Err(error) => Err(error.into()),
    }
}

fn command_ack_from_record(
    record: &MobileCommandAckRecord,
    value: &serde_json::Value,
    idempotent_replay: bool,
) -> proto::CommandAck {
    proto::CommandAck {
        accepted: true,
        client_mutation_id: record.client_mutation_id.clone(),
        ack_seq: record.ack_seq,
        entity_id: json_string(value, "entityId"),
        revision: json_string(value, "revision"),
        server_time: json_string(value, "serverTime"),
        idempotent_replay,
        error_code: String::new(),
        reject_reason: String::new(),
    }
}

fn mode_response_from_ack_result(result: &MobileCommandAckResult) -> proto::SetSessionModeResponse {
    mode_response_from_record(
        result.record(),
        matches!(result, MobileCommandAckResult::Duplicate(_)),
    )
}

fn mode_response_from_record(
    record: &MobileCommandAckRecord,
    idempotent_replay: bool,
) -> proto::SetSessionModeResponse {
    let value = ack_response_value(record);
    let ack = command_ack_from_record(record, &value, idempotent_replay);
    proto::SetSessionModeResponse {
        accepted: ack.accepted,
        thread_id: json_string(&value, "threadId"),
        preset: json_string(&value, "preset"),
        server_time: ack.server_time.clone(),
        client_mutation_id: ack.client_mutation_id.clone(),
        ack_seq: ack.ack_seq,
        entity_id: ack.entity_id.clone(),
        revision: ack.revision.clone(),
        idempotent_replay,
        ack: Some(ack),
    }
}

fn prompt_response_from_ack_result(
    result: &MobileCommandAckResult,
) -> proto::SendSessionPromptResponse {
    prompt_response_from_record(
        result.record(),
        matches!(result, MobileCommandAckResult::Duplicate(_)),
    )
}

fn prompt_response_from_record(
    record: &MobileCommandAckRecord,
    idempotent_replay: bool,
) -> proto::SendSessionPromptResponse {
    let value = ack_response_value(record);
    let ack = command_ack_from_record(record, &value, idempotent_replay);
    proto::SendSessionPromptResponse {
        accepted: ack.accepted,
        dispatch_kind: json_string(&value, "dispatchKind"),
        prompt_id: json_string(&value, "promptId"),
        server_time: ack.server_time.clone(),
        client_mutation_id: ack.client_mutation_id.clone(),
        ack_seq: ack.ack_seq,
        entity_id: ack.entity_id.clone(),
        revision: ack.revision.clone(),
        idempotent_replay,
        ack: Some(ack),
    }
}

fn notification_reply_response_from_command(
    response: NotificationReplyCommandResponse,
) -> proto::SubmitNotificationReplyResponse {
    let ack = proto::CommandAck {
        accepted: response.accepted,
        client_mutation_id: response.client_mutation_id.clone(),
        ack_seq: response.ack_seq,
        entity_id: response.entity_id.clone(),
        revision: response.revision.clone(),
        server_time: response.server_time.clone(),
        idempotent_replay: response.idempotent_replay,
        error_code: String::new(),
        reject_reason: String::new(),
    };
    proto::SubmitNotificationReplyResponse {
        accepted: response.accepted,
        dispatch_kind: response.dispatch_kind,
        prompt_id: response.prompt_id,
        server_time: response.server_time,
        client_mutation_id: response.client_mutation_id,
        ack_seq: response.ack_seq,
        entity_id: response.entity_id,
        revision: response.revision,
        idempotent_replay: response.idempotent_replay,
        ack: Some(ack),
        notification_id: response.notification_id,
    }
}

fn realtime_command_status(error: RealtimeCommandError) -> Status {
    match error {
        RealtimeCommandError::InvalidArgument(message) => Status::invalid_argument(message),
        RealtimeCommandError::AlreadyExists(message) => Status::already_exists(message),
        RealtimeCommandError::NotFound(message) => Status::not_found(message),
        RealtimeCommandError::MobileSession(error) => mobile_session_status(error),
        RealtimeCommandError::Internal(message) => Status::internal(message),
    }
}

fn mobile_session_status(error: MobileSessionError) -> Status {
    match error {
        MobileSessionError::SessionNotFound => Status::not_found(error.to_string()),
        MobileSessionError::PromptRequired
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

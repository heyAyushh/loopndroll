use std::{collections::HashSet, time::Duration};

use tokio_stream::iter;
use tonic::{Request, metadata::MetadataValue, transport::Endpoint};

use crate::{
    command_batch::build_command_batch_response,
    error::ClientCoreError,
    model::{
        ClientCommandAck, ClientCommandBatchResponse, ClientCommandKind, ClientCommandMetadata,
        ClientEndpoint, OutboundSessionFrame, OutboundSessionFrameKind,
    },
};

pub(crate) mod proto {
    tonic::include_proto!("looper.v1");
}

const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const AUTHORIZATION_HEADER: &str = "authorization";
const MOBILE_SESSION_HEADER: &str = "x-looper-mobile-session";
const BEARER_PREFIX: &str = "Bearer ";

pub(crate) async fn submit_expected_session_outbox(
    endpoints: Vec<ClientEndpoint>,
    bearer_token: String,
    mobile_session_header: String,
    frames: Vec<OutboundSessionFrame>,
) -> Result<ClientCommandBatchResponse, ClientCoreError> {
    let endpoint = select_transport_endpoint(&endpoints)?;
    let command_metadata = frames
        .iter()
        .map(command_metadata)
        .collect::<Result<Vec<_>, _>>()?;
    let expected_mutation_ids = command_metadata
        .iter()
        .map(|command| command.client_mutation_id.as_str())
        .collect::<HashSet<_>>();

    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(endpoint)
        .await
        .map_err(|_| ClientCoreError::SessionCommandTransportFailed)?;
    let client_frames = frames
        .into_iter()
        .map(client_frame)
        .collect::<Result<Vec<_>, _>>()?;
    let mut request = Request::new(iter(client_frames));
    apply_metadata(request.metadata_mut(), bearer_token, mobile_session_header)?;

    let response = client
        .session(request)
        .await
        .map_err(|_| ClientCoreError::SessionCommandTransportFailed)?;
    let mut stream = response.into_inner();
    let acks = tokio::time::timeout(COMMAND_ACK_TIMEOUT, async {
        let mut acks = Vec::with_capacity(expected_mutation_ids.len());
        while let Some(frame) = stream
            .message()
            .await
            .map_err(|_| ClientCoreError::SessionCommandTransportFailed)?
        {
            let Some(proto::server_frame::Frame::Ack(ack)) = frame.frame else {
                continue;
            };
            if expected_mutation_ids.contains(ack.client_mutation_id.as_str())
                && !acks.iter().any(|seen: &ClientCommandAck| {
                    seen.client_mutation_id == ack.client_mutation_id
                })
            {
                acks.push(client_command_ack(ack));
            }
            if acks.len() == expected_mutation_ids.len() {
                return Ok(acks);
            }
        }
        Ok(acks)
    })
    .await
    .map_err(|_| ClientCoreError::SessionCommandAckTimedOut)??;

    build_command_batch_response(command_metadata, acks)
}

fn select_transport_endpoint(endpoints: &[ClientEndpoint]) -> Result<Endpoint, ClientCoreError> {
    let endpoint = endpoints
        .iter()
        .find(|endpoint| endpoint.last_good)
        .or_else(|| endpoints.first())
        .ok_or(ClientCoreError::NoEndpoint)?;
    Endpoint::from_shared(endpoint.url.clone()).map_err(|_| ClientCoreError::InvalidEndpoint)
}

fn apply_metadata(
    metadata: &mut tonic::metadata::MetadataMap,
    bearer_token: String,
    mobile_session_header: String,
) -> Result<(), ClientCoreError> {
    if !bearer_token.trim().is_empty() {
        let value = format!("{BEARER_PREFIX}{bearer_token}");
        metadata.insert(
            AUTHORIZATION_HEADER,
            MetadataValue::try_from(value)
                .map_err(|_| ClientCoreError::SessionCommandTransportFailed)?,
        );
    }

    if !mobile_session_header.trim().is_empty() {
        metadata.insert(
            MOBILE_SESSION_HEADER,
            MetadataValue::try_from(mobile_session_header)
                .map_err(|_| ClientCoreError::SessionCommandTransportFailed)?,
        );
    }

    Ok(())
}

fn client_frame(frame: OutboundSessionFrame) -> Result<proto::ClientFrame, ClientCoreError> {
    match frame.frame_kind {
        OutboundSessionFrameKind::Command => Ok(proto::ClientFrame {
            frame: Some(proto::client_frame::Frame::Command(proto::Command {
                command: Some(command(frame)?),
            })),
        }),
        OutboundSessionFrameKind::Resume => Ok(proto::ClientFrame {
            frame: Some(proto::client_frame::Frame::Resume(proto::Resume {
                after_seq: frame.after_seq,
            })),
        }),
    }
}

fn command(frame: OutboundSessionFrame) -> Result<proto::command::Command, ClientCoreError> {
    match frame.command_kind {
        ClientCommandKind::SetSessionMode => Ok(proto::command::Command::SetSessionMode(
            proto::SetSessionModeRequest {
                thread_id: frame.thread_id,
                preset: frame.preset,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
        ClientCommandKind::SendSessionPrompt => Ok(proto::command::Command::SendSessionPrompt(
            proto::SendSessionPromptRequest {
                thread_id: frame.thread_id,
                prompt: frame.prompt,
                assistant_surface: frame.assistant_surface,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
        ClientCommandKind::SubmitNotificationReply => {
            Ok(proto::command::Command::SubmitNotificationReply(
                proto::SubmitNotificationReplyRequest {
                    notification_id: frame.notification_id,
                    thread_id: frame.thread_id,
                    prompt: frame.prompt,
                    assistant_surface: frame.assistant_surface,
                    client_mutation_id: frame.client_mutation_id,
                },
            ))
        }
        ClientCommandKind::Resume => Err(ClientCoreError::UnexpectedOutboxMutations),
    }
}

fn command_metadata(
    frame: &OutboundSessionFrame,
) -> Result<ClientCommandMetadata, ClientCoreError> {
    if frame.frame_kind != OutboundSessionFrameKind::Command
        || frame.command_kind == ClientCommandKind::Resume
    {
        return Err(ClientCoreError::UnexpectedOutboxMutations);
    }

    Ok(ClientCommandMetadata {
        command_kind: frame.command_kind,
        client_mutation_id: frame.client_mutation_id.clone(),
        preset: frame.preset.clone(),
        dispatch_kind: dispatch_kind(frame.command_kind).to_owned(),
        notification_id: frame.notification_id.clone(),
    })
}

fn dispatch_kind(command_kind: ClientCommandKind) -> &'static str {
    match command_kind {
        ClientCommandKind::SendSessionPrompt | ClientCommandKind::SubmitNotificationReply => {
            "accepted"
        }
        ClientCommandKind::SetSessionMode | ClientCommandKind::Resume => "",
    }
}

fn client_command_ack(ack: proto::CommandAck) -> ClientCommandAck {
    ClientCommandAck {
        accepted: ack.accepted,
        client_mutation_id: ack.client_mutation_id,
        ack_seq: ack.ack_seq,
        entity_id: ack.entity_id,
        revision: ack.revision,
        server_time: ack.server_time,
        idempotent_replay: ack.idempotent_replay,
        error_code: ack.error_code,
        reject_reason: ack.reject_reason,
        current_state: String::new(),
    }
}

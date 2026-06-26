use std::{collections::HashSet, time::Duration};

use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request as HyperRequest, StatusCode, Uri, body::Bytes};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::{iter, wrappers::ReceiverStream};
use tonic::{Request as TonicRequest, metadata::MetadataValue, transport::Endpoint};

use crate::{
    command_batch::build_command_batch_response,
    error::ClientCoreError,
    model::{
        ClientCommandAck, ClientCommandBatchResponse, ClientCommandKind, ClientCommandMetadata,
        ClientEndpoint, ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot,
        OutboundSessionFrame, OutboundSessionFrameKind,
    },
};

pub(crate) mod proto {
    tonic::include_proto!("looper.v1");
}

const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECTION_WARMUP_TIMEOUT: Duration = Duration::from_millis(1_500);
const STATE_MINI_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);
const STATE_MINI_RECONNECT_DELAY: Duration = Duration::from_millis(500);
const STATE_MINI_SNAPSHOT_PATH: &str = "/api/mobile/session-minis/snapshot";
const STATE_MINI_STREAM_ENDED: &str = "state mini stream ended";
const AUTHORIZATION_HEADER: &str = "authorization";
const MOBILE_SESSION_HEADER: &str = "x-looper-mobile-session";
const BEARER_PREFIX: &str = "Bearer ";

#[derive(Debug)]
pub(crate) enum StateMiniStreamEvent {
    Delta(ClientStateMiniDelta),
    RecoveredSnapshot {
        snapshot: ClientStateMiniSnapshot,
        error_description: String,
    },
    Heartbeat {
        latest_seq: i64,
        server_time: String,
    },
    Reconnecting {
        latest_seq: i64,
        error_description: String,
    },
    RecoveryRequired {
        latest_seq: i64,
        error_description: String,
    },
}

#[derive(Debug, Deserialize)]
struct StateMiniPayload {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "assistantSurface", default)]
    assistant_surface: String,
}

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
    let mut request = TonicRequest::new(iter(client_frames));
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

pub(crate) async fn warm_realtime_connection(
    endpoints: Vec<ClientEndpoint>,
    bearer_token: String,
    mobile_session_header: String,
) -> Result<(), ClientCoreError> {
    let endpoint = select_transport_endpoint(&endpoints)?;
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(endpoint)
        .await
        .map_err(|_| ClientCoreError::RealtimeConnectionWarmupFailed)?;
    let mut request = TonicRequest::new(proto::HealthRequest {});
    apply_metadata(request.metadata_mut(), bearer_token, mobile_session_header)?;
    let response = tokio::time::timeout(CONNECTION_WARMUP_TIMEOUT, client.health(request))
        .await
        .map_err(|_| ClientCoreError::RealtimeConnectionWarmupTimedOut)?
        .map_err(|_| ClientCoreError::RealtimeConnectionWarmupFailed)?;
    if response.into_inner().ok {
        Ok(())
    } else {
        Err(ClientCoreError::RealtimeConnectionWarmupFailed)
    }
}

pub(crate) async fn fetch_state_mini_snapshot(
    endpoints: Vec<ClientEndpoint>,
    bearer_token: String,
    mobile_session_header: String,
) -> Result<ClientStateMiniSnapshot, ClientCoreError> {
    let endpoint = select_client_endpoint(&endpoints)?;
    let uri = state_mini_snapshot_uri(&endpoint.url)?;
    let connector = HttpsConnectorBuilder::new()
        .with_webpki_roots()
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build();
    let client = Client::builder(TokioExecutor::new()).build(connector);
    let mut request = HyperRequest::builder().method(Method::GET).uri(uri);
    if !bearer_token.trim().is_empty() {
        request = request.header(
            AUTHORIZATION_HEADER,
            format!("{BEARER_PREFIX}{bearer_token}"),
        );
    }
    if !mobile_session_header.trim().is_empty() {
        request = request.header(MOBILE_SESSION_HEADER, mobile_session_header);
    }
    let request = request
        .body(Empty::<Bytes>::new())
        .map_err(|_| ClientCoreError::StateMiniSnapshotTransportFailed)?;

    let response = tokio::time::timeout(STATE_MINI_SNAPSHOT_TIMEOUT, client.request(request))
        .await
        .map_err(|_| ClientCoreError::StateMiniSnapshotTimedOut)?
        .map_err(|_| ClientCoreError::StateMiniSnapshotTransportFailed)?;
    if response.status() != StatusCode::OK {
        return Err(ClientCoreError::StateMiniSnapshotTransportFailed);
    }
    let body = response
        .into_body()
        .collect()
        .await
        .map_err(|_| ClientCoreError::StateMiniSnapshotTransportFailed)?
        .to_bytes();
    let body =
        serde_json::from_slice::<Value>(&body).map_err(|_| ClientCoreError::InvalidSnapshotJson)?;
    state_mini_snapshot_from_json(body)
}

pub(crate) async fn run_state_mini_stream(
    endpoints: Vec<ClientEndpoint>,
    bearer_token: String,
    mobile_session_header: String,
    after_seq: i64,
    events: mpsc::Sender<StateMiniStreamEvent>,
) {
    let mut next_after_seq = after_seq;
    loop {
        if events.is_closed() {
            return;
        }

        match run_state_mini_stream_session(
            &endpoints,
            &bearer_token,
            &mobile_session_header,
            next_after_seq,
            events.clone(),
        )
        .await
        {
            Ok(latest_seq) => {
                next_after_seq = next_after_seq.max(latest_seq);
                let _ = events
                    .send(state_mini_stream_ended_event(next_after_seq))
                    .await;
                tokio::time::sleep(STATE_MINI_RECONNECT_DELAY).await;
            }
            Err(StateMiniTransportError::RecoveryRequired {
                latest_seq,
                error_description,
            }) => {
                next_after_seq = next_after_seq.max(latest_seq);
                match fetch_state_mini_snapshot(
                    endpoints.clone(),
                    bearer_token.clone(),
                    mobile_session_header.clone(),
                )
                .await
                {
                    Ok(snapshot) => {
                        next_after_seq = next_after_seq.max(snapshot.latest_seq);
                        let _ = events
                            .send(StateMiniStreamEvent::RecoveredSnapshot {
                                snapshot,
                                error_description,
                            })
                            .await;
                    }
                    Err(error) => {
                        let _ = events
                            .send(StateMiniStreamEvent::RecoveryRequired {
                                latest_seq: next_after_seq,
                                error_description: format!(
                                    "{error_description}; snapshot recovery failed: {error}"
                                ),
                            })
                            .await;
                        tokio::time::sleep(STATE_MINI_RECONNECT_DELAY).await;
                    }
                }
            }
            Err(StateMiniTransportError::Transport {
                latest_seq,
                error_description,
            }) => {
                next_after_seq = next_after_seq.max(latest_seq);
                let _ = events
                    .send(StateMiniStreamEvent::Reconnecting {
                        latest_seq: next_after_seq,
                        error_description,
                    })
                    .await;
                tokio::time::sleep(STATE_MINI_RECONNECT_DELAY).await;
            }
        }
    }
}

#[derive(Debug)]
enum StateMiniTransportError {
    Transport {
        latest_seq: i64,
        error_description: String,
    },
    RecoveryRequired {
        latest_seq: i64,
        error_description: String,
    },
}

async fn run_state_mini_stream_session(
    endpoints: &[ClientEndpoint],
    bearer_token: &str,
    mobile_session_header: &str,
    after_seq: i64,
    events: mpsc::Sender<StateMiniStreamEvent>,
) -> Result<i64, StateMiniTransportError> {
    let endpoint = select_transport_endpoint(endpoints).map_err(|error| {
        StateMiniTransportError::Transport {
            latest_seq: after_seq,
            error_description: error.to_string(),
        }
    })?;
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(endpoint)
        .await
        .map_err(|error| StateMiniTransportError::Transport {
            latest_seq: after_seq,
            error_description: error.to_string(),
        })?;
    let (request_sender, request_receiver) = mpsc::channel(1);
    request_sender
        .send(resume_client_frame(after_seq))
        .await
        .map_err(|error| StateMiniTransportError::Transport {
            latest_seq: after_seq,
            error_description: error.to_string(),
        })?;
    let mut request = TonicRequest::new(ReceiverStream::new(request_receiver));
    apply_metadata(
        request.metadata_mut(),
        bearer_token.to_owned(),
        mobile_session_header.to_owned(),
    )
    .map_err(|error| StateMiniTransportError::Transport {
        latest_seq: after_seq,
        error_description: error.to_string(),
    })?;

    let response = client.session(request).await.map_err(|status| {
        if status.code() == tonic::Code::OutOfRange {
            StateMiniTransportError::RecoveryRequired {
                latest_seq: after_seq,
                error_description: status.message().to_owned(),
            }
        } else {
            StateMiniTransportError::Transport {
                latest_seq: after_seq,
                error_description: status.to_string(),
            }
        }
    })?;
    let _request_keepalive = request_sender;
    let mut stream = response.into_inner();
    let mut latest_seq = after_seq;
    while let Some(frame) = stream.message().await.map_err(|status| {
        if status.code() == tonic::Code::OutOfRange {
            StateMiniTransportError::RecoveryRequired {
                latest_seq,
                error_description: status.message().to_owned(),
            }
        } else {
            StateMiniTransportError::Transport {
                latest_seq,
                error_description: status.to_string(),
            }
        }
    })? {
        match frame.frame {
            Some(proto::server_frame::Frame::StateDelta(delta)) => {
                latest_seq = latest_seq.max(delta.seq);
                events
                    .send(StateMiniStreamEvent::Delta(client_state_mini_delta(delta)?))
                    .await
                    .map_err(|error| StateMiniTransportError::Transport {
                        latest_seq,
                        error_description: error.to_string(),
                    })?;
            }
            Some(proto::server_frame::Frame::Heartbeat(heartbeat)) => {
                latest_seq = latest_seq.max(heartbeat.latest_seq);
                events
                    .send(StateMiniStreamEvent::Heartbeat {
                        latest_seq,
                        server_time: heartbeat.server_time,
                    })
                    .await
                    .map_err(|error| StateMiniTransportError::Transport {
                        latest_seq,
                        error_description: error.to_string(),
                    })?;
            }
            _ => {}
        }
    }

    Ok(latest_seq)
}

fn state_mini_stream_ended_event(latest_seq: i64) -> StateMiniStreamEvent {
    StateMiniStreamEvent::Reconnecting {
        latest_seq,
        error_description: STATE_MINI_STREAM_ENDED.to_owned(),
    }
}

fn select_transport_endpoint(endpoints: &[ClientEndpoint]) -> Result<Endpoint, ClientCoreError> {
    let endpoint = select_client_endpoint(endpoints)?;
    Endpoint::from_shared(endpoint.url).map_err(|_| ClientCoreError::InvalidEndpoint)
}

fn select_client_endpoint(endpoints: &[ClientEndpoint]) -> Result<ClientEndpoint, ClientCoreError> {
    endpoints
        .iter()
        .find(|endpoint| endpoint.last_good)
        .or_else(|| endpoints.first())
        .cloned()
        .ok_or(ClientCoreError::NoEndpoint)
}

fn state_mini_snapshot_uri(endpoint_url: &str) -> Result<Uri, ClientCoreError> {
    let endpoint_url = endpoint_url.trim().trim_end_matches('/');
    if endpoint_url.is_empty() {
        return Err(ClientCoreError::InvalidEndpoint);
    }
    format!("{endpoint_url}{STATE_MINI_SNAPSHOT_PATH}")
        .parse::<Uri>()
        .map_err(|_| ClientCoreError::InvalidEndpoint)
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

fn resume_client_frame(after_seq: i64) -> proto::ClientFrame {
    proto::ClientFrame {
        frame: Some(proto::client_frame::Frame::Resume(proto::Resume {
            after_seq,
        })),
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

fn state_mini_snapshot_from_json(value: Value) -> Result<ClientStateMiniSnapshot, ClientCoreError> {
    let latest_seq = value
        .get("latestSeq")
        .or_else(|| value.get("latest_seq"))
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let server_time = value
        .get("serverTime")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let sessions = value
        .get("sessions")
        .and_then(Value::as_array)
        .map(|sessions| {
            sessions
                .iter()
                .filter_map(client_state_mini_from_json)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(ClientStateMiniSnapshot {
        latest_seq,
        sessions,
        server_time,
    })
}

fn client_state_mini_from_json(value: &Value) -> Option<ClientStateMini> {
    let session_id = value
        .get("sessionID")
        .or_else(|| value.get("sessionId"))
        .and_then(Value::as_str)?
        .trim();
    if session_id.is_empty() {
        return None;
    }
    let payload_json = serde_json::to_string(value).ok()?;
    Some(ClientStateMini {
        session_id: session_id.to_owned(),
        assistant_surface: value
            .get("assistantSurface")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        seq: value.get("seq").and_then(Value::as_i64).unwrap_or_default(),
        revision: value
            .get("revision")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        payload_json,
    })
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

fn client_state_mini_delta(
    delta: proto::StateMiniDelta,
) -> Result<ClientStateMiniDelta, StateMiniTransportError> {
    let payload =
        serde_json::from_str::<StateMiniPayload>(&delta.payload_json).map_err(|error| {
            StateMiniTransportError::Transport {
                latest_seq: delta.seq,
                error_description: format!("state mini payload json invalid: {error}"),
            }
        })?;
    let session = ClientStateMini {
        session_id: payload.session_id.clone(),
        assistant_surface: payload.assistant_surface,
        seq: delta.seq,
        revision: delta.revision.clone(),
        payload_json: delta.payload_json,
    };
    Ok(ClientStateMiniDelta {
        seq: delta.seq,
        latest_seq: delta.seq,
        entity_id: delta.entity_id,
        kind: delta.kind,
        revision: delta.revision,
        server_time: delta.server_time,
        has_session: true,
        session,
        sessions: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn state_mini_snapshot_from_json_preserves_payloads_and_skips_invalid_sessions() {
        let snapshot = state_mini_snapshot_from_json(json!({
            "latest_seq": 12,
            "serverTime": "2026-06-26T00:00:00Z",
            "sessions": [
                {
                    "sessionId": "thread-1",
                    "assistantSurface": "codex",
                    "seq": 11,
                    "revision": "rev-11",
                    "title": "Build Looper"
                },
                {
                    "sessionID": "thread-2",
                    "seq": 12,
                    "revision": "rev-12"
                },
                {
                    "assistantSurface": "codex",
                    "seq": 13
                }
            ]
        }))
        .expect("snapshot");

        assert_eq!(snapshot.latest_seq, 12);
        assert_eq!(snapshot.server_time, "2026-06-26T00:00:00Z");
        assert_eq!(snapshot.sessions.len(), 2);
        assert_eq!(snapshot.sessions[0].session_id, "thread-1");
        assert_eq!(snapshot.sessions[0].assistant_surface, "codex");
        assert_eq!(snapshot.sessions[0].seq, 11);
        assert!(
            snapshot.sessions[0]
                .payload_json
                .contains("\"title\":\"Build Looper\"")
        );
        assert_eq!(snapshot.sessions[1].session_id, "thread-2");
        assert!(snapshot.sessions[1].assistant_surface.is_empty());
    }

    #[test]
    fn state_mini_snapshot_url_appends_recovery_path() {
        let url = state_mini_snapshot_uri("http://127.0.0.1:8766/base/").expect("snapshot url");

        assert_eq!(
            url.to_string(),
            "http://127.0.0.1:8766/base/api/mobile/session-minis/snapshot"
        );
    }

    #[test]
    fn clean_state_mini_stream_end_is_reconnectable() {
        let event = state_mini_stream_ended_event(42);

        match event {
            StateMiniStreamEvent::Reconnecting {
                latest_seq,
                error_description,
            } => {
                assert_eq!(latest_seq, 42);
                assert_eq!(error_description, STATE_MINI_STREAM_ENDED);
            }
            other => panic!("expected reconnecting event, got {other:?}"),
        }
    }
}

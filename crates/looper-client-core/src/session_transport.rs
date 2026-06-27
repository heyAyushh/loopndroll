use std::{collections::HashSet, time::Duration};

use http_body_util::{BodyExt, Empty};
use hyper::{Method, Request as HyperRequest, StatusCode, Uri, body::Bytes};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request as TonicRequest, metadata::MetadataValue, transport::Endpoint};

use crate::{
    error::ClientCoreError,
    model::{
        ClientCommandAck, ClientCommandKind, ClientCommandMetadata, ClientEndpoint,
        ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot, OutboundSessionFrame,
        OutboundSessionFrameKind,
    },
};

pub(crate) mod proto {
    tonic::include_proto!("looper.v1");
}

const STATE_MINI_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);
const STATE_MINI_RECONNECT_DELAY: Duration = Duration::from_millis(500);
const STATE_MINI_SNAPSHOT_PATH: &str = "/api/mobile/session-minis/snapshot";
const STATE_MINI_STREAM_ENDED: &str = "state mini stream ended";
const DEFAULT_HTTP_API_PORT: u16 = 8765;
const DEFAULT_REALTIME_GRPC_PORT: u16 = 8766;
const AUTHORIZATION_HEADER: &str = "authorization";
const MOBILE_SESSION_HEADER: &str = "x-looper-mobile-session";
const BEARER_PREFIX: &str = "Bearer ";
const SESSION_ID_FIELD: &str = "sessionId";
const SESSION_ID_ALIAS_FIELD: &str = "sessionID";
const PAYLOAD_ID_FIELD: &str = "id";
const ASSISTANT_SURFACE_FIELD: &str = "assistantSurface";
const LATEST_SEQ_FIELD: &str = "latestSeq";
const LATEST_SEQ_ALIAS_FIELD: &str = "latest_seq";
const REPLACE_FIELD: &str = "replace";
const SESSIONS_FIELD: &str = "sessions";

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

pub(crate) async fn fetch_state_mini_snapshot(
    endpoints: Vec<ClientEndpoint>,
    bearer_token: String,
    mobile_session_header: String,
) -> Result<ClientStateMiniSnapshot, ClientCoreError> {
    let mut last_error = ClientCoreError::StateMiniSnapshotTransportFailed;
    for endpoint in snapshot_recovery_endpoints(&endpoints)? {
        match fetch_state_mini_snapshot_from_endpoint(
            &endpoint,
            bearer_token.as_str(),
            mobile_session_header.as_str(),
        )
        .await
        {
            Ok(snapshot) => return Ok(snapshot),
            Err(error) => last_error = error,
        }
    }

    Err(last_error)
}

async fn fetch_state_mini_snapshot_from_endpoint(
    endpoint: &ClientEndpoint,
    bearer_token: &str,
    mobile_session_header: &str,
) -> Result<ClientStateMiniSnapshot, ClientCoreError> {
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
    mut commands: mpsc::Receiver<OutboundSessionFrame>,
    events: mpsc::Sender<StateMiniStreamEvent>,
    command_acks: mpsc::Sender<ClientCommandAck>,
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
            &mut commands,
            events.clone(),
            command_acks.clone(),
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
    commands: &mut mpsc::Receiver<OutboundSessionFrame>,
    events: mpsc::Sender<StateMiniStreamEvent>,
    command_acks: mpsc::Sender<ClientCommandAck>,
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
    let (request_sender, request_receiver) = mpsc::channel(64);
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
    let mut stream = response.into_inner();
    let mut latest_seq = after_seq;
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    return Ok(latest_seq);
                };
                let frame = client_frame(command).map_err(|error| StateMiniTransportError::Transport {
                    latest_seq,
                    error_description: error.to_string(),
                })?;
                request_sender
                    .send(frame)
                    .await
                    .map_err(|error| StateMiniTransportError::Transport {
                        latest_seq,
                        error_description: error.to_string(),
                    })?;
            }
            frame = stream.message() => {
                let Some(frame) = frame.map_err(|status| {
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
                })? else {
                    return Ok(latest_seq);
                };

                match frame.frame {
                    Some(proto::server_frame::Frame::Ack(ack)) => {
                        let ack = client_command_ack(ack);
                        latest_seq = latest_seq.max(ack.ack_seq);
                        command_acks
                            .send(ack)
                            .await
                            .map_err(|error| StateMiniTransportError::Transport {
                                latest_seq,
                                error_description: error.to_string(),
                            })?;
                    }
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
        }
    }
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

fn snapshot_recovery_endpoints(
    endpoints: &[ClientEndpoint],
) -> Result<Vec<ClientEndpoint>, ClientCoreError> {
    if endpoints.is_empty() {
        return Err(ClientCoreError::NoEndpoint);
    }

    let mut seen_urls = HashSet::new();
    let mut ordered = Vec::with_capacity(endpoints.len());
    for prefer_last_good in [true, false] {
        for endpoint in endpoints
            .iter()
            .filter(|endpoint| endpoint.last_good == prefer_last_good)
        {
            let normalized_url = endpoint.url.trim().trim_end_matches('/').to_owned();
            if seen_urls.insert(normalized_url) {
                ordered.push(endpoint.clone());
            }
        }
    }

    Ok(ordered)
}

fn state_mini_snapshot_uri(endpoint_url: &str) -> Result<Uri, ClientCoreError> {
    let endpoint_url = endpoint_url.trim().trim_end_matches('/');
    if endpoint_url.is_empty() {
        return Err(ClientCoreError::InvalidEndpoint);
    }
    let recovery_base_url = state_mini_snapshot_base_url(endpoint_url)?;
    format!("{recovery_base_url}{STATE_MINI_SNAPSHOT_PATH}")
        .parse::<Uri>()
        .map_err(|_| ClientCoreError::InvalidEndpoint)
}

fn state_mini_snapshot_base_url(endpoint_url: &str) -> Result<String, ClientCoreError> {
    let uri = endpoint_url
        .parse::<Uri>()
        .map_err(|_| ClientCoreError::InvalidEndpoint)?;
    let scheme = uri.scheme_str().ok_or(ClientCoreError::InvalidEndpoint)?;
    let authority = uri.authority().ok_or(ClientCoreError::InvalidEndpoint)?;
    let path = uri.path().trim_end_matches('/');
    let path = if path == "/" { "" } else { path };
    let authority = snapshot_recovery_authority(scheme, authority.as_str());
    Ok(format!("{scheme}://{authority}{path}"))
}

fn snapshot_recovery_authority(scheme: &str, authority: &str) -> String {
    let realtime_port_suffix = format!(":{DEFAULT_REALTIME_GRPC_PORT}");
    if scheme == "http" && authority.ends_with(&realtime_port_suffix) {
        let host = authority.trim_end_matches(&realtime_port_suffix);
        return format!("{host}:{DEFAULT_HTTP_API_PORT}");
    }
    authority.to_owned()
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
                prompt_intent: frame.prompt_intent,
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
        ClientCommandKind::SetAssistantSurface => Ok(proto::command::Command::SetAssistantSurface(
            proto::SetAssistantSurfaceRequest {
                assistant_surface: frame.assistant_surface,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
        ClientCommandKind::SetSiriCurrentSession => Ok(
            proto::command::Command::SetSiriCurrentSession(proto::SetSiriCurrentSessionRequest {
                thread_id: frame.thread_id,
                assistant_surface: frame.assistant_surface,
                client_mutation_id: frame.client_mutation_id,
            }),
        ),
        ClientCommandKind::SetSiriDefaultSession => Ok(
            proto::command::Command::SetSiriDefaultSession(proto::SetSiriDefaultSessionRequest {
                thread_id: frame.thread_id,
                assistant_surface: frame.assistant_surface,
                client_mutation_id: frame.client_mutation_id,
            }),
        ),
        ClientCommandKind::SaveDefaultPrompt => Ok(proto::command::Command::SaveDefaultPrompt(
            proto::SaveDefaultPromptRequest {
                prompt: frame.prompt,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
        ClientCommandKind::SetSessionArchived => Ok(proto::command::Command::SetSessionArchived(
            proto::SetSessionArchivedRequest {
                thread_id: frame.thread_id,
                archived: frame.archived,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
        ClientCommandKind::DeleteSession => Ok(proto::command::Command::DeleteSession(
            proto::DeleteSessionRequest {
                thread_id: frame.thread_id,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
        ClientCommandKind::MuteSession => Ok(proto::command::Command::MuteSession(
            proto::MuteSessionRequest {
                thread_id: frame.thread_id,
                client_mutation_id: frame.client_mutation_id,
            },
        )),
    }
}

pub(crate) fn command_metadata(
    frame: &OutboundSessionFrame,
) -> Result<ClientCommandMetadata, ClientCoreError> {
    if frame.frame_kind != OutboundSessionFrameKind::Command {
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
        ClientCommandKind::SetSessionMode
        | ClientCommandKind::SetAssistantSurface
        | ClientCommandKind::SetSiriCurrentSession
        | ClientCommandKind::SetSiriDefaultSession
        | ClientCommandKind::SaveDefaultPrompt
        | ClientCommandKind::SetSessionArchived
        | ClientCommandKind::DeleteSession
        | ClientCommandKind::MuteSession => "",
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
    let payload = serde_json::from_str::<Value>(&delta.payload_json).ok();
    let Some(payload) = payload else {
        return Ok(seq_only_state_mini_delta(delta));
    };
    let replace_sessions = payload
        .get(REPLACE_FIELD)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if replace_sessions {
        let sessions = state_mini_payload_sessions(&payload);
        if !sessions.is_empty() {
            return Ok(ClientStateMiniDelta {
                seq: delta.seq,
                latest_seq: state_mini_payload_latest_seq(&payload, delta.seq),
                entity_id: delta.entity_id,
                kind: delta.kind,
                revision: delta.revision,
                server_time: delta.server_time,
                has_session: false,
                session: ClientStateMini {
                    session_id: String::new(),
                    assistant_surface: String::new(),
                    seq: delta.seq,
                    revision: String::new(),
                    payload_json: String::new(),
                },
                sessions,
            });
        }
    }
    let Some(session_id) = state_mini_payload_session_id(&payload) else {
        return Ok(seq_only_state_mini_delta(delta));
    };
    let assistant_surface = payload
        .get(ASSISTANT_SURFACE_FIELD)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let payload_json = normalized_state_mini_payload_json(payload, &session_id);
    let session = ClientStateMini {
        session_id: session_id.clone(),
        assistant_surface,
        seq: delta.seq,
        revision: delta.revision.clone(),
        payload_json,
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

fn state_mini_payload_latest_seq(payload: &Value, fallback: i64) -> i64 {
    payload
        .get(LATEST_SEQ_FIELD)
        .or_else(|| payload.get(LATEST_SEQ_ALIAS_FIELD))
        .and_then(Value::as_i64)
        .unwrap_or(fallback)
}

fn state_mini_payload_sessions(payload: &Value) -> Vec<ClientStateMini> {
    payload
        .get(SESSIONS_FIELD)
        .and_then(Value::as_array)
        .map(|sessions| {
            sessions
                .iter()
                .filter_map(client_state_mini_from_json)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn seq_only_state_mini_delta(delta: proto::StateMiniDelta) -> ClientStateMiniDelta {
    ClientStateMiniDelta {
        seq: delta.seq,
        latest_seq: delta.seq,
        entity_id: delta.entity_id,
        kind: delta.kind,
        revision: delta.revision,
        server_time: delta.server_time,
        has_session: false,
        session: ClientStateMini {
            session_id: String::new(),
            assistant_surface: String::new(),
            seq: delta.seq,
            revision: String::new(),
            payload_json: String::new(),
        },
        sessions: Vec::new(),
    }
}

fn state_mini_payload_session_id(payload: &Value) -> Option<String> {
    [SESSION_ID_FIELD, SESSION_ID_ALIAS_FIELD, PAYLOAD_ID_FIELD]
        .into_iter()
        .find_map(|field| payload.get(field).and_then(Value::as_str))
        .filter(|session_id| !session_id.trim().is_empty())
        .map(str::to_owned)
}

fn normalized_state_mini_payload_json(mut payload: Value, session_id: &str) -> String {
    if let Some(object) = payload.as_object_mut() {
        object
            .entry(PAYLOAD_ID_FIELD.to_owned())
            .or_insert_with(|| Value::String(session_id.to_owned()));
        object
            .entry(SESSION_ID_FIELD.to_owned())
            .or_insert_with(|| Value::String(session_id.to_owned()));
    }
    serde_json::to_string(&payload).unwrap_or_default()
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
    fn state_mini_snapshot_url_uses_http_api_port_for_realtime_endpoint() {
        let url = state_mini_snapshot_uri("http://127.0.0.1:8766/base/").expect("snapshot url");

        assert_eq!(
            url.to_string(),
            "http://127.0.0.1:8765/base/api/mobile/session-minis/snapshot"
        );
    }

    #[test]
    fn state_mini_snapshot_url_keeps_non_realtime_ports() {
        let url = state_mini_snapshot_uri("https://100.119.200.69:8781/").expect("snapshot url");

        assert_eq!(
            url.to_string(),
            "https://100.119.200.69:8781/api/mobile/session-minis/snapshot"
        );
    }

    #[test]
    fn snapshot_recovery_endpoints_keep_fallbacks_after_last_good() {
        let endpoints = snapshot_recovery_endpoints(&[
            ClientEndpoint {
                url: "http://100.119.200.69:8765".to_owned(),
                last_good: false,
            },
            ClientEndpoint {
                url: "http://192.168.1.33:8765".to_owned(),
                last_good: true,
            },
            ClientEndpoint {
                url: "http://192.168.1.33:8765/".to_owned(),
                last_good: false,
            },
            ClientEndpoint {
                url: "http://127.0.0.1:8765".to_owned(),
                last_good: false,
            },
        ])
        .expect("endpoints");

        let urls = endpoints
            .into_iter()
            .map(|endpoint| endpoint.url)
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            vec![
                "http://192.168.1.33:8765",
                "http://100.119.200.69:8765",
                "http://127.0.0.1:8765",
            ]
        );
    }

    #[test]
    fn state_mini_delta_without_mini_payload_advances_sequence_only() {
        let delta = client_state_mini_delta(proto::StateMiniDelta {
            seq: 14,
            entity_id: "thread-1".to_owned(),
            kind: "session_changed".to_owned(),
            revision: "rev-14".to_owned(),
            server_time: "2026-06-27T00:00:14Z".to_owned(),
            payload_json: r#"{"threadId":"thread-1","detail":"command-ack"}"#.to_owned(),
        })
        .expect("delta");

        assert_eq!(delta.seq, 14);
        assert_eq!(delta.latest_seq, 14);
        assert!(!delta.has_session);
        assert!(delta.sessions.is_empty());
    }

    #[test]
    fn state_mini_delta_accepts_payload_id_alias_and_normalizes_session_id() {
        let delta = client_state_mini_delta(proto::StateMiniDelta {
            seq: 15,
            entity_id: "thread-2".to_owned(),
            kind: "session_changed".to_owned(),
            revision: "rev-15".to_owned(),
            server_time: "2026-06-27T00:00:15Z".to_owned(),
            payload_json: r#"{"id":"thread-2","assistantSurface":"codex","title":"Build"}"#
                .to_owned(),
        })
        .expect("delta");

        assert!(delta.has_session);
        assert_eq!(delta.session.session_id, "thread-2");
        assert_eq!(delta.session.assistant_surface, "codex");
        let payload: Value =
            serde_json::from_str(&delta.session.payload_json).expect("normalized payload");
        assert_eq!(payload["id"], "thread-2");
        assert_eq!(payload["sessionId"], "thread-2");
    }

    #[test]
    fn state_mini_delta_accepts_replace_sessions_payload() {
        let delta = client_state_mini_delta(proto::StateMiniDelta {
            seq: 16,
            entity_id: "mobile".to_owned(),
            kind: "session_changed".to_owned(),
            revision: "rev-16".to_owned(),
            server_time: "2026-06-27T00:00:16Z".to_owned(),
            payload_json: json!({
                "latestSeq": 16,
                "replace": true,
                "sessions": [
                    {
                        "sessionId": "thread-1",
                        "assistantSurface": "codex",
                        "seq": 16,
                        "revision": "rev-16",
                        "title": "One"
                    },
                    {
                        "sessionID": "thread-2",
                        "assistantSurface": "devin",
                        "seq": 16,
                        "revision": "rev-16",
                        "title": "Two"
                    }
                ]
            })
            .to_string(),
        })
        .expect("delta");

        assert!(!delta.has_session);
        assert_eq!(delta.latest_seq, 16);
        assert_eq!(delta.sessions.len(), 2);
        assert_eq!(delta.sessions[0].session_id, "thread-1");
        assert_eq!(delta.sessions[1].session_id, "thread-2");
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

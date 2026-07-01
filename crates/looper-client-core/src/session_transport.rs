use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

use http_body_util::{BodyExt, Empty, Limited};
use hyper::{Method, Request as HyperRequest, StatusCode, Uri, body::Bytes};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{client::legacy::Client, rt::TokioExecutor};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request as TonicRequest, metadata::MetadataValue, transport::Endpoint};
use tonic_h3::quinn::H3QuinnConnector;
use tonic_h3::quinn::h3_quinn::quinn::{
    ClientConfig, crypto::rustls::QuicClientConfig, rustls as quinn_rustls,
};
use x509_parser::prelude::{FromDer, X509Certificate};

use crate::{
    error::ClientCoreError,
    model::{
        ClientCommandAck, ClientCommandKind, ClientCommandMetadata, ClientEndpoint,
        ClientEndpointTransport, ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot,
        ClientTextChunk, OutboundSessionFrame, OutboundSessionFrameKind,
        STATE_MINI_BATCH_COMPLETE_KIND, STATE_MINI_REPLACEMENT_COMPLETE_KIND,
        STATE_MINI_REPLACEMENT_KIND,
    },
};

pub(crate) mod proto {
    tonic::include_proto!("looper.v1");
}

const STATE_MINI_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(2);
const STATE_MINI_STREAM_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const STATE_MINI_STREAM_FALLBACK_RACE_DELAY: Duration = Duration::from_millis(25);
const STATE_MINI_RECONNECT_DELAY: Duration = Duration::from_millis(500);
const STATE_MINI_SNAPSHOT_PATH: &str = "/api/mobile/session-minis/snapshot";
const MAX_STATE_MINI_SNAPSHOT_BYTES: usize = 512 * 1024;
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
const REPLACEMENT_COMPLETE_FIELD: &str = "replacementComplete";
const REPLACEMENT_COMPLETE_ALIAS_FIELD: &str = "replacement_complete";
const SESSIONS_FIELD: &str = "sessions";
const RECOVERY_REQUIRED_FIELD: &str = "recoveryRequired";
const RECOVERY_REQUIRED_ALIAS_FIELD: &str = "recovery_required";
const RECOVERY_FIELD: &str = "recovery";
const RECOVERY_INSTRUCTION_STATE_MINI_SNAPSHOT: &str = "session-mini-snapshot";
const REASON_FIELD: &str = "reason";
const DEFAULT_RECOVERY_REQUIRED_REASON: &str = "state-mini snapshot recovery required";

#[derive(Debug)]
pub(crate) enum StateMiniStreamEvent {
    Delta(ClientStateMiniDelta),
    TextChunk(ClientTextChunk),
    Heartbeat {
        latest_seq: i64,
        server_time: String,
        endpoint_url: String,
        endpoint_transport: ClientEndpointTransport,
        fallback_reason: String,
    },
    Reconnecting {
        latest_seq: i64,
        error_description: String,
        endpoint_transport: ClientEndpointTransport,
        fallback_reason: String,
    },
    RecoveryRequired {
        latest_seq: i64,
        error_description: String,
        endpoint_transport: ClientEndpointTransport,
        fallback_reason: String,
    },
}

#[derive(Debug)]
pub(crate) struct RecoveredStateMiniSnapshot {
    pub(crate) snapshot: ClientStateMiniSnapshot,
    pub(crate) endpoint_url: String,
    pub(crate) endpoint_transport: ClientEndpointTransport,
}

pub(crate) async fn fetch_state_mini_snapshot(
    endpoints: Vec<ClientEndpoint>,
    bearer_token: String,
    mobile_session_header: String,
) -> Result<RecoveredStateMiniSnapshot, ClientCoreError> {
    let candidates = snapshot_recovery_endpoints(&endpoints)?;
    let mut last_error = ClientCoreError::StateMiniSnapshotTransportFailed;
    let (result_sender, mut result_receiver) = mpsc::channel(candidates.len());
    let mut handles = Vec::with_capacity(candidates.len());
    for (index, endpoint) in candidates.into_iter().enumerate() {
        let result_sender = result_sender.clone();
        let bearer_token = bearer_token.clone();
        let mobile_session_header = mobile_session_header.clone();
        handles.push(tokio::spawn(async move {
            if index > 0 {
                tokio::time::sleep(STATE_MINI_STREAM_FALLBACK_RACE_DELAY).await;
            }
            let result = fetch_state_mini_snapshot_from_endpoint(
                &endpoint,
                bearer_token.as_str(),
                mobile_session_header.as_str(),
            )
            .await
            .map(|snapshot| RecoveredStateMiniSnapshot {
                snapshot,
                endpoint_url: normalized_endpoint_url(&endpoint.url),
                endpoint_transport: endpoint.transport,
            });
            let _ = result_sender.send(result).await;
        }));
    }
    drop(result_sender);

    while let Some(result) = result_receiver.recv().await {
        match result {
            Ok(recovered) => {
                for handle in handles {
                    handle.abort();
                }
                return Ok(recovered);
            }
            Err(error) => {
                last_error = error;
            }
        }
    }

    Err(last_error)
}

async fn fetch_state_mini_snapshot_from_endpoint(
    endpoint: &ClientEndpoint,
    bearer_token: &str,
    mobile_session_header: &str,
) -> Result<ClientStateMiniSnapshot, ClientCoreError> {
    let uri = state_mini_snapshot_uri(endpoint)?;
    let _ = rustls::crypto::ring::default_provider().install_default();
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
    let body = Limited::new(response.into_body(), MAX_STATE_MINI_SNAPSHOT_BYTES)
        .collect()
        .await
        .map_err(|_| ClientCoreError::StateMiniSnapshotTransportFailed)?
        .to_bytes();
    let body =
        serde_json::from_slice::<Value>(&body).map_err(|_| ClientCoreError::InvalidSnapshotJson)?;
    state_mini_snapshot_from_json(body)
}

pub(crate) async fn run_state_mini_stream(
    mut endpoints: Vec<ClientEndpoint>,
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
            &mut endpoints,
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
                endpoint_transport,
                fallback_reason,
            }) => {
                next_after_seq = next_after_seq.max(latest_seq);
                let _ = events
                    .send(StateMiniStreamEvent::RecoveryRequired {
                        latest_seq: next_after_seq,
                        error_description,
                        endpoint_transport,
                        fallback_reason,
                    })
                    .await;
                tokio::time::sleep(STATE_MINI_RECONNECT_DELAY).await;
            }
            Err(StateMiniTransportError::Transport {
                latest_seq,
                error_description,
                endpoint_transport,
                fallback_reason,
            }) => {
                next_after_seq = next_after_seq.max(latest_seq);
                let _ = events
                    .send(StateMiniStreamEvent::Reconnecting {
                        latest_seq: next_after_seq,
                        error_description,
                        endpoint_transport,
                        fallback_reason,
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
        endpoint_transport: ClientEndpointTransport,
        fallback_reason: String,
    },
    RecoveryRequired {
        latest_seq: i64,
        error_description: String,
        endpoint_transport: ClientEndpointTransport,
        fallback_reason: String,
    },
}

fn state_mini_transport_error(
    latest_seq: i64,
    error_description: String,
) -> StateMiniTransportError {
    StateMiniTransportError::Transport {
        latest_seq,
        error_description,
        endpoint_transport: ClientEndpointTransport::H2,
        fallback_reason: String::new(),
    }
}

fn state_mini_transport_error_with_endpoint(
    latest_seq: i64,
    error_description: String,
    endpoint_transport: ClientEndpointTransport,
    fallback_reason: String,
) -> StateMiniTransportError {
    StateMiniTransportError::Transport {
        latest_seq,
        error_description,
        endpoint_transport,
        fallback_reason,
    }
}

fn state_mini_recovery_required_error(
    latest_seq: i64,
    error_description: String,
) -> StateMiniTransportError {
    StateMiniTransportError::RecoveryRequired {
        latest_seq,
        error_description,
        endpoint_transport: ClientEndpointTransport::H2,
        fallback_reason: String::new(),
    }
}

fn state_mini_recovery_required_error_with_endpoint(
    latest_seq: i64,
    error_description: String,
    endpoint_transport: ClientEndpointTransport,
    fallback_reason: String,
) -> StateMiniTransportError {
    StateMiniTransportError::RecoveryRequired {
        latest_seq,
        error_description,
        endpoint_transport,
        fallback_reason,
    }
}

struct OpenStateMiniSession {
    stream: tonic::Streaming<proto::ServerFrame>,
    request_sender: mpsc::Sender<proto::ClientFrame>,
    endpoint_url: String,
    endpoint_transport: ClientEndpointTransport,
    fallback_reason: String,
}

async fn run_state_mini_stream_session(
    endpoints: &mut [ClientEndpoint],
    bearer_token: &str,
    mobile_session_header: &str,
    after_seq: i64,
    commands: &mut mpsc::Receiver<OutboundSessionFrame>,
    events: mpsc::Sender<StateMiniStreamEvent>,
    command_acks: mpsc::Sender<ClientCommandAck>,
) -> Result<i64, StateMiniTransportError> {
    let candidates = session_transport_endpoints(endpoints)
        .map_err(|error| state_mini_transport_error(after_seq, error.to_string()))?;
    let mut last_transport_error = ClientCoreError::StateMiniSnapshotTransportFailed.to_string();
    let mut h3_fallback_reason = String::new();
    for tier in session_transport_tiers(candidates) {
        let is_h3_tier = tier
            .first()
            .map(|endpoint| endpoint.transport == ClientEndpointTransport::H3)
            .unwrap_or(false);
        match open_state_mini_stream_tier(
            tier,
            bearer_token,
            mobile_session_header,
            after_seq,
            h3_fallback_reason.clone(),
        )
        .await
        {
            Ok(opened) => {
                let OpenStateMiniSession {
                    stream,
                    request_sender,
                    endpoint_url,
                    endpoint_transport,
                    fallback_reason,
                } = opened;
                mark_endpoint_last_good(endpoints, &endpoint_url, endpoint_transport);
                return drive_state_mini_stream_session(
                    stream,
                    request_sender,
                    commands,
                    events,
                    command_acks,
                    after_seq,
                    endpoint_url,
                    endpoint_transport,
                    fallback_reason,
                )
                .await;
            }
            Err(StateMiniTransportError::RecoveryRequired {
                latest_seq,
                error_description,
                endpoint_transport,
                fallback_reason,
            }) => {
                return Err(StateMiniTransportError::RecoveryRequired {
                    latest_seq,
                    error_description,
                    endpoint_transport,
                    fallback_reason,
                });
            }
            Err(StateMiniTransportError::Transport {
                error_description, ..
            }) => {
                if is_h3_tier {
                    h3_fallback_reason = format!("h3 pre-stream failure: {error_description}");
                }
                last_transport_error = error_description;
            }
        }
    }

    Err(state_mini_transport_error(after_seq, last_transport_error))
}

async fn open_state_mini_stream_tier(
    candidates: Vec<ClientEndpoint>,
    bearer_token: &str,
    mobile_session_header: &str,
    after_seq: i64,
    fallback_reason: String,
) -> Result<OpenStateMiniSession, StateMiniTransportError> {
    let mut last_transport_error = ClientCoreError::StateMiniSnapshotTransportFailed.to_string();
    let (result_sender, mut result_receiver) = mpsc::channel(candidates.len());
    let mut handles = Vec::with_capacity(candidates.len());
    for (index, endpoint) in candidates.into_iter().enumerate() {
        let result_sender = result_sender.clone();
        let bearer_token = bearer_token.to_owned();
        let mobile_session_header = mobile_session_header.to_owned();
        let fallback_reason = fallback_reason.clone();
        handles.push(tokio::spawn(async move {
            if index > 0 {
                tokio::time::sleep(STATE_MINI_STREAM_FALLBACK_RACE_DELAY).await;
            }
            let result = open_state_mini_stream_candidate(
                endpoint,
                bearer_token,
                mobile_session_header,
                after_seq,
                fallback_reason,
            )
            .await;
            let _ = result_sender.send(result).await;
        }));
    }
    drop(result_sender);

    while let Some(result) = result_receiver.recv().await {
        match result {
            Ok(opened) => {
                for handle in handles {
                    handle.abort();
                }
                return Ok(opened);
            }
            Err(StateMiniTransportError::RecoveryRequired {
                latest_seq,
                error_description,
                endpoint_transport,
                fallback_reason,
            }) => {
                for handle in handles {
                    handle.abort();
                }
                return Err(StateMiniTransportError::RecoveryRequired {
                    latest_seq,
                    error_description,
                    endpoint_transport,
                    fallback_reason,
                });
            }
            Err(StateMiniTransportError::Transport {
                error_description, ..
            }) => {
                last_transport_error = error_description;
            }
        }
    }

    Err(state_mini_transport_error(after_seq, last_transport_error))
}

async fn open_state_mini_stream_candidate(
    endpoint: ClientEndpoint,
    bearer_token: String,
    mobile_session_header: String,
    after_seq: i64,
    fallback_reason: String,
) -> Result<OpenStateMiniSession, StateMiniTransportError> {
    let endpoint_url = normalized_endpoint_url(&endpoint.url);
    match endpoint.transport {
        ClientEndpointTransport::H2 => {
            open_h2_state_mini_stream_candidate(
                endpoint,
                endpoint_url,
                bearer_token,
                mobile_session_header,
                after_seq,
                fallback_reason,
            )
            .await
        }
        ClientEndpointTransport::H3 => {
            open_h3_state_mini_stream_candidate(
                endpoint,
                endpoint_url,
                bearer_token,
                mobile_session_header,
                after_seq,
            )
            .await
        }
    }
}

async fn open_h2_state_mini_stream_candidate(
    endpoint: ClientEndpoint,
    endpoint_url: String,
    bearer_token: String,
    mobile_session_header: String,
    after_seq: i64,
    fallback_reason: String,
) -> Result<OpenStateMiniSession, StateMiniTransportError> {
    let channel_endpoint = Endpoint::from_shared(endpoint.url)
        .map_err(|error| state_mini_transport_error(after_seq, error.to_string()))?
        .connect_timeout(STATE_MINI_STREAM_CONNECT_TIMEOUT);
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(channel_endpoint)
        .await
        .map_err(|error| state_mini_transport_error(after_seq, error.to_string()))?;
    let (request_sender, request_receiver) = mpsc::channel(64);
    request_sender
        .send(resume_client_frame(after_seq))
        .await
        .map_err(|error| state_mini_transport_error(after_seq, error.to_string()))?;
    let mut request = TonicRequest::new(ReceiverStream::new(request_receiver));
    apply_metadata(request.metadata_mut(), bearer_token, mobile_session_header)
        .map_err(|error| state_mini_transport_error(after_seq, error.to_string()))?;

    let response = client.session(request).await.map_err(|status| {
        if status.code() == tonic::Code::OutOfRange {
            state_mini_recovery_required_error(after_seq, status.message().to_owned())
        } else {
            state_mini_transport_error(after_seq, status.to_string())
        }
    })?;

    Ok(OpenStateMiniSession {
        stream: response.into_inner(),
        request_sender,
        endpoint_url,
        endpoint_transport: ClientEndpointTransport::H2,
        fallback_reason,
    })
}

async fn open_h3_state_mini_stream_candidate(
    endpoint: ClientEndpoint,
    endpoint_url: String,
    bearer_token: String,
    mobile_session_header: String,
    after_seq: i64,
) -> Result<OpenStateMiniSession, StateMiniTransportError> {
    let uri = endpoint_url.parse::<Uri>().map_err(|_| {
        state_mini_transport_error_with_endpoint(
            after_seq,
            ClientCoreError::InvalidEndpoint.to_string(),
            ClientEndpointTransport::H3,
            String::new(),
        )
    })?;
    let client_endpoint = h3_client_endpoint(&endpoint).map_err(|error| {
        state_mini_transport_error_with_endpoint(
            after_seq,
            error.to_string(),
            ClientEndpointTransport::H3,
            String::new(),
        )
    })?;
    let connector = H3QuinnConnector::new(uri.clone(), "localhost".to_owned(), client_endpoint);
    let channel = tonic_h3::H3Channel::new(connector, uri);
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::new(channel);
    let (request_sender, request_receiver) = mpsc::channel(64);
    request_sender
        .send(resume_client_frame(after_seq))
        .await
        .map_err(|error| {
            state_mini_transport_error_with_endpoint(
                after_seq,
                error.to_string(),
                ClientEndpointTransport::H3,
                String::new(),
            )
        })?;
    let mut request = TonicRequest::new(ReceiverStream::new(request_receiver));
    apply_metadata(request.metadata_mut(), bearer_token, mobile_session_header).map_err(
        |error| {
            state_mini_transport_error_with_endpoint(
                after_seq,
                error.to_string(),
                ClientEndpointTransport::H3,
                String::new(),
            )
        },
    )?;

    let response = tokio::time::timeout(STATE_MINI_STREAM_CONNECT_TIMEOUT, client.session(request))
        .await
        .map_err(|_| {
            state_mini_transport_error_with_endpoint(
                after_seq,
                "H3 Session open timed out".to_owned(),
                ClientEndpointTransport::H3,
                String::new(),
            )
        })?
        .map_err(|status| {
            if status.code() == tonic::Code::OutOfRange {
                state_mini_recovery_required_error_with_endpoint(
                    after_seq,
                    status.message().to_owned(),
                    ClientEndpointTransport::H3,
                    String::new(),
                )
            } else {
                state_mini_transport_error_with_endpoint(
                    after_seq,
                    status.to_string(),
                    ClientEndpointTransport::H3,
                    String::new(),
                )
            }
        })?;

    Ok(OpenStateMiniSession {
        stream: response.into_inner(),
        request_sender,
        endpoint_url,
        endpoint_transport: ClientEndpointTransport::H3,
        fallback_reason: String::new(),
    })
}

async fn drive_state_mini_stream_session(
    mut stream: tonic::Streaming<proto::ServerFrame>,
    request_sender: mpsc::Sender<proto::ClientFrame>,
    commands: &mut mpsc::Receiver<OutboundSessionFrame>,
    events: mpsc::Sender<StateMiniStreamEvent>,
    command_acks: mpsc::Sender<ClientCommandAck>,
    after_seq: i64,
    endpoint_url: String,
    endpoint_transport: ClientEndpointTransport,
    fallback_reason: String,
) -> Result<i64, StateMiniTransportError> {
    let mut latest_seq = after_seq;
    events
        .send(StateMiniStreamEvent::Heartbeat {
            latest_seq,
            server_time: String::new(),
            endpoint_url: endpoint_url.clone(),
            endpoint_transport,
            fallback_reason: fallback_reason.clone(),
        })
        .await
        .map_err(|error| {
            state_mini_transport_error_with_endpoint(
                latest_seq,
                error.to_string(),
                endpoint_transport,
                fallback_reason.clone(),
            )
        })?;
    loop {
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else {
                    return Ok(latest_seq);
                };
                let frame = client_frame(command).map_err(|error| {
                    state_mini_transport_error_with_endpoint(
                        latest_seq,
                        error.to_string(),
                        endpoint_transport,
                        fallback_reason.clone(),
                    )
                })?;
                request_sender
                    .send(frame)
                    .await
                    .map_err(|error| {
                        state_mini_transport_error_with_endpoint(
                            latest_seq,
                            error.to_string(),
                            endpoint_transport,
                            fallback_reason.clone(),
                        )
                    })?;
            }
            frame = stream.message() => {
                let Some(frame) = frame.map_err(|status| {
                    if status.code() == tonic::Code::OutOfRange {
                        state_mini_recovery_required_error_with_endpoint(
                            latest_seq,
                            status.message().to_owned(),
                            endpoint_transport,
                            fallback_reason.clone(),
                        )
                    } else {
                        state_mini_transport_error_with_endpoint(
                            latest_seq,
                            status.to_string(),
                            endpoint_transport,
                            fallback_reason.clone(),
                        )
                    }
                })? else {
                    return Ok(latest_seq);
                };

                match frame.frame {
                    Some(proto::server_frame::Frame::Ack(ack)) => {
                        latest_seq = state_mini_data_cursor_after_ack(latest_seq, ack.ack_seq);
                        let ack = client_command_ack(ack);
                        command_acks
                            .send(ack)
                            .await
                            .map_err(|error| {
                                state_mini_transport_error_with_endpoint(
                                    latest_seq,
                                    error.to_string(),
                                    endpoint_transport,
                                    fallback_reason.clone(),
                                )
                            })?;
                    }
                    Some(proto::server_frame::Frame::StateDelta(delta)) => {
                        let delta = client_state_mini_delta(delta)?;
                        latest_seq = state_mini_data_cursor_after_delta(latest_seq, &delta);
                        events
                            .send(StateMiniStreamEvent::Delta(delta))
                            .await
                            .map_err(|error| {
                                state_mini_transport_error_with_endpoint(
                                    latest_seq,
                                    error.to_string(),
                                    endpoint_transport,
                                    fallback_reason.clone(),
                                )
                            })?;
                    }
                    Some(proto::server_frame::Frame::TextChunk(text_chunk)) => {
                        let text_chunk = client_text_chunk(text_chunk);
                        latest_seq = state_mini_data_cursor_after_text_chunk(
                            latest_seq,
                            text_chunk.seq,
                        );
                        events
                            .send(StateMiniStreamEvent::TextChunk(text_chunk))
                            .await
                            .map_err(|error| {
                                state_mini_transport_error_with_endpoint(
                                    latest_seq,
                                    error.to_string(),
                                    endpoint_transport,
                                    fallback_reason.clone(),
                                )
                            })?;
                    }
                    Some(proto::server_frame::Frame::Heartbeat(heartbeat)) => {
                        latest_seq =
                            state_mini_data_cursor_after_heartbeat(latest_seq, heartbeat.latest_seq);
                        events
                            .send(StateMiniStreamEvent::Heartbeat {
                                latest_seq: heartbeat.latest_seq,
                                server_time: heartbeat.server_time,
                                endpoint_url: endpoint_url.clone(),
                                endpoint_transport,
                                fallback_reason: fallback_reason.clone(),
                            })
                            .await
                            .map_err(|error| {
                                state_mini_transport_error_with_endpoint(
                                    latest_seq,
                                    error.to_string(),
                                    endpoint_transport,
                                    fallback_reason.clone(),
                                )
                            })?;
                    }
                    _ => {}
                }
            }
        }
    }
}

fn state_mini_data_cursor_after_ack(current_seq: i64, ack_seq: i64) -> i64 {
    current_seq.max(ack_seq)
}

fn state_mini_data_cursor_after_delta(current_seq: i64, delta: &ClientStateMiniDelta) -> i64 {
    if is_state_mini_bulk_delta(delta) && !is_state_mini_replacement_complete_delta(delta) {
        current_seq
    } else {
        current_seq.max(delta.seq)
    }
}

fn state_mini_data_cursor_after_text_chunk(current_seq: i64, text_chunk_seq: i64) -> i64 {
    current_seq.max(text_chunk_seq)
}

fn state_mini_data_cursor_after_heartbeat(current_seq: i64, heartbeat_seq: i64) -> i64 {
    current_seq.max(heartbeat_seq)
}

fn is_state_mini_bulk_delta(delta: &ClientStateMiniDelta) -> bool {
    !delta.has_session && (delta.kind == STATE_MINI_REPLACEMENT_KIND || !delta.sessions.is_empty())
}

fn is_state_mini_replacement_complete_delta(delta: &ClientStateMiniDelta) -> bool {
    !delta.has_session
        && matches!(
            delta.kind.as_str(),
            STATE_MINI_REPLACEMENT_COMPLETE_KIND | STATE_MINI_BATCH_COMPLETE_KIND
        )
}

fn state_mini_stream_ended_event(latest_seq: i64) -> StateMiniStreamEvent {
    StateMiniStreamEvent::Reconnecting {
        latest_seq,
        error_description: STATE_MINI_STREAM_ENDED.to_owned(),
        endpoint_transport: ClientEndpointTransport::H2,
        fallback_reason: String::new(),
    }
}

fn session_transport_endpoints(
    endpoints: &[ClientEndpoint],
) -> Result<Vec<ClientEndpoint>, ClientCoreError> {
    ordered_client_endpoints(endpoints)
}

fn snapshot_recovery_endpoints(
    endpoints: &[ClientEndpoint],
) -> Result<Vec<ClientEndpoint>, ClientCoreError> {
    ordered_client_endpoints(endpoints)
}

fn ordered_client_endpoints(
    endpoints: &[ClientEndpoint],
) -> Result<Vec<ClientEndpoint>, ClientCoreError> {
    if endpoints.is_empty() {
        return Err(ClientCoreError::NoEndpoint);
    }

    let mut seen_endpoints = HashSet::new();
    let mut ordered = Vec::with_capacity(endpoints.len());
    for (prefer_transport, prefer_last_good) in [
        (ClientEndpointTransport::H3, true),
        (ClientEndpointTransport::H3, false),
        (ClientEndpointTransport::H2, true),
        (ClientEndpointTransport::H2, false),
    ] {
        for endpoint in endpoints.iter().filter(|endpoint| {
            endpoint.transport == prefer_transport && endpoint.last_good == prefer_last_good
        }) {
            let normalized_url = endpoint.url.trim().trim_end_matches('/').to_owned();
            if seen_endpoints.insert((endpoint.transport, normalized_url)) {
                ordered.push(endpoint.clone());
            }
        }
    }

    Ok(ordered)
}

fn session_transport_tiers(candidates: Vec<ClientEndpoint>) -> Vec<Vec<ClientEndpoint>> {
    let h3 = candidates
        .iter()
        .filter(|endpoint| endpoint.transport == ClientEndpointTransport::H3)
        .cloned()
        .collect::<Vec<_>>();
    let h2 = candidates
        .into_iter()
        .filter(|endpoint| endpoint.transport == ClientEndpointTransport::H2)
        .collect::<Vec<_>>();
    [h3, h2]
        .into_iter()
        .filter(|tier| !tier.is_empty())
        .collect()
}

fn mark_endpoint_last_good(
    endpoints: &mut [ClientEndpoint],
    endpoint_url: &str,
    transport: ClientEndpointTransport,
) {
    let endpoint_url = normalized_endpoint_url(endpoint_url);
    for endpoint in endpoints {
        endpoint.last_good = endpoint.transport == transport
            && normalized_endpoint_url(&endpoint.url) == endpoint_url;
    }
}

fn normalized_endpoint_url(endpoint_url: &str) -> String {
    endpoint_url.trim().trim_end_matches('/').to_owned()
}

fn state_mini_snapshot_uri(endpoint: &ClientEndpoint) -> Result<Uri, ClientCoreError> {
    let recovery_base_url = state_mini_snapshot_base_url(endpoint)?;
    format!("{recovery_base_url}{STATE_MINI_SNAPSHOT_PATH}")
        .parse::<Uri>()
        .map_err(|_| ClientCoreError::InvalidEndpoint)
}

fn state_mini_snapshot_base_url(endpoint: &ClientEndpoint) -> Result<String, ClientCoreError> {
    let explicit = endpoint.recovery_base_url.trim().trim_end_matches('/');
    if !explicit.is_empty() {
        return Ok(explicit.to_owned());
    }
    if endpoint.transport == ClientEndpointTransport::H3 {
        return Err(ClientCoreError::InvalidEndpoint);
    }
    let endpoint_url = endpoint.url.trim().trim_end_matches('/');
    if endpoint_url.is_empty() {
        return Err(ClientCoreError::InvalidEndpoint);
    }
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

fn h3_client_endpoint(
    endpoint: &ClientEndpoint,
) -> Result<tonic_h3::quinn::h3_quinn::Endpoint, ClientCoreError> {
    let cert_pin = normalized_sha256_pin(&endpoint.h3_certificate_sha256)?;
    let spki_pin = normalized_sha256_pin(&endpoint.h3_certificate_spki_sha256)?;
    if cert_pin.is_none() && spki_pin.is_none() {
        return Err(ClientCoreError::InvalidEndpoint);
    }

    let mut client_endpoint = tonic_h3::quinn::h3_quinn::Endpoint::client(
        "0.0.0.0:0"
            .parse::<SocketAddr>()
            .map_err(|_| ClientCoreError::InvalidEndpoint)?,
    )
    .map_err(|_| ClientCoreError::StateMiniStreamTransportFailed)?;
    let tls_config = h3_client_tls_config(cert_pin, spki_pin)?;
    let quic_config =
        QuicClientConfig::try_from(tls_config).map_err(|_| ClientCoreError::InvalidEndpoint)?;
    client_endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_config)));
    Ok(client_endpoint)
}

fn h3_client_tls_config(
    cert_pin: Option<String>,
    spki_pin: Option<String>,
) -> Result<quinn_rustls::ClientConfig, ClientCoreError> {
    let provider = quinn_rustls::crypto::ring::default_provider();
    let verifier = Arc::new(PinnedH3CertificateVerifier {
        cert_pin,
        spki_pin,
        supported: provider.signature_verification_algorithms,
    });
    let mut tls_config = quinn_rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&quinn_rustls::version::TLS13])
        .map_err(|_| ClientCoreError::InvalidEndpoint)?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    Ok(tls_config)
}

#[derive(Debug)]
struct PinnedH3CertificateVerifier {
    cert_pin: Option<String>,
    spki_pin: Option<String>,
    supported: quinn_rustls::crypto::WebPkiSupportedAlgorithms,
}

impl quinn_rustls::client::danger::ServerCertVerifier for PinnedH3CertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &quinn_rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[quinn_rustls::pki_types::CertificateDer<'_>],
        _server_name: &quinn_rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: quinn_rustls::pki_types::UnixTime,
    ) -> Result<quinn_rustls::client::danger::ServerCertVerified, quinn_rustls::Error> {
        let cert_sha256 = sha256_hex(end_entity.as_ref());
        if self.cert_pin.as_deref() == Some(cert_sha256.as_str()) {
            return Ok(quinn_rustls::client::danger::ServerCertVerified::assertion());
        }
        if let Some(expected_spki_pin) = self.spki_pin.as_deref() {
            let spki_sha256 = certificate_spki_sha256(end_entity.as_ref())?;
            if expected_spki_pin == spki_sha256 {
                return Ok(quinn_rustls::client::danger::ServerCertVerified::assertion());
            }
        }
        Err(quinn_rustls::Error::InvalidCertificate(
            quinn_rustls::CertificateError::ApplicationVerificationFailure,
        ))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &quinn_rustls::pki_types::CertificateDer<'_>,
        dss: &quinn_rustls::DigitallySignedStruct,
    ) -> Result<quinn_rustls::client::danger::HandshakeSignatureValid, quinn_rustls::Error> {
        quinn_rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &quinn_rustls::pki_types::CertificateDer<'_>,
        dss: &quinn_rustls::DigitallySignedStruct,
    ) -> Result<quinn_rustls::client::danger::HandshakeSignatureValid, quinn_rustls::Error> {
        quinn_rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<quinn_rustls::SignatureScheme> {
        self.supported.supported_schemes()
    }
}

fn certificate_spki_sha256(certificate_der: &[u8]) -> Result<String, quinn_rustls::Error> {
    let (_, certificate) = X509Certificate::from_der(certificate_der).map_err(|_| {
        quinn_rustls::Error::InvalidCertificate(quinn_rustls::CertificateError::BadEncoding)
    })?;
    Ok(sha256_hex(certificate.tbs_certificate.subject_pki.raw))
}

fn normalized_sha256_pin(pin: &str) -> Result<Option<String>, ClientCoreError> {
    let pin = pin.trim();
    if pin.is_empty() {
        return Ok(None);
    }
    let hex = pin.strip_prefix("sha256:").unwrap_or(pin);
    if hex.len() != 64 || !hex.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(ClientCoreError::InvalidEndpoint);
    }
    Ok(Some(hex.to_ascii_lowercase()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
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
        ClientCommandKind::SetDefaultNotificationTargets => {
            Ok(proto::command::Command::SetDefaultNotificationTargets(
                proto::SetDefaultNotificationTargetsRequest {
                    notification_target_ids: frame.notification_target_ids,
                    client_mutation_id: frame.client_mutation_id,
                },
            ))
        }
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
        | ClientCommandKind::SetSiriCurrentSession
        | ClientCommandKind::SetSiriDefaultSession
        | ClientCommandKind::SaveDefaultPrompt
        | ClientCommandKind::SetDefaultNotificationTargets
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
        account_id: ack.account_id,
        node_id: ack.node_id,
        client_mutation_id: ack.client_mutation_id,
        ack_seq: ack.ack_seq,
        entity_id: ack.entity_id,
        revision: ack.revision,
        server_time: ack.server_time,
        idempotent_replay: ack.idempotent_replay,
        error_code: ack.error_code,
        reject_reason: ack.reject_reason,
        current_state: ack.current_state,
    }
}

fn client_text_chunk(text_chunk: proto::TextChunk) -> ClientTextChunk {
    ClientTextChunk {
        seq: text_chunk.seq,
        thread_id: text_chunk.thread_id,
        message_id: text_chunk.message_id,
        content: text_chunk.content,
        is_final: text_chunk.is_final,
        server_time: text_chunk.server_time,
    }
}

fn client_state_mini_delta(
    delta: proto::StateMiniDelta,
) -> Result<ClientStateMiniDelta, StateMiniTransportError> {
    let payload = serde_json::from_str::<Value>(&delta.payload_json).ok();
    let Some(payload) = payload else {
        return Ok(seq_only_state_mini_delta(delta));
    };
    if state_mini_payload_requires_snapshot_recovery(&payload) {
        return Err(state_mini_recovery_required_error(
            state_mini_payload_latest_seq(&payload, delta.seq),
            state_mini_payload_recovery_reason(&payload),
        ));
    }
    let replace_sessions = payload
        .get(REPLACE_FIELD)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let replacement_complete = payload
        .get(REPLACEMENT_COMPLETE_FIELD)
        .or_else(|| payload.get(REPLACEMENT_COMPLETE_ALIAS_FIELD))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if replace_sessions {
        let sessions = state_mini_payload_sessions(&payload);
        return Ok(ClientStateMiniDelta {
            seq: delta.seq,
            latest_seq: state_mini_payload_latest_seq(&payload, delta.seq),
            entity_id: delta.entity_id,
            kind: if replacement_complete {
                STATE_MINI_REPLACEMENT_COMPLETE_KIND
            } else {
                STATE_MINI_REPLACEMENT_KIND
            }
            .to_owned(),
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
    let sessions = state_mini_payload_sessions(&payload);
    if !sessions.is_empty() {
        return Ok(ClientStateMiniDelta {
            seq: delta.seq,
            latest_seq: state_mini_payload_latest_seq(&payload, delta.seq),
            entity_id: delta.entity_id,
            kind: if replacement_complete {
                STATE_MINI_BATCH_COMPLETE_KIND.to_owned()
            } else {
                delta.kind
            },
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

fn state_mini_payload_requires_snapshot_recovery(payload: &Value) -> bool {
    let recovery_required = payload
        .get(RECOVERY_REQUIRED_FIELD)
        .or_else(|| payload.get(RECOVERY_REQUIRED_ALIAS_FIELD))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let recovery_instruction = payload
        .get(RECOVERY_FIELD)
        .and_then(Value::as_str)
        .unwrap_or_default();

    recovery_required && recovery_instruction == RECOVERY_INSTRUCTION_STATE_MINI_SNAPSHOT
}

fn state_mini_payload_recovery_reason(payload: &Value) -> String {
    payload
        .get(REASON_FIELD)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .unwrap_or(DEFAULT_RECOVERY_REQUIRED_REASON)
        .to_owned()
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
    use std::{
        io::{Read, Write},
        net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
        sync::{Arc, Mutex},
        thread,
        time::Instant,
    };

    use rcgen::generate_simple_self_signed;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use tonic_h3::quinn::H3QuinnAcceptor;
    use tonic_h3::quinn::h3_quinn::Endpoint as H3Endpoint;
    use tonic_h3::quinn::h3_quinn::quinn::{
        ServerConfig, VarInt, crypto::rustls::QuicServerConfig,
    };

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
        let url = state_mini_snapshot_uri(&h2_endpoint("http://127.0.0.1:8766/base/", false))
            .expect("snapshot url");

        assert_eq!(
            url.to_string(),
            "http://127.0.0.1:8765/base/api/mobile/session-minis/snapshot"
        );
    }

    #[test]
    fn state_mini_snapshot_url_keeps_non_realtime_ports() {
        let url = state_mini_snapshot_uri(&h2_endpoint("https://100.119.200.69:8781/", false))
            .expect("snapshot url");

        assert_eq!(
            url.to_string(),
            "https://100.119.200.69:8781/api/mobile/session-minis/snapshot"
        );
    }

    #[test]
    fn snapshot_recovery_endpoints_keep_fallbacks_after_last_good() {
        let endpoints = snapshot_recovery_endpoints(&[
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://100.119.200.69:8765".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            },
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://192.168.1.33:8765".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: true,
            },
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://192.168.1.33:8765/".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            },
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://127.0.0.1:8765".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
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
    fn state_mini_snapshot_recovery_uses_first_ready_endpoint() {
        let (slow_url, slow_server) =
            spawn_snapshot_server(Duration::from_millis(900), 7, "thread-slow");
        let (fast_url, fast_server) =
            spawn_snapshot_server(Duration::from_millis(0), 42, "thread-fast");
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        let start = Instant::now();
        let recovered = runtime
            .block_on(fetch_state_mini_snapshot(
                vec![
                    ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: slow_url,
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: true,
                    },
                    ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: fast_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    },
                ],
                String::new(),
                String::new(),
            ))
            .expect("first ready snapshot");

        assert_eq!(recovered.endpoint_url, fast_url);
        assert_eq!(recovered.snapshot.latest_seq, 42);
        assert_eq!(recovered.snapshot.sessions[0].session_id, "thread-fast");
        assert!(start.elapsed() < Duration::from_millis(500));
        let _ = slow_server.join();
        let _ = fast_server.join();
    }

    #[test]
    fn state_mini_snapshot_recovery_rejects_oversized_body() {
        let (url, server) = spawn_raw_snapshot_server(
            Duration::from_millis(0),
            "x".repeat(MAX_STATE_MINI_SNAPSHOT_BYTES + 1),
        );
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        let error = runtime
            .block_on(fetch_state_mini_snapshot(
                vec![ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: true,
                }],
                String::new(),
                String::new(),
            ))
            .expect_err("oversized snapshot rejects");

        assert_eq!(error, ClientCoreError::StateMiniSnapshotTransportFailed);
        let _ = server.join();
    }

    #[test]
    fn state_mini_delta_recovery_instruction_triggers_snapshot_recovery() {
        let result = client_state_mini_delta(proto::StateMiniDelta {
            seq: 57,
            entity_id: "mobile-state".to_owned(),
            kind: "state_mini".to_owned(),
            revision: "rev-57".to_owned(),
            server_time: "2026-06-30T00:00:00Z".to_owned(),
            payload_json: json!({
                "controlOnly": true,
                "reason": "state_delta_frame_cap_exceeded",
                "entityId": "mobile-state",
                "kind": "state_mini",
                "latestSeq": 57,
                "recoveryRequired": true,
                "recovery": "session-mini-snapshot"
            })
            .to_string(),
        });

        match result {
            Err(StateMiniTransportError::RecoveryRequired {
                latest_seq,
                error_description,
                ..
            }) => {
                assert_eq!(latest_seq, 57);
                assert_eq!(error_description, "state_delta_frame_cap_exceeded");
            }
            Err(other) => panic!("expected recovery-required error, got {other:?}"),
            Ok(delta) => panic!(
                "expected recovery-required error, got delta seq {}",
                delta.seq
            ),
        }
    }

    #[test]
    fn session_transport_endpoints_keep_fallbacks_after_last_good() {
        let endpoints = session_transport_endpoints(&[
            ClientEndpoint {
                transport: ClientEndpointTransport::H2,
                url: "http://100.119.200.69:8766".to_owned(),
                recovery_base_url: "http://100.119.200.69:8765".to_owned(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            },
            ClientEndpoint {
                transport: ClientEndpointTransport::H2,
                url: "http://192.168.1.33:8766".to_owned(),
                recovery_base_url: "http://192.168.1.33:8765".to_owned(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: true,
            },
            ClientEndpoint {
                transport: ClientEndpointTransport::H2,
                url: "http://192.168.1.33:8766/".to_owned(),
                recovery_base_url: "http://192.168.1.33:8765".to_owned(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            },
            ClientEndpoint {
                transport: ClientEndpointTransport::H2,
                url: "http://127.0.0.1:8766".to_owned(),
                recovery_base_url: "http://127.0.0.1:8765".to_owned(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
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
                "http://192.168.1.33:8766",
                "http://100.119.200.69:8766",
                "http://127.0.0.1:8766",
            ]
        );
    }

    #[test]
    fn h3_endpoint_transport_order_prefers_h3_before_h2_and_last_good_h3_first() {
        let endpoints = session_transport_endpoints(&[
            h2_endpoint("http://127.0.0.1:8766", true),
            h3_endpoint(
                "https://127.0.0.1:8766",
                "http://127.0.0.1:8765",
                "sha256:01",
                false,
            ),
            h3_endpoint(
                "https://100.64.0.2:8766",
                "http://100.64.0.2:8765",
                "sha256:02",
                true,
            ),
            h2_endpoint("http://100.64.0.2:8766", false),
        ])
        .expect("endpoints");

        let ordered = endpoints
            .into_iter()
            .map(|endpoint| (endpoint.transport, endpoint.url))
            .collect::<Vec<_>>();
        assert_eq!(
            ordered,
            vec![
                (
                    ClientEndpointTransport::H3,
                    "https://100.64.0.2:8766".to_owned()
                ),
                (
                    ClientEndpointTransport::H3,
                    "https://127.0.0.1:8766".to_owned()
                ),
                (
                    ClientEndpointTransport::H2,
                    "http://127.0.0.1:8766".to_owned()
                ),
                (
                    ClientEndpointTransport::H2,
                    "http://100.64.0.2:8766".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn state_mini_snapshot_recovery_uses_explicit_recovery_base_url_for_h3() {
        let url = state_mini_snapshot_uri(&h3_endpoint(
            "https://127.0.0.1:8766/realtime",
            "http://127.0.0.1:9876/mobile",
            "sha256:01",
            true,
        ))
        .expect("snapshot url");

        assert_eq!(
            url.to_string(),
            "http://127.0.0.1:9876/mobile/api/mobile/session-minis/snapshot"
        );
    }

    #[test]
    fn h3_rejects_missing_or_malformed_certificate_pin_material() {
        let missing_pin = h3_endpoint("https://127.0.0.1:8766", "http://127.0.0.1:8765", "", false);
        assert_eq!(
            h3_client_endpoint(&missing_pin).expect_err("missing H3 pin rejects"),
            ClientCoreError::InvalidEndpoint
        );

        let malformed_cert_pin = h3_endpoint(
            "https://127.0.0.1:8766",
            "http://127.0.0.1:8765",
            "sha256:not-hex",
            false,
        );
        assert_eq!(
            h3_client_endpoint(&malformed_cert_pin).expect_err("malformed H3 cert pin rejects"),
            ClientCoreError::InvalidEndpoint
        );

        let malformed_spki_pin = ClientEndpoint {
            transport: ClientEndpointTransport::H3,
            url: "https://127.0.0.1:8766".to_owned(),
            recovery_base_url: "http://127.0.0.1:8765".to_owned(),
            h3_certificate_sha256: String::new(),
            h3_certificate_spki_sha256: "sha256:short".to_owned(),
            last_good: false,
        };
        assert_eq!(
            h3_client_endpoint(&malformed_spki_pin).expect_err("malformed H3 SPKI pin rejects"),
            ClientCoreError::InvalidEndpoint
        );
        println!(
            "manual_qa_malformed_h3_pin missing_pin=reject malformed_cert=reject malformed_spki=reject"
        );
    }

    #[test]
    fn h3_snapshot_recovery_rejects_missing_explicit_recovery_base_url() {
        let error = state_mini_snapshot_uri(&h3_endpoint(
            "https://127.0.0.1:8766/realtime",
            "",
            "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            true,
        ))
        .expect_err("H3 recovery requires explicit HTTP base URL");

        assert_eq!(error, ClientCoreError::InvalidEndpoint);
    }

    #[test]
    fn successful_session_endpoint_becomes_next_last_good() {
        let mut endpoints = vec![
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://100.119.200.69:8766".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: true,
            },
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://192.168.1.33:8766/".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            },
            ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: "http://127.0.0.1:8766".to_owned(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            },
        ];

        mark_endpoint_last_good(
            &mut endpoints,
            " http://192.168.1.33:8766 ",
            ClientEndpointTransport::H2,
        );

        assert!(!endpoints[0].last_good);
        assert!(endpoints[1].last_good);
        assert!(!endpoints[2].last_good);
        let urls = session_transport_endpoints(&endpoints)
            .expect("endpoints")
            .into_iter()
            .map(|endpoint| endpoint.url)
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            vec![
                "http://192.168.1.33:8766/",
                "http://100.119.200.69:8766",
                "http://127.0.0.1:8766",
            ]
        );
    }

    #[test]
    fn session_stream_falls_through_stale_last_good_and_marks_fallback() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let (fallback_url, server) = spawn_realtime_session_server().await;
            let stale_url = unused_local_url();
            let mut endpoints = vec![
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: stale_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: true,
                },
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: fallback_url.clone(),
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
            ];
            let (command_sender, mut commands) = mpsc::channel(1);
            drop(command_sender);
            let (events_sender, mut events) = mpsc::channel(2);
            let (command_acks_sender, _command_acks) = mpsc::channel(1);

            let latest_seq = run_state_mini_stream_session(
                &mut endpoints,
                "",
                "",
                23,
                &mut commands,
                events_sender,
                command_acks_sender,
            )
            .await
            .expect("fallback session connects");

            assert_eq!(latest_seq, 23);
            let event = events.recv().await.expect("opened endpoint event");
            match event {
                StateMiniStreamEvent::Heartbeat {
                    endpoint_url,
                    latest_seq,
                    ..
                } => {
                    assert_eq!(endpoint_url, normalized_endpoint_url(&fallback_url));
                    assert_eq!(latest_seq, 23);
                }
                other => panic!("expected endpoint heartbeat, got {other:?}"),
            }
            assert!(!endpoints[0].last_good);
            assert!(endpoints[1].last_good);

            server.abort();
            let _ = server.await;
        });
    }

    #[test]
    fn h3_session_stream_uses_quinn() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let h3 = spawn_h3_realtime_session_server(81).await;
            let mut endpoints = vec![h3_endpoint(
                &h3.url,
                "http://127.0.0.1:8765",
                &h3.certificate_sha256,
                false,
            )];
            let (command_sender, mut commands) = mpsc::channel(1);
            let (events_sender, mut events) = mpsc::channel(4);
            let (command_acks_sender, _command_acks) = mpsc::channel(1);
            let session_task = tokio::spawn(async move {
                run_state_mini_stream_session(
                    &mut endpoints,
                    "",
                    "",
                    80,
                    &mut commands,
                    events_sender,
                    command_acks_sender,
                )
                .await
            });

            let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .expect("H3 event timeout")
                .expect("opened endpoint event");
            match event {
                StateMiniStreamEvent::Heartbeat {
                    endpoint_url,
                    endpoint_transport,
                    latest_seq,
                    fallback_reason,
                    ..
                } => {
                    assert_eq!(endpoint_url, normalized_endpoint_url(&h3.url));
                    assert_eq!(endpoint_transport, ClientEndpointTransport::H3);
                    assert_eq!(latest_seq, 80);
                    assert!(fallback_reason.is_empty());
                    println!(
                        "manual_qa_h3_success transport=h3 endpoint={} latest_seq={}",
                        endpoint_url, latest_seq
                    );
                }
                other => panic!("expected H3 endpoint heartbeat, got {other:?}"),
            }

            drop(command_sender);
            session_task.abort();
            let _ = session_task.await;
            h3.shutdown().await;
        });
    }

    #[test]
    fn h3_failure_falls_back_to_h2_without_losing_resume_or_ack() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let dead_h3 = reserve_dead_udp_url().await;
            let (h2_url, server, observed_resume) =
                spawn_realtime_session_server_with_ack(88, "mutation-fallback").await;
            let mut endpoints = vec![
                h3_endpoint(
                    &dead_h3,
                    "http://127.0.0.1:8765",
                    "sha256:0000000000000000000000000000000000000000000000000000000000000000",
                    false,
                ),
                h2_endpoint(&h2_url, false),
            ];
            let (command_sender, mut commands) = mpsc::channel(2);
            command_sender
                .send(test_outbound_command("mutation-fallback", 77))
                .await
                .expect("queue command");
            let (events_sender, mut events) = mpsc::channel(4);
            let (command_acks_sender, mut command_acks) = mpsc::channel(2);

            let latest_seq = run_state_mini_stream_session(
                &mut endpoints,
                "",
                "",
                77,
                &mut commands,
                events_sender,
                command_acks_sender,
            )
            .await
            .expect("H2 fallback session connects");

            assert_eq!(latest_seq, 88);
            assert_eq!(
                observed_resume.lock().expect("resume lock").as_slice(),
                &[77],
                "fallback must preserve Resume after_seq"
            );
            let event = events.recv().await.expect("fallback endpoint event");
            match event {
                StateMiniStreamEvent::Heartbeat {
                    endpoint_url,
                    endpoint_transport,
                    fallback_reason,
                    latest_seq,
                    ..
                } => {
                    assert_eq!(endpoint_url, normalized_endpoint_url(&h2_url));
                    assert_eq!(endpoint_transport, ClientEndpointTransport::H2);
                    assert_eq!(latest_seq, 77);
                    assert!(
                        fallback_reason.contains("h3"),
                        "fallback reason should identify H3 failure: {fallback_reason}"
                    );
                    println!(
                        "manual_qa_h3_fallback transport=h2 fallback_reason={fallback_reason} resume_after_seq=77 ack_seq=88"
                    );
                }
                other => panic!("expected H2 fallback heartbeat, got {other:?}"),
            }
            let ack = command_acks.recv().await.expect("fallback command ack");
            assert_eq!(ack.client_mutation_id, "mutation-fallback");
            assert_eq!(ack.ack_seq, 88);
            assert!(!endpoints[0].last_good);
            assert!(endpoints[1].last_good);

            server.abort();
            let _ = server.await;
        });
    }

    #[test]
    fn h3_established_stream_failure_reconnects_with_after_seq_and_preserves_pending_ack() {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");

        runtime.block_on(async {
            let h3 =
                spawn_h3_realtime_session_server_fail_then_ack(91, 95, "mutation-after-h3-failure")
                    .await;
            let (h2_url, h2_server, h2_observed_resume) =
                spawn_realtime_session_server_with_ack(95, "mutation-after-h3-failure").await;
            let endpoints = vec![
                h3_endpoint(
                    &h3.url,
                    "http://127.0.0.1:8765",
                    &h3.certificate_sha256,
                    false,
                ),
                h2_endpoint(&h2_url, false),
            ];
            let (command_sender, commands) = mpsc::channel(2);
            let (events_sender, mut events) = mpsc::channel(8);
            let (command_acks_sender, mut command_acks) = mpsc::channel(2);
            let stream_task = tokio::spawn(async move {
                run_state_mini_stream(
                    endpoints,
                    String::new(),
                    String::new(),
                    90,
                    commands,
                    events_sender,
                    command_acks_sender,
                )
                .await;
            });

            let opened = recv_stream_event(&mut events, "initial H3 heartbeat").await;
            match opened {
                StateMiniStreamEvent::Heartbeat {
                    latest_seq,
                    endpoint_transport,
                    fallback_reason,
                    ..
                } => {
                    assert_eq!(latest_seq, 90);
                    assert_eq!(endpoint_transport, ClientEndpointTransport::H3);
                    assert!(fallback_reason.is_empty());
                }
                other => panic!("expected initial H3 heartbeat, got {other:?}"),
            }

            let mut expected_resume_after_seq = 90;
            let reconnecting = loop {
                let event = recv_stream_event(&mut events, "H3 reconnecting event").await;
                match event {
                    StateMiniStreamEvent::Heartbeat {
                        latest_seq,
                        endpoint_transport,
                        ..
                    } => {
                        assert_eq!(endpoint_transport, ClientEndpointTransport::H3);
                        expected_resume_after_seq = expected_resume_after_seq.max(latest_seq);
                    }
                    other @ StateMiniStreamEvent::Reconnecting { .. } => break other,
                    other => panic!("expected H3 heartbeat or reconnecting event, got {other:?}"),
                }
            };
            match reconnecting {
                StateMiniStreamEvent::Reconnecting {
                    latest_seq,
                    endpoint_transport,
                    fallback_reason,
                    error_description,
                } => {
                    assert_eq!(latest_seq, expected_resume_after_seq);
                    assert_eq!(endpoint_transport, ClientEndpointTransport::H3);
                    assert!(fallback_reason.is_empty());
                    assert!(
                        error_description.contains("Connection error")
                            || error_description.contains("established h3 stream failed"),
                        "unexpected reconnect reason: {error_description}"
                    );
                }
                other => panic!("expected H3 reconnecting event, got {other:?}"),
            }

            h3.endpoint.close(VarInt::from_u32(0), b"test h3 down");
            command_sender
                .send(test_outbound_command(
                    "mutation-after-h3-failure",
                    expected_resume_after_seq,
                ))
                .await
                .expect("queue command across reconnect");

            let reopened = recv_stream_event(&mut events, "fallback H2 heartbeat").await;
            match reopened {
                StateMiniStreamEvent::Heartbeat {
                    latest_seq,
                    endpoint_transport,
                    fallback_reason,
                    endpoint_url,
                    ..
                } => {
                    assert_eq!(latest_seq, expected_resume_after_seq);
                    assert_eq!(endpoint_url, normalized_endpoint_url(&h2_url));
                    assert_eq!(endpoint_transport, ClientEndpointTransport::H2);
                    assert!(
                        fallback_reason.contains("h3"),
                        "fallback reason should identify H3 reconnect failure: {fallback_reason}"
                    );
                }
                other => panic!("expected fallback H2 heartbeat, got {other:?}"),
            }

            let ack = tokio::time::timeout(Duration::from_secs(3), command_acks.recv())
                .await
                .expect("ack timeout")
                .expect("ack after H3 reconnect");
            assert_eq!(ack.client_mutation_id, "mutation-after-h3-failure");
            assert_eq!(ack.ack_seq, 95);

            let resumes = wait_for_resume_count(h2_observed_resume, 1).await;
            assert_eq!(
                resumes.last().copied(),
                Some(expected_resume_after_seq),
                "H3 reconnect must preserve Resume after_seq from the failed established stream"
            );
            assert!(
                !resumes.contains(&0),
                "H3 reconnect must not reset Resume after_seq to zero: {resumes:?}"
            );
            println!(
                "manual_qa_h3_established_reconnect transport=h3_then_h2 resumes={resumes:?} ack_seq={}",
                ack.ack_seq
            );

            stream_task.abort();
            let _ = stream_task.await;
            drop(command_sender);
            h2_server.abort();
            let _ = h2_server.await;
            h3.shutdown().await;
        });
    }

    fn h2_endpoint(url: &str, last_good: bool) -> ClientEndpoint {
        ClientEndpoint {
            transport: ClientEndpointTransport::H2,
            url: url.to_owned(),
            recovery_base_url: url.replace(":8766", ":8765"),
            h3_certificate_sha256: String::new(),
            h3_certificate_spki_sha256: String::new(),
            last_good,
        }
    }

    fn h3_endpoint(
        url: &str,
        recovery_base_url: &str,
        certificate_sha256: &str,
        last_good: bool,
    ) -> ClientEndpoint {
        ClientEndpoint {
            transport: ClientEndpointTransport::H3,
            url: url.to_owned(),
            recovery_base_url: recovery_base_url.to_owned(),
            h3_certificate_sha256: certificate_sha256.to_owned(),
            h3_certificate_spki_sha256: String::new(),
            last_good,
        }
    }

    fn spawn_snapshot_server(
        delay: Duration,
        latest_seq: i64,
        session_id: &'static str,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let url = format!("http://{}", listener.local_addr().expect("server addr"));
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            thread::sleep(delay);
            let body = json!({
                "latestSeq": latest_seq,
                "serverTime": "2026-06-28T00:00:00Z",
                "sessions": [
                    {
                        "sessionId": session_id,
                        "assistantSurface": "codex",
                        "seq": latest_seq,
                        "revision": format!("rev-{latest_seq}"),
                        "title": session_id,
                    }
                ]
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (url, handle)
    }

    fn spawn_raw_snapshot_server(
        delay: Duration,
        body: String,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let url = format!("http://{}", listener.local_addr().expect("server addr"));
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            thread::sleep(delay);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (url, handle)
    }

    fn unused_local_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused port");
        let address = listener.local_addr().expect("unused addr");
        drop(listener);
        format!("http://{address}")
    }

    async fn spawn_realtime_session_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind realtime server port");
        let address = listener.local_addr().expect("realtime server addr");
        drop(listener);
        let handle = tokio::spawn(async move {
            let service = proto::looper_realtime_server::LooperRealtimeServer::new(
                TestRealtimeSessionService::default(),
            );
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(address)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        (format!("http://{address}"), handle)
    }

    async fn spawn_realtime_session_server_with_ack(
        ack_seq: i64,
        client_mutation_id: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>, Arc<Mutex<Vec<i64>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind realtime server port");
        let address = listener.local_addr().expect("realtime server addr");
        drop(listener);
        let observed_resume = Arc::new(Mutex::new(Vec::new()));
        let service = TestRealtimeSessionService {
            heartbeat_seq: None,
            ack_seq: Some(ack_seq),
            ack_client_mutation_id: client_mutation_id.to_owned(),
            observed_resume: observed_resume.clone(),
        };
        let handle = tokio::spawn(async move {
            let service = proto::looper_realtime_server::LooperRealtimeServer::new(service);
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(address)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        (format!("http://{address}"), handle, observed_resume)
    }

    struct SpawnedH3RealtimeSessionServer {
        url: String,
        certificate_sha256: String,
        endpoint: H3Endpoint,
        server_task: tokio::task::JoinHandle<Result<(), tonic_h3::Error>>,
    }

    impl SpawnedH3RealtimeSessionServer {
        async fn shutdown(self) {
            self.endpoint.close(VarInt::from_u32(0), b"test shutdown");
            self.endpoint.wait_idle().await;
            let _ = self.server_task.await;
        }
    }

    async fn spawn_h3_realtime_session_server(
        heartbeat_seq: i64,
    ) -> SpawnedH3RealtimeSessionServer {
        let observed_resume = Arc::new(Mutex::new(Vec::new()));
        let service = TestRealtimeSessionService {
            heartbeat_seq: Some(heartbeat_seq),
            ack_seq: None,
            ack_client_mutation_id: String::new(),
            observed_resume: observed_resume.clone(),
        };
        spawn_h3_realtime_session_server_with_service(service).await
    }

    async fn spawn_h3_realtime_session_server_fail_then_ack(
        heartbeat_seq: i64,
        ack_seq: i64,
        client_mutation_id: &'static str,
    ) -> SpawnedH3RealtimeSessionServer {
        let observed_resume = Arc::new(Mutex::new(Vec::new()));
        let service = FailThenAckRealtimeSessionService {
            heartbeat_seq,
            ack_seq,
            ack_client_mutation_id: client_mutation_id.to_owned(),
            observed_resume: observed_resume.clone(),
            session_count: Arc::new(Mutex::new(0)),
        };
        spawn_h3_realtime_session_server_with_service(service).await
    }

    async fn spawn_h3_realtime_session_server_with_service<S>(
        service: S,
    ) -> SpawnedH3RealtimeSessionServer
    where
        S: proto::looper_realtime_server::LooperRealtime + Clone + Send + Sync + 'static,
        S::SessionStream: Send + 'static,
    {
        let certificate = generate_simple_self_signed(vec!["localhost".to_owned()])
            .expect("generate H3 test certificate");
        let certificate_der = certificate.cert.der().as_ref().to_vec();
        let certificate_sha256 = format!("sha256:{}", sha256_hex(&certificate_der));
        let mut tls_config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .expect("H3 TLS versions")
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(certificate_der)],
            PrivateKeyDer::Pkcs8(certificate.key_pair.serialize_der().into()),
        )
        .expect("H3 test cert");
        tls_config.alpn_protocols = vec![b"h3".to_vec()];
        let quic_config =
            QuicServerConfig::try_from(Arc::new(tls_config)).expect("H3 QUIC server config");
        let endpoint = H3Endpoint::server(
            ServerConfig::with_crypto(Arc::new(quic_config)),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        )
        .expect("H3 server endpoint");
        let address = endpoint.local_addr().expect("H3 local addr");
        let acceptor = H3QuinnAcceptor::new(endpoint.clone());
        let routes = tonic::service::Routes::new(
            proto::looper_realtime_server::LooperRealtimeServer::new(service),
        );
        let server_task = tokio::spawn(async move {
            tonic_h3::server::H3Router::new(routes)
                .serve(acceptor)
                .await
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        SpawnedH3RealtimeSessionServer {
            url: format!("https://{}:{}", address.ip(), address.port()),
            certificate_sha256,
            endpoint,
            server_task,
        }
    }

    #[derive(Clone)]
    struct FailThenAckRealtimeSessionService {
        heartbeat_seq: i64,
        ack_seq: i64,
        ack_client_mutation_id: String,
        observed_resume: Arc<Mutex<Vec<i64>>>,
        session_count: Arc<Mutex<usize>>,
    }

    #[tonic::async_trait]
    impl proto::looper_realtime_server::LooperRealtime for FailThenAckRealtimeSessionService {
        type SessionStream = ReceiverStream<Result<proto::ServerFrame, tonic::Status>>;

        async fn health(
            &self,
            _request: tonic::Request<proto::HealthRequest>,
        ) -> Result<tonic::Response<proto::HealthResponse>, tonic::Status> {
            Ok(tonic::Response::new(proto::HealthResponse {
                ok: true,
                service: "test".to_owned(),
                server_time: String::new(),
            }))
        }

        async fn session(
            &self,
            request: tonic::Request<tonic::Streaming<proto::ClientFrame>>,
        ) -> Result<tonic::Response<Self::SessionStream>, tonic::Status> {
            let heartbeat_seq = self.heartbeat_seq;
            let ack_seq = self.ack_seq;
            let ack_client_mutation_id = self.ack_client_mutation_id.clone();
            let observed_resume = self.observed_resume.clone();
            let session_index = {
                let mut session_count = self.session_count.lock().expect("session count lock");
                *session_count += 1;
                *session_count
            };
            let mut stream = request.into_inner();
            let (sender, receiver) = mpsc::channel(4);
            tokio::spawn(async move {
                if session_index == 1 {
                    let _ = sender
                        .send(Ok(proto::ServerFrame {
                            frame: Some(proto::server_frame::Frame::Heartbeat(proto::Heartbeat {
                                latest_seq: heartbeat_seq,
                                server_time: "2026-07-01T00:00:00Z".to_owned(),
                            })),
                        }))
                        .await;
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if let Ok(Ok(Some(frame))) =
                        tokio::time::timeout(Duration::from_millis(50), stream.message()).await
                    {
                        if let Some(proto::client_frame::Frame::Resume(resume)) = frame.frame {
                            observed_resume
                                .lock()
                                .expect("resume lock")
                                .push(resume.after_seq);
                        }
                    }
                    let _ = sender
                        .send(Err(tonic::Status::unavailable(
                            "established h3 stream failed",
                        )))
                        .await;
                    return;
                }

                if let Ok(Ok(Some(frame))) =
                    tokio::time::timeout(Duration::from_millis(500), stream.message()).await
                {
                    if let Some(proto::client_frame::Frame::Resume(resume)) = frame.frame {
                        observed_resume
                            .lock()
                            .expect("resume lock")
                            .push(resume.after_seq);
                    }
                }
                let _ = sender
                    .send(Ok(proto::ServerFrame {
                        frame: Some(proto::server_frame::Frame::Ack(proto::CommandAck {
                            accepted: true,
                            account_id: "test".to_owned(),
                            node_id: "default".to_owned(),
                            client_mutation_id: ack_client_mutation_id.clone(),
                            ack_seq,
                            entity_id: "thread-fallback".to_owned(),
                            revision: format!("rev-{ack_seq}"),
                            server_time: "2026-07-01T00:00:00Z".to_owned(),
                            idempotent_replay: true,
                            error_code: String::new(),
                            reject_reason: String::new(),
                            current_state: String::new(),
                        })),
                    }))
                    .await;
                tokio::time::sleep(Duration::from_millis(100)).await;
            });
            Ok(tonic::Response::new(ReceiverStream::new(receiver)))
        }
    }

    async fn reserve_dead_udp_url() -> String {
        let socket =
            tokio::net::UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("reserve dead UDP socket");
        let address = socket.local_addr().expect("dead UDP addr");
        drop(socket);
        format!("https://{}:{}", address.ip(), address.port())
    }

    #[derive(Clone)]
    struct TestRealtimeSessionService {
        heartbeat_seq: Option<i64>,
        ack_seq: Option<i64>,
        ack_client_mutation_id: String,
        observed_resume: Arc<Mutex<Vec<i64>>>,
    }

    impl Default for TestRealtimeSessionService {
        fn default() -> Self {
            Self {
                heartbeat_seq: None,
                ack_seq: None,
                ack_client_mutation_id: String::new(),
                observed_resume: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[tonic::async_trait]
    impl proto::looper_realtime_server::LooperRealtime for TestRealtimeSessionService {
        type SessionStream = ReceiverStream<Result<proto::ServerFrame, tonic::Status>>;

        async fn health(
            &self,
            _request: tonic::Request<proto::HealthRequest>,
        ) -> Result<tonic::Response<proto::HealthResponse>, tonic::Status> {
            Ok(tonic::Response::new(proto::HealthResponse {
                ok: true,
                service: "test".to_owned(),
                server_time: String::new(),
            }))
        }

        async fn session(
            &self,
            request: tonic::Request<tonic::Streaming<proto::ClientFrame>>,
        ) -> Result<tonic::Response<Self::SessionStream>, tonic::Status> {
            let heartbeat_seq = self.heartbeat_seq;
            let ack_seq = self.ack_seq;
            let ack_client_mutation_id = self.ack_client_mutation_id.clone();
            let observed_resume = self.observed_resume.clone();
            let mut stream = request.into_inner();
            let (sender, receiver) = mpsc::channel(4);
            tokio::spawn(async move {
                if let Some(heartbeat_seq) = heartbeat_seq {
                    let _ = sender
                        .send(Ok(proto::ServerFrame {
                            frame: Some(proto::server_frame::Frame::Heartbeat(proto::Heartbeat {
                                latest_seq: heartbeat_seq,
                                server_time: "2026-07-01T00:00:00Z".to_owned(),
                            })),
                        }))
                        .await;
                    return;
                }
                while let Ok(Some(frame)) = stream.message().await {
                    match frame.frame {
                        Some(proto::client_frame::Frame::Resume(resume)) => {
                            observed_resume
                                .lock()
                                .expect("resume lock")
                                .push(resume.after_seq);
                        }
                        Some(proto::client_frame::Frame::Command(_)) => {
                            if let Some(ack_seq) = ack_seq {
                                let _ = sender
                                    .send(Ok(proto::ServerFrame {
                                        frame: Some(proto::server_frame::Frame::Ack(
                                            proto::CommandAck {
                                                accepted: true,
                                                account_id: "test".to_owned(),
                                                node_id: "default".to_owned(),
                                                client_mutation_id: ack_client_mutation_id.clone(),
                                                ack_seq,
                                                entity_id: "thread-fallback".to_owned(),
                                                revision: format!("rev-{ack_seq}"),
                                                server_time: "2026-07-01T00:00:00Z".to_owned(),
                                                idempotent_replay: false,
                                                error_code: String::new(),
                                                reject_reason: String::new(),
                                                current_state: String::new(),
                                            },
                                        )),
                                    }))
                                    .await;
                            }
                            return;
                        }
                        _ => {}
                    }
                }
            });
            Ok(tonic::Response::new(ReceiverStream::new(receiver)))
        }
    }

    fn test_outbound_command(client_mutation_id: &str, after_seq: i64) -> OutboundSessionFrame {
        OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SetSessionMode,
            thread_id: "thread-fallback".to_owned(),
            preset: "auto".to_owned(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            notification_target_ids: Vec::new(),
            archived: false,
            client_mutation_id: client_mutation_id.to_owned(),
            after_seq,
        }
    }

    async fn recv_stream_event(
        events: &mut mpsc::Receiver<StateMiniStreamEvent>,
        label: &str,
    ) -> StateMiniStreamEvent {
        tokio::time::timeout(Duration::from_secs(3), events.recv())
            .await
            .unwrap_or_else(|_| panic!("{label} timed out"))
            .unwrap_or_else(|| panic!("{label} channel closed"))
    }

    async fn wait_for_resume_count(
        observed_resume: Arc<Mutex<Vec<i64>>>,
        expected_count: usize,
    ) -> Vec<i64> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let resumes = observed_resume.lock().expect("resume lock").clone();
            if resumes.len() >= expected_count || tokio::time::Instant::now() >= deadline {
                return resumes;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
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
        assert_eq!(delta.kind, STATE_MINI_REPLACEMENT_KIND);
        assert_eq!(delta.latest_seq, 16);
        assert_eq!(delta.sessions.len(), 2);
        assert_eq!(delta.sessions[0].session_id, "thread-1");
        assert_eq!(delta.sessions[1].session_id, "thread-2");
    }

    #[test]
    fn state_mini_delta_accepts_non_replacing_session_batch_payload() {
        let delta = client_state_mini_delta(proto::StateMiniDelta {
            seq: 16,
            entity_id: "mobile".to_owned(),
            kind: "session_mini_batch".to_owned(),
            revision: "rev-16".to_owned(),
            server_time: "2026-06-27T00:00:16Z".to_owned(),
            payload_json: json!({
                "latestSeq": 16,
                "replace": false,
                "sessions": [
                    {
                        "sessionId": "thread-2",
                        "assistantSurface": "zed",
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
        assert_eq!(delta.kind, "session_mini_batch");
        assert_eq!(delta.latest_seq, 16);
        assert_eq!(delta.sessions.len(), 1);
        assert_eq!(delta.sessions[0].session_id, "thread-2");
        assert_eq!(delta.sessions[0].assistant_surface, "zed");
    }

    #[test]
    fn state_mini_delta_accepts_empty_replace_sessions_payload() {
        let delta = client_state_mini_delta(proto::StateMiniDelta {
            seq: 17,
            entity_id: "mobile".to_owned(),
            kind: "session_changed".to_owned(),
            revision: "rev-17".to_owned(),
            server_time: "2026-06-27T00:00:17Z".to_owned(),
            payload_json: json!({
                "latestSeq": 17,
                "replace": true,
                "sessions": []
            })
            .to_string(),
        })
        .expect("delta");

        assert!(!delta.has_session);
        assert_eq!(delta.kind, STATE_MINI_REPLACEMENT_KIND);
        assert_eq!(delta.latest_seq, 17);
        assert!(delta.sessions.is_empty());
        assert_eq!(delta.entity_id, "mobile");
    }

    #[test]
    fn state_mini_replay_cursor_advances_on_server_finality_frames() {
        let current_seq = 10;
        let single_delta = ClientStateMiniDelta {
            seq: 11,
            latest_seq: 11,
            entity_id: "thread-1".to_owned(),
            kind: "session_changed".to_owned(),
            revision: "rev-11".to_owned(),
            server_time: "2026-06-27T00:00:11Z".to_owned(),
            has_session: true,
            session: ClientStateMini {
                session_id: "thread-1".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 11,
                revision: "rev-11".to_owned(),
                payload_json: r#"{"title":"One"}"#.to_owned(),
            },
            sessions: Vec::new(),
        };

        assert_eq!(state_mini_data_cursor_after_ack(current_seq, 20), 20);
        assert_eq!(state_mini_data_cursor_after_heartbeat(current_seq, 30), 30);
        assert_eq!(
            state_mini_data_cursor_after_delta(current_seq, &single_delta),
            11
        );
    }

    #[test]
    fn partial_replacement_chunk_reconnect_waits_for_completion_marker() {
        let current_seq = 10;
        let replacement = client_state_mini_delta(proto::StateMiniDelta {
            seq: 11,
            entity_id: "mobile".to_owned(),
            kind: "session_changed".to_owned(),
            revision: "rev-11".to_owned(),
            server_time: "2026-06-27T00:00:11Z".to_owned(),
            payload_json: json!({
                "latestSeq": 11,
                "replace": true,
                "replacementComplete": false,
                "sessions": [
                    {
                        "sessionId": "thread-1",
                        "assistantSurface": "codex",
                        "seq": 11,
                        "revision": "rev-11",
                        "title": "One"
                    }
                ]
            })
            .to_string(),
        })
        .expect("replacement chunk");
        let continuation = client_state_mini_delta(proto::StateMiniDelta {
            seq: 11,
            entity_id: "mobile".to_owned(),
            kind: "session_mini_batch".to_owned(),
            revision: "rev-11".to_owned(),
            server_time: "2026-06-27T00:00:11Z".to_owned(),
            payload_json: json!({
                "latestSeq": 11,
                "replace": false,
                "replacementComplete": true,
                "sessions": [
                    {
                        "sessionId": "thread-zed",
                        "assistantSurface": "zed",
                        "seq": 11,
                        "revision": "rev-11",
                        "title": "Zed"
                    }
                ]
            })
            .to_string(),
        })
        .expect("continuation chunk");

        assert_eq!(
            state_mini_data_cursor_after_delta(current_seq, &replacement),
            current_seq
        );
        assert_eq!(
            state_mini_data_cursor_after_delta(current_seq, &continuation),
            11
        );
    }

    #[test]
    fn clean_state_mini_stream_end_is_reconnectable() {
        let event = state_mini_stream_ended_event(42);

        match event {
            StateMiniStreamEvent::Reconnecting {
                latest_seq,
                error_description,
                ..
            } => {
                assert_eq!(latest_seq, 42);
                assert_eq!(error_description, STATE_MINI_STREAM_ENDED);
            }
            other => panic!("expected reconnecting event, got {other:?}"),
        }
    }
}

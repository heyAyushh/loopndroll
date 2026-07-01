use anyhow::{Context, Result, bail};

use crate::grpc::proto;

mod transport;

#[cfg(test)]
mod tests;

use transport::{
    local_h3_certificate_sha256, local_session_transport_endpoints,
    open_local_session_command_stream,
};

const COMMAND_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) async fn submit_local_session_command(
    http_base_url: &str,
    command: proto::Command,
    client_mutation_id: &str,
) -> Result<proto::CommandAck> {
    let LocalSessionCommandResult {
        ack,
        transport: _transport,
        endpoint_url: _endpoint_url,
        fallback_reason: _fallback_reason,
    } = submit_local_session_command_observed(http_base_url, command, client_mutation_id).await?;
    Ok(ack)
}

async fn submit_local_session_command_observed(
    http_base_url: &str,
    command: proto::Command,
    client_mutation_id: &str,
) -> Result<LocalSessionCommandResult> {
    submit_local_session_command_with_h3_certificate_sha256(
        http_base_url,
        command,
        client_mutation_id,
        local_h3_certificate_sha256(),
    )
    .await
}

async fn submit_local_session_command_with_h3_certificate_sha256(
    http_base_url: &str,
    command: proto::Command,
    client_mutation_id: &str,
    h3_certificate_sha256: Option<String>,
) -> Result<LocalSessionCommandResult> {
    let frame = proto::ClientFrame {
        frame: Some(proto::client_frame::Frame::Command(command)),
    };
    let opened = open_local_session_command_stream(
        local_session_transport_endpoints(http_base_url, h3_certificate_sha256)?,
        frame,
    )
    .await?;

    let ack = wait_for_local_command_ack(opened.stream, client_mutation_id).await?;

    if !ack.accepted {
        let reason = if ack.reject_reason.is_empty() {
            "command rejected"
        } else {
            ack.reject_reason.as_str()
        };
        bail!("{reason}");
    }
    Ok(LocalSessionCommandResult {
        ack,
        transport: opened.transport,
        endpoint_url: opened.endpoint_url,
        fallback_reason: opened.fallback_reason,
    })
}

async fn wait_for_local_command_ack(
    mut stream: tonic::Streaming<proto::ServerFrame>,
    client_mutation_id: &str,
) -> Result<proto::CommandAck> {
    tokio::time::timeout(COMMAND_ACK_TIMEOUT, async {
        while let Some(frame) = stream.message().await? {
            let Some(proto::server_frame::Frame::Ack(ack)) = frame.frame else {
                continue;
            };
            if ack.client_mutation_id == client_mutation_id {
                return Ok::<proto::CommandAck, tonic::Status>(ack);
            }
        }
        Err(tonic::Status::not_found(
            "matching command ack not received",
        ))
    })
    .await
    .context("local Session command ACK timed out")?
    .context("local Session command ACK failed")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalSessionTransport {
    H3,
    H2,
}

#[derive(Clone, Debug)]
pub(super) struct LocalSessionEndpoint {
    pub(super) transport: LocalSessionTransport,
    pub(super) url: String,
    pub(super) h3_certificate_sha256: Option<String>,
}

pub(super) struct OpenLocalSessionCommandStream {
    pub(super) stream: tonic::Streaming<proto::ServerFrame>,
    pub(super) transport: LocalSessionTransport,
    pub(super) endpoint_url: String,
    pub(super) fallback_reason: String,
}

#[derive(Debug)]
struct LocalSessionCommandResult {
    ack: proto::CommandAck,
    transport: LocalSessionTransport,
    endpoint_url: String,
    fallback_reason: String,
}

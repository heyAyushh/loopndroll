use anyhow::{Context, Result, anyhow, bail};
use tokio_stream::iter;
use tonic::transport::Endpoint;

use crate::grpc::proto;
use crate::mobile::network::DEFAULT_GRPC_PORT_OFFSET;

const COMMAND_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) async fn submit_local_session_command(
    http_base_url: &str,
    command: proto::Command,
    client_mutation_id: &str,
) -> Result<proto::CommandAck> {
    let endpoint = local_grpc_endpoint(http_base_url)?;
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(endpoint)
        .await
        .context("connect local Session stream")?;
    let frame = proto::ClientFrame {
        frame: Some(proto::client_frame::Frame::Command(command)),
    };
    let request = tonic::Request::new(iter([frame]));
    let response = client
        .session(request)
        .await
        .context("submit local Session command")?;
    let mut stream = response.into_inner();

    let ack = tokio::time::timeout(COMMAND_ACK_TIMEOUT, async {
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
    .context("local Session command ACK timed out")??;

    if !ack.accepted {
        let reason = if ack.reject_reason.is_empty() {
            "command rejected"
        } else {
            ack.reject_reason.as_str()
        };
        bail!("{reason}");
    }
    Ok(ack)
}

fn local_grpc_endpoint(http_base_url: &str) -> Result<Endpoint> {
    let mut url = reqwest::Url::parse(http_base_url).context("parse control-plane base URL")?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow!("control-plane base URL has no port"))?;
    let grpc_port = port
        .checked_add(DEFAULT_GRPC_PORT_OFFSET)
        .ok_or_else(|| anyhow!("control-plane port is too high to derive gRPC port"))?;
    url.set_port(Some(grpc_port))
        .map_err(|_| anyhow!("invalid derived gRPC port"))?;
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Endpoint::from_shared(url.to_string()).context("build local gRPC endpoint")
}

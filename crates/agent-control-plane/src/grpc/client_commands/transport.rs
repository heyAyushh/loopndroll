use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::codegen::http::Uri;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;
use tonic_h3::quinn::H3QuinnConnector;

use crate::grpc::proto;
use crate::mobile::network::DEFAULT_GRPC_PORT_OFFSET;

use super::{
    LocalSessionEndpoint, LocalSessionTransport, OpenLocalSessionClient,
    OpenLocalSessionCommandStream,
};

const LOCAL_H3_SESSION_OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);
const AUTHORIZATION_METADATA: &str = "authorization";
const BEARER_PREFIX: &str = "Bearer ";
const LOCAL_CONNECTION_CODE_PATH: &str = "api/mobile/connection-code";

#[derive(Deserialize)]
struct LocalConnectionCodeResponse {
    #[serde(rename = "pairingTokenId")]
    pairing_token_id: String,
    #[serde(rename = "pairingToken")]
    pairing_token: String,
}

pub(super) async fn open_local_session_command_stream(
    endpoints: Vec<LocalSessionEndpoint>,
    frame: proto::ClientFrame,
) -> Result<OpenLocalSessionCommandStream> {
    let mut last_error: Option<anyhow::Error> = None;
    let mut h3_fallback_reason = String::new();

    for endpoint in endpoints {
        match endpoint.transport {
            LocalSessionTransport::H3 => {
                match open_h3_local_session_command_stream(endpoint.clone(), frame.clone()).await {
                    Ok(opened) => return Ok(opened),
                    Err(error) => {
                        h3_fallback_reason = format!("h3 pre-stream failure: {error:#}");
                        last_error = Some(error);
                    }
                }
            }
            LocalSessionTransport::H2 => {
                match open_h2_local_session_command_stream(
                    endpoint.clone(),
                    frame.clone(),
                    h3_fallback_reason.clone(),
                )
                .await
                {
                    Ok(opened) => return Ok(opened),
                    Err(error) => last_error = Some(error),
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow!("no local Session transport endpoints available")))
}

async fn open_h2_local_session_command_stream(
    endpoint: LocalSessionEndpoint,
    frame: proto::ClientFrame,
    fallback_reason: String,
) -> Result<OpenLocalSessionCommandStream> {
    let channel_endpoint =
        Endpoint::from_shared(endpoint.url.clone()).context("build local H2 gRPC endpoint")?;
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::connect(channel_endpoint)
        .await
        .context("connect local H2 Session stream")?;
    let (request, request_sender) = session_request(frame).await?;
    let response = client
        .session(request)
        .await
        .context("submit local H2 Session command")?;
    Ok(OpenLocalSessionCommandStream {
        stream: response.into_inner(),
        request_sender,
        client: OpenLocalSessionClient::H2(client),
        transport: LocalSessionTransport::H2,
        endpoint_url: endpoint.url,
        fallback_reason,
    })
}

async fn open_h3_local_session_command_stream(
    endpoint: LocalSessionEndpoint,
    frame: proto::ClientFrame,
) -> Result<OpenLocalSessionCommandStream> {
    let uri = endpoint
        .url
        .parse::<Uri>()
        .context("parse local H3 gRPC endpoint")?;
    let certificate_sha256 = endpoint
        .h3_certificate_sha256
        .as_deref()
        .ok_or_else(|| anyhow!("local H3 certificate pin missing"))?;
    let client_endpoint = crate::grpc::pinned_h3_client_endpoint(certificate_sha256)
        .context("build pinned local H3 client endpoint")?;
    let connector = H3QuinnConnector::new(uri.clone(), "localhost".to_owned(), client_endpoint);
    let channel = tonic_h3::H3Channel::new(connector, uri);
    let mut client = proto::looper_realtime_client::LooperRealtimeClient::new(channel);
    let (mut request, request_sender) = session_request(frame).await?;
    let authorization = local_h3_authorization_header(&endpoint.http_base_url).await?;
    request.metadata_mut().insert(
        AUTHORIZATION_METADATA,
        MetadataValue::try_from(authorization).context("build local H3 authorization metadata")?,
    );
    let response = tokio::time::timeout(LOCAL_H3_SESSION_OPEN_TIMEOUT, client.session(request))
        .await
        .context("local H3 Session open timed out")?
        .context("submit local H3 Session command")?;
    Ok(OpenLocalSessionCommandStream {
        stream: response.into_inner(),
        request_sender,
        client: OpenLocalSessionClient::H3(client),
        transport: LocalSessionTransport::H3,
        endpoint_url: endpoint.url,
        fallback_reason: String::new(),
    })
}

async fn session_request(
    frame: proto::ClientFrame,
) -> Result<(
    tonic::Request<ReceiverStream<proto::ClientFrame>>,
    mpsc::Sender<proto::ClientFrame>,
)> {
    let (request_sender, request_receiver) = mpsc::channel(1);
    request_sender
        .send(frame)
        .await
        .context("queue local Session command frame")?;
    Ok((
        tonic::Request::new(ReceiverStream::new(request_receiver)),
        request_sender,
    ))
}

pub(super) fn local_session_transport_endpoints(
    http_base_url: &str,
    h3_certificate_sha256: Option<String>,
) -> Result<Vec<LocalSessionEndpoint>> {
    let mut endpoints = Vec::new();
    if let Some(certificate_sha256) = h3_certificate_sha256.filter(|value| !value.trim().is_empty())
    {
        endpoints.push(LocalSessionEndpoint {
            transport: LocalSessionTransport::H3,
            url: local_grpc_url(http_base_url, LocalSessionTransport::H3)?,
            h3_certificate_sha256: Some(certificate_sha256),
            http_base_url: http_base_url.to_owned(),
        });
    }
    endpoints.push(LocalSessionEndpoint {
        transport: LocalSessionTransport::H2,
        url: local_grpc_url(http_base_url, LocalSessionTransport::H2)?,
        h3_certificate_sha256: None,
        http_base_url: http_base_url.to_owned(),
    });
    Ok(endpoints)
}

pub(super) fn local_h3_certificate_sha256() -> Option<String> {
    crate::grpc::load_persisted_h3_certificate_sha256(&crate::runtime::default_store_path())
        .ok()
        .flatten()
}

fn local_grpc_url(http_base_url: &str, transport: LocalSessionTransport) -> Result<String> {
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
    match transport {
        LocalSessionTransport::H2 => {
            url.set_scheme("http")
                .map_err(|_| anyhow!("invalid derived H2 gRPC scheme"))?;
        }
        LocalSessionTransport::H3 => {
            url.set_scheme("https")
                .map_err(|_| anyhow!("invalid derived H3 gRPC scheme"))?;
        }
    }
    Ok(url.to_string())
}

async fn local_h3_authorization_header(http_base_url: &str) -> Result<String> {
    let mut url = reqwest::Url::parse(http_base_url).context("parse local HTTP base URL")?;
    url.set_path(LOCAL_CONNECTION_CODE_PATH);
    url.set_query(None);
    url.set_fragment(None);
    let connection_code = reqwest::get(url)
        .await
        .context("request local H3 pairing token")?
        .error_for_status()
        .context("local H3 pairing token request failed")?
        .json::<LocalConnectionCodeResponse>()
        .await
        .context("decode local H3 pairing token")?;
    Ok(format!(
        "{BEARER_PREFIX}{}.{}",
        connection_code.pairing_token_id, connection_code.pairing_token
    ))
}

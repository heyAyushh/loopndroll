use anyhow::{Context, Result, anyhow, bail};
use tokio_stream::iter;
use tonic::codegen::http::Uri;
use tonic::transport::Endpoint;
use tonic_h3::quinn::H3QuinnConnector;

use crate::grpc::proto;
use crate::mobile::network::DEFAULT_GRPC_PORT_OFFSET;

const COMMAND_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const LOCAL_H3_SESSION_OPEN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

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

async fn open_local_session_command_stream(
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
    let request = tonic::Request::new(iter([frame]));
    let response = client
        .session(request)
        .await
        .context("submit local H2 Session command")?;
    Ok(OpenLocalSessionCommandStream {
        stream: response.into_inner(),
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
    let request = tonic::Request::new(iter([frame]));
    let response = tokio::time::timeout(LOCAL_H3_SESSION_OPEN_TIMEOUT, client.session(request))
        .await
        .context("local H3 Session open timed out")?
        .context("submit local H3 Session command")?;
    Ok(OpenLocalSessionCommandStream {
        stream: response.into_inner(),
        transport: LocalSessionTransport::H3,
        endpoint_url: endpoint.url,
        fallback_reason: String::new(),
    })
}

fn local_session_transport_endpoints(
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
        });
    }
    endpoints.push(LocalSessionEndpoint {
        transport: LocalSessionTransport::H2,
        url: local_grpc_url(http_base_url, LocalSessionTransport::H2)?,
        h3_certificate_sha256: None,
    });
    Ok(endpoints)
}

fn local_h3_certificate_sha256() -> Option<String> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalSessionTransport {
    H3,
    H2,
}

#[derive(Clone, Debug)]
struct LocalSessionEndpoint {
    transport: LocalSessionTransport,
    url: String,
    h3_certificate_sha256: Option<String>,
}

struct OpenLocalSessionCommandStream {
    stream: tonic::Streaming<proto::ServerFrame>,
    transport: LocalSessionTransport,
    endpoint_url: String,
    fallback_reason: String,
}

#[derive(Debug)]
struct LocalSessionCommandResult {
    ack: proto::CommandAck,
    transport: LocalSessionTransport,
    endpoint_url: String,
    fallback_reason: String,
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::pin::Pin;
    use std::time::Duration;

    use futures_core::Stream;
    use rusqlite::Connection;
    use tempfile::TempDir;
    use tokio::sync::oneshot;
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status};

    use super::*;
    use crate::control_plane::{ControlPlane, ControlPlaneConfig};
    use crate::grpc::proto::looper_realtime_server::{LooperRealtime, LooperRealtimeServer};
    use crate::grpc::proto::{HealthRequest, HealthResponse, SetSessionModeRequest, command};

    #[tokio::test]
    async fn local_session_command_h3() {
        let fixture = TestControlPlaneFixture::new();
        fixture.write_state_db();
        let control_plane = fixture.control_plane();
        prime_state_mini_cache(&control_plane);
        let grpc_address = reserve_local_tcp_address().await;
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let h3 = crate::grpc::spawn_h3_server(control_plane.clone(), grpc_address, async {
            let _ = shutdown_receiver.await;
        })
        .await
        .expect("spawn H3 server");

        let client_mutation_id = "local-h3-mode";
        let command = set_session_mode_command("thread-main", client_mutation_id);
        let result = submit_local_session_command_with_h3_certificate_sha256(
            &http_base_url_for_grpc_address(grpc_address),
            command,
            client_mutation_id,
            Some(h3.certificate_sha256.clone()),
        )
        .await
        .expect("local Session command should use H3 when H2 is absent");

        let ack = result.ack;
        assert!(ack.accepted, "H3 local command ACK should be accepted");
        assert_eq!(result.transport, LocalSessionTransport::H3);
        assert!(result.fallback_reason.is_empty());
        assert_eq!(ack.client_mutation_id, client_mutation_id);
        assert!(
            control_plane
                .store()
                .mobile_command_ack("SetSessionMode", client_mutation_id)
                .expect("stored command ACK lookup")
                .is_some(),
            "local helper must use the Session command ACK path"
        );
        println!(
            "local_session_command_h3 transport=h3 endpoint={} ack_mutation_id={} ack_seq={} udp_addr={}",
            result.endpoint_url, ack.client_mutation_id, ack.ack_seq, h3.listen_address
        );

        let _ = shutdown_sender.send(());
        h3.server_task
            .await
            .expect("H3 server task should not panic")
            .expect("H3 server should shut down");
    }

    #[tokio::test]
    async fn local_session_command_h3_falls_back_to_h2() {
        let fixture = TestControlPlaneFixture::new();
        fixture.write_state_db();
        let control_plane = fixture.control_plane();
        prime_state_mini_cache(&control_plane);
        let certificate = crate::grpc::load_or_create_h3_certificate(&control_plane)
            .expect("local H3 certificate");
        let h2 = spawn_h2(control_plane.clone()).await;

        let client_mutation_id = "local-h2-fallback-mode";
        let command = set_session_mode_command("thread-main", client_mutation_id);
        let result = submit_local_session_command_with_h3_certificate_sha256(
            &http_base_url_for_grpc_address(h2.address),
            command,
            client_mutation_id,
            Some(certificate.certificate_sha256),
        )
        .await
        .expect("local Session command should fall back to H2 when H3 is absent");

        assert!(result.ack.accepted, "fallback H2 ACK should be accepted");
        assert_eq!(result.transport, LocalSessionTransport::H2);
        assert!(
            result.fallback_reason.contains("h3 pre-stream failure"),
            "fallback should record H3 failure reason, got {:?}",
            result.fallback_reason
        );
        assert!(
            control_plane
                .store()
                .mobile_command_ack("SetSessionMode", client_mutation_id)
                .expect("stored command ACK lookup")
                .is_some(),
            "fallback must still use the Session command ACK path"
        );
        println!(
            "local_session_command_h3_falls_back_to_h2 transport=h2 endpoint={} fallback_reason={} ack_mutation_id={} ack_seq={}",
            result.endpoint_url,
            result.fallback_reason,
            result.ack.client_mutation_id,
            result.ack.ack_seq
        );

        h2.shutdown().await;
    }

    #[tokio::test]
    async fn local_session_command_ack_timeout_remains_two_seconds() {
        let h2 = spawn_no_ack_h2().await;
        let started = tokio::time::Instant::now();
        let error = submit_local_session_command_with_h3_certificate_sha256(
            &http_base_url_for_grpc_address(h2.address),
            set_session_mode_command("thread-main", "timeout-command"),
            "timeout-command",
            None,
        )
        .await
        .expect_err("local Session command should time out waiting for ACK");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= COMMAND_ACK_TIMEOUT
                && elapsed < COMMAND_ACK_TIMEOUT + Duration::from_secs(2),
            "ACK timeout should stay near 2s, elapsed={elapsed:?}"
        );
        assert!(
            error
                .to_string()
                .contains("local Session command ACK timed out"),
            "unexpected timeout error: {error:#}"
        );
        println!(
            "local_session_command_ack_timeout timeout_ms={} error={}",
            elapsed.as_millis(),
            error
        );

        h2.shutdown().await;
    }

    #[test]
    fn local_session_transport_candidates_require_pin_for_h3() {
        let endpoints = local_session_transport_endpoints("http://127.0.0.1:8765", None)
            .expect("local transport endpoints");
        assert_eq!(endpoints.len(), 1);
        assert_eq!(endpoints[0].transport, LocalSessionTransport::H2);
        assert_eq!(endpoints[0].url, "http://127.0.0.1:8766/");
    }

    fn set_session_mode_command(thread_id: &str, client_mutation_id: &str) -> proto::Command {
        proto::Command {
            command: Some(command::Command::SetSessionMode(SetSessionModeRequest {
                thread_id: thread_id.to_owned(),
                preset: "await-reply".to_owned(),
                client_mutation_id: client_mutation_id.to_owned(),
            })),
        }
    }

    async fn reserve_local_tcp_address() -> SocketAddr {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("reserve local TCP address");
        let address = listener.local_addr().expect("reserved TCP address");
        drop(listener);
        assert!(address.port() > 0, "reserved gRPC port must be nonzero");
        address
    }

    async fn spawn_h2(control_plane: ControlPlane) -> SpawnedH2 {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("bind H2 listener");
        let address = listener.local_addr().expect("H2 listener address");
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_task = tokio::spawn(async move {
            crate::grpc::serve_with_listener(control_plane, listener, async {
                let _ = shutdown_receiver.await;
            })
            .await
            .expect("H2 server");
        });
        SpawnedH2 {
            address,
            shutdown_sender: Some(shutdown_sender),
            server_task,
        }
    }

    async fn spawn_no_ack_h2() -> SpawnedH2 {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("bind no-ACK H2 listener");
        let address = listener.local_addr().expect("no-ACK H2 listener address");
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_task = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(LooperRealtimeServer::new(NoAckRealtimeService))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_receiver.await;
                })
                .await
                .expect("no-ACK H2 server");
        });
        SpawnedH2 {
            address,
            shutdown_sender: Some(shutdown_sender),
            server_task,
        }
    }

    struct SpawnedH2 {
        address: SocketAddr,
        shutdown_sender: Option<oneshot::Sender<()>>,
        server_task: tokio::task::JoinHandle<()>,
    }

    impl SpawnedH2 {
        async fn shutdown(mut self) {
            if let Some(sender) = self.shutdown_sender.take() {
                let _ = sender.send(());
            }
            self.server_task.await.expect("H2 server task should join");
        }
    }

    struct NoAckRealtimeService;

    #[tonic::async_trait]
    impl LooperRealtime for NoAckRealtimeService {
        type SessionStream =
            Pin<Box<dyn Stream<Item = Result<proto::ServerFrame, Status>> + Send + 'static>>;

        async fn health(
            &self,
            _request: Request<HealthRequest>,
        ) -> Result<Response<HealthResponse>, Status> {
            Ok(Response::new(HealthResponse {
                ok: true,
                service: "looper-realtime".to_owned(),
                server_time: String::new(),
            }))
        }

        async fn session(
            &self,
            _request: Request<tonic::Streaming<proto::ClientFrame>>,
        ) -> Result<Response<Self::SessionStream>, Status> {
            Ok(Response::new(Box::pin(futures_util::stream::pending())))
        }
    }

    fn http_base_url_for_grpc_address(grpc_address: SocketAddr) -> String {
        let http_port = grpc_address
            .port()
            .checked_sub(DEFAULT_GRPC_PORT_OFFSET)
            .expect("reserved gRPC port must allow HTTP base derivation");
        format!("http://{}:{http_port}", grpc_address.ip())
    }

    fn prime_state_mini_cache(control_plane: &ControlPlane) {
        control_plane
            .reconcile_mobile_session_mini_projection()
            .expect("reconcile state minis");
    }

    struct TestControlPlaneFixture {
        temp_dir: TempDir,
        codex_home: std::path::PathBuf,
    }

    impl TestControlPlaneFixture {
        fn new() -> Self {
            let temp_dir = TempDir::new().expect("temp dir");
            let codex_home = temp_dir.path().join(".codex");
            fs::create_dir_all(codex_home.join("sessions")).expect("codex dirs");
            fs::create_dir_all(temp_dir.path().join(".grok/sessions")).expect("grok dirs");
            Self {
                temp_dir,
                codex_home,
            }
        }

        fn write_state_db(&self) {
            let connection =
                Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
            connection
                .execute_batch(
                    r#"
create table threads (
  thread_id text primary key,
  title text,
  cwd text,
  source text,
  model text,
  reasoning_effort text,
  created_at_ms integer,
  updated_at_ms integer,
  archived integer
);
insert into threads values
  ('thread-main', 'Main task', '/tmp/project', 'desktop', 'gpt-5.5', 'high', 1000, 2000, 0);
"#,
                )
                .expect("seed state");
            Connection::open(self.codex_home.join("logs_1.sqlite")).expect("logs");
        }

        fn control_plane(&self) -> ControlPlane {
            ControlPlane::new(ControlPlaneConfig {
                codex_home: self.codex_home.clone(),
                codex_executable: Some("/usr/bin/false".to_owned()),
                grok_home: self.temp_dir.path().join(".grok"),
                store_path: self.temp_dir.path().join("control-plane.sqlite"),
                hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
                home_path: self.temp_dir.path().to_path_buf(),
                zed_process_commands: Some(Vec::new()),
            })
        }
    }
}

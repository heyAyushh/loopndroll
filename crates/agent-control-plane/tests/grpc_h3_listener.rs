use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig};
use agent_control_plane::grpc::proto::{
    ClientFrame, HealthRequest, looper_realtime_client::LooperRealtimeClient, server_frame,
};
use agent_control_plane::http::build_router;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use rusqlite::Connection;
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tonic::codegen::http::Uri;
use tonic::metadata::MetadataValue;
use tonic_h3::quinn::H3QuinnConnector;
use tonic_h3::quinn::h3_quinn::Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{
    ClientConfig, crypto::rustls::QuicClientConfig, rustls as quinn_rustls,
};
use tower::ServiceExt;

const H3_LISTENER_TIMEOUT: Duration = Duration::from_secs(5);

type H3LooperClient = LooperRealtimeClient<tonic_h3::H3Channel<H3QuinnConnector>>;

#[tokio::test]
async fn grpc_h3_listener_binds_shutdowns_and_coexists_with_h2() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let authorization =
        issue_mobile_authorization_header(&build_router(control_plane.clone())).await;
    let h2 = spawn_h2_client(control_plane.clone()).await;

    let listen_address = agent_control_plane::grpc::default_h3_listen_address(h2.address)
        .expect("H3 listen address");
    let h3 = spawn_h3(control_plane.clone(), listen_address).await;
    assert!(
        h3.certificate_sha256.starts_with("sha256:"),
        "H3 cert pin should be exposed as sha256-prefixed hex"
    );
    let reloaded = agent_control_plane::grpc::load_or_create_h3_certificate(&control_plane)
        .expect("reload persisted H3 certificate");
    assert_eq!(
        h3.certificate_sha256, reloaded.certificate_sha256,
        "H3 certificate should persist in control-plane-owned state"
    );

    let mut h3_client = h3.client();
    let health = tokio::time::timeout(H3_LISTENER_TIMEOUT, h3_client.health(HealthRequest {}))
        .await
        .expect("H3 health timed out")
        .expect("H3 health response")
        .into_inner();
    assert!(health.ok, "H3 health should report ok");

    let mut h3_request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    h3_request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(authorization.as_str()).expect("authorization metadata"),
    );
    let mut h3_stream = tokio::time::timeout(H3_LISTENER_TIMEOUT, h3_client.session(h3_request))
        .await
        .expect("H3 session open timed out")
        .expect("H3 session should open")
        .into_inner();
    let h3_frame = tokio::time::timeout(H3_LISTENER_TIMEOUT, h3_stream.message())
        .await
        .expect("H3 frame timed out")
        .expect("H3 frame result")
        .expect("H3 should produce a Session frame");
    let h3_ack = match h3_frame.frame {
        Some(server_frame::Frame::Ack(ack)) => ack,
        other => panic!("expected H3 Session ack, got {other:?}"),
    };
    assert_eq!(h3_ack.error_code, "empty_client_frame");
    println!(
        "h3_listener_session_ok udp_addr={} pin={}",
        h3.address, h3.certificate_sha256
    );

    let mut h2_client = h2.client.clone();
    let h2_health = tokio::time::timeout(H3_LISTENER_TIMEOUT, h2_client.health(HealthRequest {}))
        .await
        .expect("H2 health timed out")
        .expect("H2 health response")
        .into_inner();
    assert!(h2_health.ok, "H2 listener should remain live");
    println!("h2_coexistence_ok tcp_addr={}", h2.address);

    drop(h3_stream);
    drop(h3_client);
    let h3_address = h3.address;
    h3.shutdown().await;
    tokio::net::UdpSocket::bind(h3_address)
        .await
        .expect("H3 UDP port should be reusable after shutdown");
    println!("h3_shutdown_released_udp udp_addr={h3_address}");
    h2.shutdown().await;
}

#[tokio::test]
async fn grpc_h3_listener_allows_loopback_without_mobile_auth() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let h3 = spawn_h3(
        control_plane,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
    )
    .await;
    let mut client = h3.client();
    let request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    let mut stream = tokio::time::timeout(H3_LISTENER_TIMEOUT, client.session(request))
        .await
        .expect("loopback H3 session timed out")
        .expect("loopback H3 should bypass auth")
        .into_inner();
    let frame = tokio::time::timeout(H3_LISTENER_TIMEOUT, stream.message())
        .await
        .expect("loopback H3 frame timed out")
        .expect("loopback H3 frame result")
        .expect("loopback H3 should produce a frame");
    assert!(matches!(frame.frame, Some(server_frame::Frame::Ack(_))));
    println!("h3_loopback_bypass_ok udp_addr={}", h3.address);
    h3.shutdown().await;
}

#[tokio::test]
async fn grpc_h3_auth_rejects_missing_pairing_token() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let h3 = spawn_h3(
        control_plane,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
    )
    .await;
    let mut client = h3.client_for_host(Ipv4Addr::LOCALHOST.into());
    let request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    let status = tokio::time::timeout(H3_LISTENER_TIMEOUT, client.session(request))
        .await
        .expect("non-loopback H3 auth rejection timed out")
        .expect_err("non-loopback H3 should reject missing pairing token");
    assert_eq!(status.code(), tonic::Code::Unauthenticated);
    assert!(
        status.message().contains("pairing token required"),
        "unexpected auth error: {status}"
    );
    println!(
        "h3_missing_token_rejected code={:?} message={}",
        status.code(),
        status.message()
    );
    h3.shutdown().await;
}

struct SpawnedH2 {
    address: SocketAddr,
    client: LooperRealtimeClient<tonic::transport::Channel>,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<()>,
}

impl SpawnedH2 {
    async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        let _ = tokio::time::timeout(H3_LISTENER_TIMEOUT, self.server_task).await;
    }
}

async fn spawn_h2_client(control_plane: ControlPlane) -> SpawnedH2 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind H2 listener");
    let address = listener.local_addr().expect("H2 listener address");
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        agent_control_plane::grpc::serve_with_listener(control_plane, listener, async {
            let _ = shutdown_receiver.await;
        })
        .await
        .expect("H2 server");
    });
    let client = LooperRealtimeClient::connect(format!("http://{address}"))
        .await
        .expect("connect H2 client");
    SpawnedH2 {
        address,
        client,
        shutdown_sender: Some(shutdown_sender),
        server_task,
    }
}

struct SpawnedH3 {
    address: SocketAddr,
    certificate_sha256: String,
    client_endpoint: Endpoint,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<Result<(), tonic_h3::Error>>,
}

impl SpawnedH3 {
    fn client(&self) -> H3LooperClient {
        self.client_for_host(self.address.ip())
    }

    fn client_for_host(&self, host: IpAddr) -> H3LooperClient {
        LooperRealtimeClient::new(quinn_h3_channel(
            h3_uri(SocketAddr::new(host, self.address.port())),
            self.client_endpoint.clone(),
        ))
    }

    async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        let _ = tokio::time::timeout(H3_LISTENER_TIMEOUT, self.server_task).await;
    }
}

async fn spawn_h3(control_plane: ControlPlane, listen_address: SocketAddr) -> SpawnedH3 {
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let server = agent_control_plane::grpc::spawn_h3_server(control_plane, listen_address, async {
        let _ = shutdown_receiver.await;
    })
    .await
    .expect("spawn H3 server");
    let client_endpoint = configured_client_endpoint(&server.certificate_der).await;
    SpawnedH3 {
        address: server.listen_address,
        certificate_sha256: server.certificate_sha256,
        client_endpoint,
        shutdown_sender: Some(shutdown_sender),
        server_task: server.server_task,
    }
}

fn quinn_h3_channel(uri: Uri, endpoint: Endpoint) -> tonic_h3::H3Channel<H3QuinnConnector> {
    let connector = H3QuinnConnector::new(uri.clone(), "localhost".to_owned(), endpoint);
    tonic_h3::H3Channel::new(connector, uri)
}

fn h3_uri(address: SocketAddr) -> Uri {
    format!("https://{}:{}", address.ip(), address.port())
        .parse()
        .expect("H3 URI")
}

async fn configured_client_endpoint(certificate_der: &[u8]) -> Endpoint {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(certificate_der.to_vec()))
        .expect("trust H3 test cert");
    let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .expect("H3 client endpoint");
    let mut tls_config = quinn_rustls::ClientConfig::builder_with_provider(Arc::new(
        quinn_rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&quinn_rustls::version::TLS13])
    .expect("H3 client TLS versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    tls_config.enable_early_data = true;
    let quic_config = QuicClientConfig::try_from(tls_config).expect("H3 QUIC client config");
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_config)));
    endpoint
}

async fn issue_mobile_authorization_header(router: &Router) -> String {
    let response = request_json(
        router,
        Method::GET,
        "/api/mobile/connection-code",
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    let token_id = response["pairingTokenId"]
        .as_str()
        .expect("pairing token id");
    let token = response["pairingToken"].as_str().expect("pairing token");
    format!("Bearer {token_id}.{token}")
}

async fn request_json(
    router: &Router,
    method: Method,
    path: &str,
    remote_address: Option<SocketAddr>,
) -> serde_json::Value {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("request");
    if let Some(remote_address) = remote_address {
        request.extensions_mut().insert(ConnectInfo(remote_address));
    }
    let response = router.clone().oneshot(request).await.expect("response");
    assert!(
        response.status().is_success(),
        "response status: {}",
        response.status()
    );
    let body = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json response")
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
        let connection = Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
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

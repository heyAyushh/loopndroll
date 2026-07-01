use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig};
use agent_control_plane::grpc::LooperRealtimeService;
use agent_control_plane::grpc::proto::{
    ClientFrame, HealthRequest, HealthResponse, looper_realtime_client::LooperRealtimeClient,
    looper_realtime_server::LooperRealtimeServer, server_frame,
};
use agent_control_plane::http::build_router;
use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rusqlite::Connection;
use rustls::RootCertStore;
use rustls::pki_types::PrivateKeyDer;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tonic::codegen::http::Uri;
use tonic::metadata::MetadataValue;
use tonic_h3::quinn::H3QuinnConnector;
use tonic_h3::quinn::h3_quinn::Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{ClientConfig, ServerConfig, VarInt};
use tower::ServiceExt;

const H3_SMOKE_TIMEOUT: Duration = Duration::from_secs(5);

type H3LooperClient = LooperRealtimeClient<tonic_h3::H3Channel<H3QuinnConnector>>;

#[tokio::test]
async fn grpc_h3_quinn_smoke_health_and_session() {
    let fixture = TestControlPlaneFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let authorization =
        issue_mobile_authorization_header(&build_router(control_plane.clone())).await;
    let h3 = spawn_h3_realtime(control_plane).await;
    let (mut client, health) = h3.ready_client().await;

    assert!(health.ok, "H3 health should report ok");
    assert_eq!(health.service, "looper-realtime");
    println!(
        "h3_health_ok service={} udp_addr={}",
        health.service, h3.address
    );

    let mut request = tonic::Request::new(tokio_stream::iter(vec![ClientFrame { frame: None }]));
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(authorization.as_str()).expect("authorization metadata"),
    );
    let mut stream = tokio::time::timeout(H3_SMOKE_TIMEOUT, client.session(request))
        .await
        .expect("H3 Session open timed out")
        .expect("H3 Session should open")
        .into_inner();
    let frame = tokio::time::timeout(H3_SMOKE_TIMEOUT, stream.message())
        .await
        .expect("H3 Session frame timed out")
        .expect("H3 Session frame result")
        .expect("H3 Session should yield an observable frame");
    let ack = match frame.frame {
        Some(server_frame::Frame::Ack(ack)) => ack,
        other => panic!("expected Session ack over H3, got {other:?}"),
    };
    assert_eq!(ack.error_code, "empty_client_frame");
    assert!(
        !ack.accepted,
        "empty client frame should be rejected but prove Session round trip"
    );
    println!(
        "h3_session_open ack_error={} udp_addr={}",
        ack.error_code, h3.address
    );

    h3.shutdown().await;
}

#[tokio::test]
async fn grpc_h3_quinn_smoke_dead_udp_returns_error() {
    let client_endpoint = configured_client_endpoint(&h3_certificate().cert).await;
    let dead_address = reserve_dead_udp_address().await;
    let uri = h3_uri(dead_address);
    let channel = quinn_h3_channel(uri, client_endpoint);
    let mut client = LooperRealtimeClient::new(channel);

    let result = tokio::time::timeout(H3_SMOKE_TIMEOUT, client.health(HealthRequest {})).await;
    match result {
        Ok(Err(error)) => {
            println!("dead_udp_handled_error addr={dead_address} error={error}");
        }
        Err(_) => {
            println!("dead_udp_handled_error addr={dead_address} error=connect timed out");
        }
        Ok(Ok(response)) => panic!(
            "dead UDP endpoint unexpectedly returned health over H3: {:?}",
            response.into_inner()
        ),
    }
}

struct SpawnedH3Realtime {
    address: SocketAddr,
    client_endpoint: Endpoint,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<Result<(), tonic_h3::Error>>,
}

impl SpawnedH3Realtime {
    fn client(&self) -> H3LooperClient {
        LooperRealtimeClient::new(quinn_h3_channel(
            h3_uri(self.address),
            self.client_endpoint.clone(),
        ))
    }

    async fn ready_client(&self) -> (H3LooperClient, HealthResponse) {
        let deadline = tokio::time::Instant::now() + H3_SMOKE_TIMEOUT;
        let mut attempts = 0;

        loop {
            attempts += 1;
            let mut client = self.client();
            let attempt_error = match tokio::time::timeout(
                Duration::from_millis(750),
                client.health(HealthRequest {}),
            )
            .await
            {
                Ok(Ok(response)) => return (client, response.into_inner()),
                Ok(Err(error)) => error.to_string(),
                Err(_) => "health attempt timed out".to_owned(),
            };

            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "H3 health did not become ready after {attempts} attempts at {}: {attempt_error}",
                    self.address
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn shutdown(self) {
        let SpawnedH3Realtime {
            address,
            client_endpoint,
            shutdown_sender,
            server_task,
        } = self;
        if let Some(sender) = shutdown_sender {
            let _ = sender.send(());
        }
        client_endpoint.close(VarInt::from_u32(0), b"test shutdown");
        client_endpoint.wait_idle().await;
        let result = tokio::time::timeout(H3_SMOKE_TIMEOUT, server_task)
            .await
            .unwrap_or_else(|_| panic!("H3 smoke server task did not shut down for {address}"));
        result
            .unwrap_or_else(|error| panic!("H3 smoke server task panicked for {address}: {error}"))
            .unwrap_or_else(|error| {
                panic!("H3 smoke server returned error for {address}: {error}")
            });
    }
}

async fn spawn_h3_realtime(control_plane: ControlPlane) -> SpawnedH3Realtime {
    let CertifiedKey { cert, key_pair } = h3_certificate();
    let key = PrivateKeyDer::Pkcs8(key_pair.serialize_der().into());
    let server_config =
        ServerConfig::with_single_cert(vec![cert.der().clone()], key).expect("H3 server config");
    let server_endpoint = Endpoint::server(
        server_config,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
    )
    .expect("H3 server endpoint");
    let address = server_endpoint.local_addr().expect("H3 server local addr");
    let endpoint_for_shutdown_signal = server_endpoint.clone();
    let endpoint_for_idle = server_endpoint.clone();
    let client_endpoint = configured_client_endpoint(&cert).await;
    let acceptor = tonic_h3::quinn::H3QuinnAcceptor::new(server_endpoint);
    let routes = tonic::service::Routes::new(LooperRealtimeServer::new(
        LooperRealtimeService::new(control_plane),
    ));
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        let shutdown = async move {
            let _ = shutdown_receiver.await;
            endpoint_for_shutdown_signal.close(VarInt::from_u32(0), b"test shutdown");
        };
        tonic_h3::server::H3Router::new(routes)
            .serve_with_shutdown(acceptor, shutdown)
            .await?;
        endpoint_for_idle.close(VarInt::from_u32(0), b"test shutdown");
        endpoint_for_idle.wait_idle().await;
        Ok(())
    });
    SpawnedH3Realtime {
        address,
        client_endpoint,
        shutdown_sender: Some(shutdown_sender),
        server_task,
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

fn h3_certificate() -> CertifiedKey {
    generate_simple_self_signed(vec!["localhost".to_owned()]).expect("self-signed localhost cert")
}

async fn configured_client_endpoint(cert: &rcgen::Certificate) -> Endpoint {
    let mut roots = RootCertStore::empty();
    roots.add(cert.der().clone()).expect("trust H3 test cert");
    let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .expect("H3 client endpoint");
    endpoint.set_default_client_config(
        ClientConfig::with_root_certificates(Arc::new(roots)).expect("H3 client config"),
    );
    endpoint
}

async fn reserve_dead_udp_address() -> SocketAddr {
    let socket = tokio::net::UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .expect("reserve dead UDP socket");
    let address = socket.local_addr().expect("dead UDP local addr");
    drop(socket);
    address
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

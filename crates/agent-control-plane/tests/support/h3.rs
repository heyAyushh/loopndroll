use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use agent_control_plane::control_plane::ControlPlane;
use agent_control_plane::grpc::proto::{
    HealthRequest, HealthResponse, looper_realtime_client::LooperRealtimeClient,
};
use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use tokio::sync::oneshot;
use tonic::codegen::http::Uri;
use tonic_h3::quinn::H3QuinnConnector;
use tonic_h3::quinn::h3_quinn::Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{
    ClientConfig, VarInt, crypto::rustls::QuicClientConfig, rustls as quinn_rustls,
};

pub const H3_TEST_TIMEOUT: Duration = Duration::from_secs(5);

pub type H3LooperClient = LooperRealtimeClient<tonic_h3::H3Channel<H3QuinnConnector>>;

#[allow(dead_code)]
pub struct SpawnedH2 {
    pub address: SocketAddr,
    pub client: LooperRealtimeClient<tonic::transport::Channel>,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<()>,
}

#[allow(dead_code)]
impl SpawnedH2 {
    pub async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        tokio::time::timeout(H3_TEST_TIMEOUT, self.server_task)
            .await
            .expect("H2 server task should shut down")
            .expect("H2 server task should not panic");
    }
}

#[allow(dead_code)]
pub async fn spawn_h2_client(control_plane: ControlPlane) -> SpawnedH2 {
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

pub struct SpawnedH3 {
    pub address: SocketAddr,
    #[allow(dead_code)]
    pub certificate_sha256: String,
    client_endpoint: Endpoint,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<Result<(), tonic_h3::Error>>,
}

impl SpawnedH3 {
    pub fn client_for_host(&self, host: IpAddr) -> H3LooperClient {
        LooperRealtimeClient::new(quinn_h3_channel(
            h3_uri(SocketAddr::new(host, self.address.port())),
            self.client_endpoint.clone(),
        ))
    }

    pub fn client(&self) -> H3LooperClient {
        self.client_for_host(self.readiness_host())
    }

    pub async fn ready_client(&self) -> (H3LooperClient, HealthResponse) {
        let deadline = tokio::time::Instant::now() + H3_TEST_TIMEOUT;
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

    #[allow(dead_code)]
    pub async fn wait_until_ready(&self) {
        let (_, health) = self.ready_client().await;
        assert!(health.ok, "H3 health should report ok before auth probe");
    }

    pub async fn shutdown(self) -> SocketAddr {
        let SpawnedH3 {
            address,
            client_endpoint,
            shutdown_sender,
            server_task,
            ..
        } = self;
        if let Some(sender) = shutdown_sender {
            let _ = sender.send(());
        }
        client_endpoint.close(VarInt::from_u32(0), b"test shutdown");
        client_endpoint.wait_idle().await;
        let result = tokio::time::timeout(H3_TEST_TIMEOUT, server_task)
            .await
            .unwrap_or_else(|_| panic!("H3 server task did not shut down for {address}"));
        result
            .unwrap_or_else(|error| panic!("H3 server task panicked for {address}: {error}"))
            .unwrap_or_else(|error| panic!("H3 server returned error for {address}: {error}"));
        assert_udp_port_rebinds(address).await;
        address
    }

    fn readiness_host(&self) -> IpAddr {
        match self.address.ip() {
            IpAddr::V4(address) if address.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(address) if address.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
            address => address,
        }
    }
}

pub async fn spawn_h3(control_plane: ControlPlane, listen_address: SocketAddr) -> SpawnedH3 {
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

pub fn quinn_h3_channel(uri: Uri, endpoint: Endpoint) -> tonic_h3::H3Channel<H3QuinnConnector> {
    let connector = H3QuinnConnector::new(uri.clone(), "localhost".to_owned(), endpoint);
    tonic_h3::H3Channel::new(connector, uri)
}

pub fn h3_uri(address: SocketAddr) -> Uri {
    format!("https://{}:{}", address.ip(), address.port())
        .parse()
        .expect("H3 URI")
}

pub async fn configured_client_endpoint(certificate_der: &[u8]) -> Endpoint {
    let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .expect("H3 client endpoint");
    let tls_config = h3_client_tls_config(certificate_der);
    let quic_config = QuicClientConfig::try_from(tls_config).expect("H3 QUIC client config");
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_config)));
    endpoint
}

pub fn h3_client_tls_config(certificate_der: &[u8]) -> quinn_rustls::ClientConfig {
    let mut roots = RootCertStore::empty();
    roots
        .add(CertificateDer::from(certificate_der.to_vec()))
        .expect("trust H3 test cert");
    let mut tls_config = quinn_rustls::ClientConfig::builder_with_provider(Arc::new(
        quinn_rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&quinn_rustls::version::TLS13])
    .expect("H3 client TLS versions")
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls_config.alpn_protocols = vec![b"h3".to_vec()];
    tls_config
}

#[allow(dead_code)]
pub async fn reserve_dead_udp_address() -> SocketAddr {
    let socket = tokio::net::UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .await
        .expect("reserve dead UDP socket");
    let address = socket.local_addr().expect("dead UDP local addr");
    drop(socket);
    address
}

async fn assert_udp_port_rebinds(address: SocketAddr) {
    let rebound = tokio::net::UdpSocket::bind(address)
        .await
        .unwrap_or_else(|error| {
            panic!("H3 UDP port did not rebind after shutdown at {address}: {error}")
        });
    let rebound_address = rebound.local_addr().expect("rebound UDP local addr");
    assert_eq!(rebound_address, address);
    println!("h3_udp_rebind_ok udp_addr={address}");
}

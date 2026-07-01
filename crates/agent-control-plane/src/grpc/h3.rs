use std::fs;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::task::JoinHandle;
use tonic_h3::quinn::H3QuinnAcceptor;
use tonic_h3::quinn::h3_quinn::Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{
    ServerConfig, VarInt, crypto::rustls::QuicServerConfig, rustls as quinn_rustls,
};

use crate::control_plane::ControlPlane;
use crate::grpc::LooperRealtimeService;
use crate::grpc::proto::looper_realtime_server::LooperRealtimeServer;

pub const GRPC_H3_LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_GRPC_H3_LISTEN";
const H3_ALPN: &[u8] = b"h3";
const H3_CERT_STATE_EXTENSION: &str = "h3-local-cert.json";
const H3_CERT_SERVER_NAME: &str = "localhost";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcH3Certificate {
    pub certificate_der: Vec<u8>,
    private_key_der: Vec<u8>,
    pub certificate_sha256: String,
}

#[derive(Debug)]
pub struct SpawnedGrpcH3Server {
    pub listen_address: SocketAddr,
    pub certificate_der: Vec<u8>,
    pub certificate_sha256: String,
    pub server_task: JoinHandle<Result<(), tonic_h3::Error>>,
}

#[derive(Serialize, Deserialize)]
struct PersistedGrpcH3Certificate {
    certificate_der_base64: String,
    private_key_der_base64: String,
}

pub fn default_h3_listen_address(h2_listen_address: SocketAddr) -> Result<SocketAddr> {
    h3_listen_address_from_env_value(
        h2_listen_address,
        std::env::var(GRPC_H3_LISTEN_ENV).ok().as_deref(),
    )
}

pub fn load_or_create_h3_certificate(control_plane: &ControlPlane) -> Result<GrpcH3Certificate> {
    let path = control_plane
        .store_path()
        .with_extension(H3_CERT_STATE_EXTENSION);
    if path.exists() {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("read H3 certificate state {}", path.display()))?;
        let persisted: PersistedGrpcH3Certificate = serde_json::from_str(&content)
            .with_context(|| format!("parse H3 certificate state {}", path.display()))?;
        let certificate_der = BASE64
            .decode(persisted.certificate_der_base64)
            .context("decode H3 certificate DER")?;
        let private_key_der = BASE64
            .decode(persisted.private_key_der_base64)
            .context("decode H3 private key DER")?;
        return Ok(GrpcH3Certificate::new(certificate_der, private_key_der));
    }

    let certificate = generate_h3_certificate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create H3 certificate state dir {}", parent.display()))?;
    }
    let persisted = PersistedGrpcH3Certificate {
        certificate_der_base64: BASE64.encode(&certificate.certificate_der),
        private_key_der_base64: BASE64.encode(&certificate.private_key_der),
    };
    fs::write(
        &path,
        serde_json::to_vec_pretty(&persisted).context("serialize H3 certificate state")?,
    )
    .with_context(|| format!("write H3 certificate state {}", path.display()))?;
    Ok(certificate)
}

pub async fn spawn_h3_server(
    control_plane: ControlPlane,
    listen_address: SocketAddr,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<SpawnedGrpcH3Server> {
    let certificate = load_or_create_h3_certificate(&control_plane)?;
    let server_config = h3_server_config(&certificate)?;
    let endpoint = Endpoint::server(server_config, listen_address).context("bind H3 endpoint")?;
    let local_address = endpoint.local_addr().context("H3 endpoint local address")?;
    let endpoint_for_shutdown = endpoint.clone();
    let acceptor = H3QuinnAcceptor::new(endpoint);
    let service_peer_addr = SocketAddr::new(local_address.ip(), local_address.port());
    let routes = tonic::service::Routes::new(LooperRealtimeServer::new(
        LooperRealtimeService::with_peer_addr_override(control_plane, service_peer_addr),
    ));
    let server_task = tokio::spawn(async move {
        let result = tonic_h3::server::H3Router::new(routes)
            .serve_with_shutdown(acceptor, shutdown)
            .await;
        endpoint_for_shutdown.close(VarInt::from_u32(0), b"shutdown");
        endpoint_for_shutdown.wait_idle().await;
        result
    });

    Ok(SpawnedGrpcH3Server {
        listen_address: local_address,
        certificate_der: certificate.certificate_der,
        certificate_sha256: certificate.certificate_sha256,
        server_task,
    })
}

impl GrpcH3Certificate {
    fn new(certificate_der: Vec<u8>, private_key_der: Vec<u8>) -> Self {
        let certificate_sha256 = format!("sha256:{}", sha256_hex(&certificate_der));
        Self {
            certificate_der,
            private_key_der,
            certificate_sha256,
        }
    }
}

fn generate_h3_certificate() -> Result<GrpcH3Certificate> {
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(vec![H3_CERT_SERVER_NAME.to_owned()])
            .context("generate H3 local certificate")?;
    Ok(GrpcH3Certificate::new(
        cert.der().as_ref().to_vec(),
        key_pair.serialize_der(),
    ))
}

fn h3_server_config(certificate: &GrpcH3Certificate) -> Result<ServerConfig> {
    let mut tls_config = quinn_rustls::ServerConfig::builder_with_provider(Arc::new(
        quinn_rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&quinn_rustls::version::TLS13])
    .map_err(|error| anyhow!("configure H3 TLS protocol versions: {error:?}"))?
    .with_no_client_auth()
    .with_single_cert(
        vec![CertificateDer::from(certificate.certificate_der.clone())],
        PrivateKeyDer::Pkcs8(certificate.private_key_der.clone().into()),
    )
    .context("configure H3 TLS certificate")?;
    tls_config.alpn_protocols = vec![H3_ALPN.to_vec()];
    tls_config.max_early_data_size = u32::MAX;
    let quic_crypto = QuicServerConfig::try_from(Arc::new(tls_config))
        .map_err(|error| anyhow!("configure H3 QUIC TLS: {error:?}"))?;
    Ok(ServerConfig::with_crypto(Arc::new(quic_crypto)))
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

fn h3_listen_address_from_env_value(
    h2_listen_address: SocketAddr,
    listen_address: Option<&str>,
) -> Result<SocketAddr> {
    if let Some(listen_address) = listen_address {
        return listen_address
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid {GRPC_H3_LISTEN_ENV} value"));
    }
    Ok(h2_listen_address)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::h3_listen_address_from_env_value;

    #[test]
    fn grpc_h3_invalid_listen_address_is_rejected() {
        let h2_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8766);
        let error = h3_listen_address_from_env_value(h2_address, Some("not-a-socket"))
            .expect_err("invalid H3 listen override should be rejected");
        assert!(
            error
                .to_string()
                .contains("invalid AGENT_CONTROL_PLANE_GRPC_H3_LISTEN value"),
            "unexpected parse error: {error:#}"
        );
    }
}

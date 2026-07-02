use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tonic_h3::quinn::H3QuinnAcceptor;
use tonic_h3::quinn::h3_quinn::Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{
    IdleTimeout, ServerConfig, TransportConfig, VarInt, crypto::rustls::QuicServerConfig,
    rustls as quinn_rustls,
};

use crate::control_plane::ControlPlane;
use crate::grpc::LooperRealtimeService;
use crate::grpc::proto::looper_realtime_server::LooperRealtimeServer;

use super::certificate::load_or_create_h3_certificate;
use super::{GRPC_H3_LISTEN_ENV, GrpcH3Certificate, H3_ALPN, SpawnedGrpcH3Server};

const H3_QUIC_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);
const H3_QUIC_MAX_IDLE_TIMEOUT: Duration = Duration::from_secs(40);

pub fn default_h3_listen_address(h2_listen_address: SocketAddr) -> Result<SocketAddr> {
    h3_listen_address_from_env_value(
        h2_listen_address,
        std::env::var(GRPC_H3_LISTEN_ENV).ok().as_deref(),
    )
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
    let endpoint_for_shutdown_signal = endpoint.clone();
    let endpoint_for_idle = endpoint.clone();
    let acceptor = H3QuinnAcceptor::new(endpoint);
    // tonic-h3 0.0.5 does not surface the QUIC remote peer through
    // `tonic::Request::remote_addr()`. Do not substitute the local listen socket:
    // that turns `0.0.0.0` into a false non-loopback peer and `127.0.0.1` into a
    // false loopback peer. Without a real peer address h3 requests must prove
    // mobile auth with metadata.
    let routes = tonic::service::Routes::new(LooperRealtimeServer::new(
        LooperRealtimeService::new(control_plane),
    ));
    let server_task = tokio::spawn(async move {
        let shutdown = async move {
            shutdown.await;
            endpoint_for_shutdown_signal.close(VarInt::from_u32(0), b"shutdown");
        };
        let result = tonic_h3::server::H3Router::new(routes)
            .serve_with_shutdown(acceptor, shutdown)
            .await;
        endpoint_for_idle.close(VarInt::from_u32(0), b"shutdown");
        endpoint_for_idle.wait_idle().await;
        result
    });

    Ok(SpawnedGrpcH3Server {
        listen_address: local_address,
        certificate_der: certificate.certificate_der,
        certificate_sha256: certificate.certificate_sha256,
        server_task,
    })
}

fn h3_server_config(certificate: &GrpcH3Certificate) -> Result<ServerConfig> {
    let tls_config = h3_tls_server_config(certificate)?;
    let quic_crypto = QuicServerConfig::try_from(Arc::new(tls_config))
        .map_err(|error| anyhow!("configure H3 QUIC TLS: {error:?}"))?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));
    server_config.transport_config(Arc::new(h3_server_transport_config()?));
    Ok(server_config)
}

pub(super) fn h3_server_transport_config() -> Result<TransportConfig> {
    let mut transport_config = TransportConfig::default();
    // The Session service sends application heartbeats every 15s and the client
    // read deadline is 20s. A 10s QUIC ping keeps NAT state warm before the app
    // heartbeat is due, while a 40s QUIC idle timeout lets the app-level deadline
    // classify missing frames before Quinn tears down the connection.
    transport_config.keep_alive_interval(Some(H3_QUIC_KEEP_ALIVE_INTERVAL));
    transport_config.max_idle_timeout(Some(
        IdleTimeout::try_from(H3_QUIC_MAX_IDLE_TIMEOUT)
            .context("configure H3 QUIC idle timeout")?,
    ));
    Ok(transport_config)
}

pub(super) fn h3_tls_server_config(
    certificate: &GrpcH3Certificate,
) -> Result<quinn_rustls::ServerConfig> {
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
    Ok(tls_config)
}

pub(super) fn h3_listen_address_from_env_value(
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

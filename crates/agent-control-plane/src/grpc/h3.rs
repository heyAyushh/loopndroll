use std::net::SocketAddr;

use anyhow::Result;
use tokio::task::JoinHandle;

mod certificate;
mod client;
mod server;

#[cfg(test)]
mod tests;

pub const GRPC_H3_LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_GRPC_H3_LISTEN";
const H3_ALPN: &[u8] = b"h3";
const H3_CERT_SERVER_NAME: &str = "localhost";
const H3_CERT_STATE_EXTENSION: &str = "h3-local-cert.json";

pub use certificate::{load_or_create_h3_certificate, load_persisted_h3_certificate_sha256};
pub(crate) use client::pinned_h3_client_endpoint;
pub use server::{default_h3_listen_address, spawn_h3_server};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcH3Certificate {
    pub certificate_der: Vec<u8>,
    pub(super) private_key_der: Vec<u8>,
    pub certificate_sha256: String,
}

#[derive(Debug)]
pub struct SpawnedGrpcH3Server {
    pub listen_address: SocketAddr,
    pub certificate_der: Vec<u8>,
    pub certificate_sha256: String,
    pub server_task: JoinHandle<Result<(), tonic_h3::Error>>,
}

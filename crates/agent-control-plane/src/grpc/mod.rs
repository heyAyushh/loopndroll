use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use socket2::{SockRef, TcpKeepalive};
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::{Stream, StreamExt};
use tonic::transport::Server;

use crate::control_plane::ControlPlane;

mod auth;
mod events;
mod service;

pub mod proto {
    tonic::include_proto!("looper.v1");
}

pub use service::LooperRealtimeService;

const GRPC_HTTP2_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(20);
const GRPC_HTTP2_KEEPALIVE_TIMEOUT: Duration = Duration::from_secs(5);
const GRPC_TCP_KEEPALIVE_IDLE: Duration = Duration::from_secs(30);
const GRPC_TCP_KEEPALIVE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrpcServerConfig {
    pub listen_address: SocketAddr,
}

pub async fn serve_with_listener(
    control_plane: ControlPlane,
    listener: TcpListener,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let service = proto::looper_realtime_server::LooperRealtimeServer::new(
        LooperRealtimeService::new(control_plane),
    );
    Server::builder()
        .http2_keepalive_interval(Some(GRPC_HTTP2_KEEPALIVE_INTERVAL))
        .http2_keepalive_timeout(Some(GRPC_HTTP2_KEEPALIVE_TIMEOUT))
        .add_service(service)
        .serve_with_incoming_shutdown(realtime_incoming(listener), shutdown)
        .await?;
    Ok(())
}

fn realtime_incoming(
    listener: TcpListener,
) -> impl Stream<Item = io::Result<tokio::net::TcpStream>> {
    TcpListenerStream::new(listener).map(|accepted| accepted.and_then(configure_realtime_socket))
}

fn configure_realtime_socket(stream: tokio::net::TcpStream) -> io::Result<tokio::net::TcpStream> {
    stream.set_nodelay(true)?;

    let keepalive = TcpKeepalive::new()
        .with_time(GRPC_TCP_KEEPALIVE_IDLE)
        .with_interval(GRPC_TCP_KEEPALIVE_INTERVAL);
    SockRef::from(&stream).set_tcp_keepalive(&keepalive)?;

    Ok(stream)
}

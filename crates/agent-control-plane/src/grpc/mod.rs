use std::net::SocketAddr;

use anyhow::Result;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use crate::control_plane::ControlPlane;

mod auth;
mod events;
mod service;

pub mod proto {
    tonic::include_proto!("looper.v1");
}

pub use service::LooperRealtimeService;

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
        .add_service(service)
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
        .await?;
    Ok(())
}

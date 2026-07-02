use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::pin::Pin;

use futures_core::Stream;
use rusqlite::Connection;
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, Status};

use crate::control_plane::{ControlPlane, ControlPlaneConfig};
use crate::grpc::proto;
use crate::grpc::proto::looper_realtime_server::{LooperRealtime, LooperRealtimeServer};
use crate::grpc::proto::{HealthRequest, HealthResponse, SetSessionModeRequest, command};
use crate::mobile::network::DEFAULT_GRPC_PORT_OFFSET;

pub(super) fn set_session_mode_command(
    thread_id: &str,
    client_mutation_id: &str,
) -> proto::Command {
    proto::Command {
        command: Some(command::Command::SetSessionMode(SetSessionModeRequest {
            thread_id: thread_id.to_owned(),
            preset: "await-reply".to_owned(),
            client_mutation_id: client_mutation_id.to_owned(),
        })),
    }
}

pub(super) async fn reserve_local_tcp_address() -> SocketAddr {
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
            .await
            .expect("reserve local TCP address");
    let address = listener.local_addr().expect("reserved TCP address");
    drop(listener);
    assert!(address.port() > 0, "reserved gRPC port must be nonzero");
    address
}

pub(super) async fn spawn_h2(control_plane: ControlPlane) -> SpawnedH2 {
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

pub(super) async fn spawn_no_ack_h2() -> SpawnedH2 {
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

pub(super) struct SpawnedH2 {
    pub(super) address: SocketAddr,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<()>,
}

impl SpawnedH2 {
    pub(super) async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        self.server_task.await.expect("H2 server task should join");
    }
}

struct NoAckRealtimeService;

fn no_ack_unary_command_status() -> Status {
    Status::unimplemented("no-ACK fixture only implements Session")
}

#[tonic::async_trait]
impl LooperRealtime for NoAckRealtimeService {
    type SessionStream =
        Pin<Box<dyn Stream<Item = std::result::Result<proto::ServerFrame, Status>> + Send>>;

    async fn health(
        &self,
        _request: Request<HealthRequest>,
    ) -> std::result::Result<Response<HealthResponse>, Status> {
        Ok(Response::new(HealthResponse {
            ok: true,
            service: "looper-realtime".to_owned(),
            server_time: String::new(),
        }))
    }

    async fn session(
        &self,
        _request: Request<tonic::Streaming<proto::ClientFrame>>,
    ) -> std::result::Result<Response<Self::SessionStream>, Status> {
        Ok(Response::new(Box::pin(futures_util::stream::pending())))
    }

    async fn set_session_mode(
        &self,
        _request: Request<proto::SetSessionModeRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn send_session_prompt(
        &self,
        _request: Request<proto::SendSessionPromptRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn submit_notification_reply(
        &self,
        _request: Request<proto::SubmitNotificationReplyRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_siri_current_session(
        &self,
        _request: Request<proto::SetSiriCurrentSessionRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_siri_default_session(
        &self,
        _request: Request<proto::SetSiriDefaultSessionRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn save_default_prompt(
        &self,
        _request: Request<proto::SaveDefaultPromptRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_session_archived(
        &self,
        _request: Request<proto::SetSessionArchivedRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn delete_session(
        &self,
        _request: Request<proto::DeleteSessionRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn mute_session(
        &self,
        _request: Request<proto::MuteSessionRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_scope(
        &self,
        _request: Request<proto::SetScopeRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_global_preset(
        &self,
        _request: Request<proto::SetGlobalPresetRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_global_notification(
        &self,
        _request: Request<proto::SetGlobalNotificationRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_default_notification_targets(
        &self,
        _request: Request<proto::SetDefaultNotificationTargetsRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_global_completion_check(
        &self,
        _request: Request<proto::SetGlobalCompletionCheckRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn upsert_notification_route(
        &self,
        _request: Request<proto::UpsertNotificationRouteRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn delete_notification_route(
        &self,
        _request: Request<proto::DeleteNotificationRouteRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn upsert_completion_check(
        &self,
        _request: Request<proto::UpsertCompletionCheckRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn delete_completion_check(
        &self,
        _request: Request<proto::DeleteCompletionCheckRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_session_notifications(
        &self,
        _request: Request<proto::SetSessionNotificationsRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_session_completion_check(
        &self,
        _request: Request<proto::SetSessionCompletionCheckRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }

    async fn set_assistant_surface(
        &self,
        _request: Request<proto::SetAssistantSurfaceRequest>,
    ) -> std::result::Result<Response<proto::CommandAck>, Status> {
        Err(no_ack_unary_command_status())
    }
}

pub(super) fn http_base_url_for_grpc_address(grpc_address: SocketAddr) -> String {
    let http_port = grpc_address
        .port()
        .checked_sub(DEFAULT_GRPC_PORT_OFFSET)
        .expect("reserved gRPC port must allow HTTP base derivation");
    format!("http://{}:{http_port}", grpc_address.ip())
}

pub(super) fn prime_state_mini_cache(control_plane: &ControlPlane) {
    control_plane
        .reconcile_mobile_session_mini_projection()
        .expect("reconcile state minis");
}

pub(super) struct TestControlPlaneFixture {
    temp_dir: TempDir,
    codex_home: std::path::PathBuf,
}

impl TestControlPlaneFixture {
    pub(super) fn new() -> Self {
        let temp_dir = TempDir::new().expect("temp dir");
        let codex_home = temp_dir.path().join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).expect("codex dirs");
        fs::create_dir_all(temp_dir.path().join(".grok/sessions")).expect("grok dirs");
        Self {
            temp_dir,
            codex_home,
        }
    }

    pub(super) fn write_state_db(&self) {
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

    pub(super) fn control_plane(&self) -> ControlPlane {
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

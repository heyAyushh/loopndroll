mod support;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent_control_plane::control_plane::ControlPlane;
use agent_control_plane::events::MobileSessionMiniProjectionInput;
use agent_control_plane::http::build_router;
use agent_control_plane::mobile::events::{MobileEventInput, MobileEventKind};
use agent_control_plane::mobile::session::MobileHookPayload;
use looper_client_core::{
    ClientEndpoint, ClientEndpointTransport, ClientStateSnapshot, LooperClientCoreSessionRuntime,
};
use tempfile::TempDir;
use tokio::sync::oneshot;

use support::control_plane::TestControlPlaneFixture;
use support::h3::{SpawnedH2, SpawnedH3, reserve_dead_udp_address, spawn_h2_client, spawn_h3};
use support::mobile_auth::issue_mobile_authorization_header;

const THREAD_ID: &str = "thread-main";
const ASSISTANT_SURFACE: &str = "codex";
const PRESET_AWAIT_REPLY: &str = "await-reply";
const PRESET_INFINITE: &str = "infinite";
const PROMPT_TEXT: &str = "Continue from e2e harness.";
const FOLLOW_UP_PROMPT_TEXT: &str = "Follow up after failed delivery.";
const INITIAL_TITLE: &str = "E2E initial session";
const STREAM_DOWN_TITLE: &str = "E2E delta while h3 down";
const H2_STREAM_DOWN_TITLE: &str = "E2E delta while h2 down";
const DELIVERY_FAILED_TITLE: &str = "E2E prompt delivery failed";
const BEARER_PREFIX: &str = "Bearer ";
const PROMPT_DELIVERY_FAILED_DETAIL: &str = "prompt-delivery-failed";
const POLL_ATTEMPTS: usize = 160;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const HTTP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const QUEUED_PROMPT_COUNT: i64 = 1;

#[tokio::test]
async fn client_core_queue_prompt_drains_outbox_against_real_h2_server() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime("queue-prompt.json");
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let prompt = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            "queue".to_owned(),
        )
        .await
        .expect("send prompt intent should be accepted locally");
    assert!(prompt.accepted);

    wait_for_prompt_ack_and_empty_outbox(&runtime, &prompt.client_mutation_id).await;
    wait_for_queued_prompt_count(&harness.control_plane, QUEUED_PROMPT_COUNT).await;

    let local = runtime.local_snapshot().expect("local snapshot");
    assert!(
        local.pending_commands.is_empty(),
        "acked prompt command must be removed from durable pending commands: {:?}",
        local.pending_commands
    );

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn client_core_set_mode_round_trips_to_server_projection_and_stream_snapshot() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(None, INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime("set-mode.json");
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let mode = runtime
        .set_mode(THREAD_ID.to_owned(), PRESET_AWAIT_REPLY.to_owned())
        .await
        .expect("set mode intent should be accepted locally");
    assert!(mode.accepted);

    wait_for_mode_ack_and_projection(&harness.control_plane, &runtime, &mode.client_mutation_id)
        .await;
    wait_for_effective_mode(&runtime, PRESET_AWAIT_REPLY).await;

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn client_core_h2_stream_recovers_delta_after_listener_restart_same_port() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = RestartableH2::spawn(harness.control_plane.clone(), None).await;
    let runtime = harness.runtime("h2-restart.json");
    start_runtime(
        &runtime,
        vec![h2_endpoint(h2.address, &http.base_url(), false)],
        &harness.bearer_token,
    );
    wait_for_transport(&runtime, ClientEndpointTransport::H2).await;
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let h2_address = h2.abort().await;
    harness.emit_session_mini_event(Some(PRESET_AWAIT_REPLY), H2_STREAM_DOWN_TITLE);
    let target_seq = harness
        .control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest mobile state seq after h2 down delta");
    let restarted_h2 = RestartableH2::spawn(harness.control_plane.clone(), Some(h2_address)).await;

    let recovered = wait_for_session_title(&runtime, H2_STREAM_DOWN_TITLE).await;
    assert!(
        recovered.latest_seq >= target_seq,
        "client cursor must advance through replayed down-time delta: latest={} target={}",
        recovered.latest_seq,
        target_seq
    );

    stop_runtime(runtime).await;
    restarted_h2.abort().await;
    http.shutdown().await;
}

#[tokio::test]
#[ignore = "current behavior: established h3 stream closes as client after replay; production transport fix required"]
async fn client_core_h3_stream_recovers_delta_after_listener_restart_same_port() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let h3 = spawn_loopback_h3(harness.control_plane.clone()).await;
    let runtime = harness.runtime("h3-restart.json");
    start_runtime(&runtime, endpoints(&h3, &h2, &http), &harness.bearer_token);
    wait_for_h3_ready_session(&runtime, INITIAL_TITLE).await;

    let h3_address = h3.shutdown().await;
    harness.emit_session_mini_event(Some(PRESET_AWAIT_REPLY), STREAM_DOWN_TITLE);
    let target_seq = harness
        .control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest mobile state seq after h3 down delta");
    let restarted_h3 = spawn_h3(harness.control_plane.clone(), h3_address).await;
    restarted_h3.wait_until_ready().await;

    let recovered = wait_for_session_title(&runtime, STREAM_DOWN_TITLE).await;
    assert!(
        recovered.latest_seq >= target_seq,
        "client cursor must advance through replayed down-time delta: latest={} target={}",
        recovered.latest_seq,
        target_seq
    );

    stop_runtime(runtime).await;
    restarted_h3.shutdown().await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn client_core_falls_back_to_h2_when_h3_endpoint_is_dead() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let dead_h3_address = reserve_dead_udp_address().await;
    let h3_certificate =
        agent_control_plane::grpc::load_or_create_h3_certificate(&harness.control_plane)
            .expect("h3 certificate");
    let runtime = harness.runtime("h2-fallback.json");
    let endpoints = vec![
        h3_endpoint(
            dead_h3_address,
            &http.base_url(),
            &h3_certificate.certificate_sha256,
            false,
        ),
        h2_endpoint(h2.address, &http.base_url(), false),
    ];

    start_runtime(&runtime, endpoints, &harness.bearer_token);
    let snapshot = wait_for_transport(&runtime, ClientEndpointTransport::H2).await;
    assert_eq!(snapshot.endpoint_transport, ClientEndpointTransport::H2);

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn prompt_enqueued_while_stream_down_delivers_after_reconnect_without_user_action() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = RestartableH2::spawn(harness.control_plane.clone(), None).await;
    let runtime = harness.runtime("prompt-while-stream-down.json");
    start_runtime(
        &runtime,
        vec![h2_endpoint(h2.address, &http.base_url(), false)],
        &harness.bearer_token,
    );
    wait_for_transport(&runtime, ClientEndpointTransport::H2).await;
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let h2_address = h2.abort().await;
    let prompt = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            "queue".to_owned(),
        )
        .await
        .expect("prompt should enqueue locally while stream is down");
    assert!(prompt.accepted);
    let local = runtime.local_snapshot().expect("local snapshot");
    assert_eq!(
        local.pending_commands.len(),
        1,
        "prompt must remain durable until reconnect flushes it"
    );

    let restarted_h2 = RestartableH2::spawn(harness.control_plane.clone(), Some(h2_address)).await;

    wait_for_prompt_ack_and_empty_outbox(&runtime, &prompt.client_mutation_id).await;
    wait_for_queued_prompt_count(&harness.control_plane, QUEUED_PROMPT_COUNT).await;

    stop_runtime(runtime).await;
    restarted_h2.abort().await;
    http.shutdown().await;
}

#[tokio::test]
async fn prompt_delivery_failed_event_does_not_wedge_follow_up_prompt() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_INFINITE), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime("prompt-delivery-failed-follow-up.json");
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let first = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            "queue".to_owned(),
        )
        .await
        .expect("first prompt should enqueue locally");
    assert!(first.accepted);
    wait_for_prompt_ack_and_empty_outbox(&runtime, &first.client_mutation_id).await;

    harness.emit_prompt_delivery_failed_event(Some(PRESET_INFINITE), DELIVERY_FAILED_TITLE);
    wait_for_session_title(&runtime, DELIVERY_FAILED_TITLE).await;

    let follow_up = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            FOLLOW_UP_PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            "queue".to_owned(),
        )
        .await
        .expect("follow-up prompt should enqueue locally after delivery failure");
    assert!(follow_up.accepted);
    wait_for_prompt_ack_and_empty_outbox(&runtime, &follow_up.client_mutation_id).await;

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

struct E2eHarness {
    _fixture: TestControlPlaneFixture,
    runtime_dir: TempDir,
    control_plane: ControlPlane,
    bearer_token: String,
}

impl E2eHarness {
    async fn new() -> Self {
        let fixture = TestControlPlaneFixture::new();
        fixture.write_state_db();
        let control_plane = fixture.control_plane();
        let authorization_header =
            issue_mobile_authorization_header(&build_router(control_plane.clone())).await;
        let bearer_token = authorization_header
            .strip_prefix(BEARER_PREFIX)
            .unwrap_or(authorization_header.as_str())
            .to_owned();
        Self {
            _fixture: fixture,
            runtime_dir: TempDir::new().expect("runtime temp dir"),
            control_plane,
            bearer_token,
        }
    }

    fn runtime(&self, file_name: &str) -> Arc<LooperClientCoreSessionRuntime> {
        let file_path = self.runtime_dir.path().join(file_name);
        LooperClientCoreSessionRuntime::new(path_string(&file_path)).expect("client-core runtime")
    }

    fn seed_hook_session(&self, preset: Option<&str>, title: &str) {
        record_thread_active(&self.control_plane);
        if let Some(preset) = preset {
            self.control_plane
                .mobile_session_service()
                .set_session_preset(THREAD_ID, Some(preset))
                .expect("set initial session preset");
        }
        self.emit_session_mini_event(preset, title);
    }

    fn emit_session_mini_event(&self, preset: Option<&str>, title: &str) {
        self.control_plane
            .emit_mobile_session_event_with_cached_minis(
                MobileEventInput {
                    kind: MobileEventKind::SessionChanged,
                    thread_id: Some(THREAD_ID.to_owned()),
                    prompt_id: None,
                    detail: Some(title.to_owned()),
                },
                vec![session_mini_projection_input(preset, title)],
            );
    }

    fn emit_prompt_delivery_failed_event(&self, preset: Option<&str>, title: &str) {
        self.control_plane
            .emit_mobile_session_event_with_cached_minis(
                MobileEventInput {
                    kind: MobileEventKind::SessionChanged,
                    thread_id: Some(THREAD_ID.to_owned()),
                    prompt_id: None,
                    detail: Some(PROMPT_DELIVERY_FAILED_DETAIL.to_owned()),
                },
                vec![session_mini_projection_input(preset, title)],
            );
    }

    async fn spawn_http(&self) -> SpawnedHttp {
        SpawnedHttp::spawn(self.control_plane.clone()).await
    }
}

struct SpawnedHttp {
    address: SocketAddr,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<std::io::Result<()>>,
}

impl SpawnedHttp {
    async fn spawn(control_plane: ControlPlane) -> Self {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("bind HTTP listener");
        let address = listener.local_addr().expect("HTTP listener address");
        let router = build_router(control_plane);
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_task = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = shutdown_receiver.await;
            })
            .await
        });
        Self {
            address,
            shutdown_sender: Some(shutdown_sender),
            server_task,
        }
    }

    fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        tokio::time::timeout(HTTP_SHUTDOWN_TIMEOUT, self.server_task)
            .await
            .expect("HTTP server task should shut down")
            .expect("HTTP server task should not panic")
            .expect("HTTP server should exit cleanly");
    }
}

struct RestartableH2 {
    address: SocketAddr,
    server_task: tokio::task::JoinHandle<()>,
}

impl RestartableH2 {
    async fn spawn(control_plane: ControlPlane, address: Option<SocketAddr>) -> Self {
        let listener = tokio::net::TcpListener::bind(
            address.unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)),
        )
        .await
        .expect("bind restartable H2 listener");
        let address = listener.local_addr().expect("restartable H2 address");
        let server_task = tokio::spawn(async move {
            agent_control_plane::grpc::serve_with_listener(
                control_plane,
                listener,
                std::future::pending::<()>(),
            )
            .await
            .expect("restartable H2 server");
        });
        Self {
            address,
            server_task,
        }
    }

    async fn abort(self) -> SocketAddr {
        self.server_task.abort();
        let _ = self.server_task.await;
        self.address
    }
}

async fn spawn_loopback_h3(control_plane: ControlPlane) -> SpawnedH3 {
    let h3 = spawn_h3(
        control_plane,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
    )
    .await;
    h3.wait_until_ready().await;
    h3
}

fn start_runtime(
    runtime: &LooperClientCoreSessionRuntime,
    endpoints: Vec<ClientEndpoint>,
    bearer_token: &str,
) {
    let snapshot = runtime
        .start(endpoints, bearer_token.to_owned(), String::new())
        .expect("start client-core runtime");
    assert!(
        snapshot.outbox_depth == 0,
        "fresh runtime should start with empty outbox"
    );
}

async fn stop_runtime(runtime: Arc<LooperClientCoreSessionRuntime>) {
    let _ = runtime.stop();
    tokio::task::spawn_blocking(move || drop(runtime))
        .await
        .expect("drop client-core runtime");
}

fn endpoints(h3: &SpawnedH3, h2: &SpawnedH2, http: &SpawnedHttp) -> Vec<ClientEndpoint> {
    vec![
        h3_endpoint(h3.address, &http.base_url(), &h3.certificate_sha256, false),
        h2_endpoint(h2.address, &http.base_url(), false),
    ]
}

fn h2_only_endpoints(h2: &SpawnedH2, http: &SpawnedHttp) -> Vec<ClientEndpoint> {
    vec![h2_endpoint(h2.address, &http.base_url(), false)]
}

fn h3_endpoint(
    address: SocketAddr,
    recovery_base_url: &str,
    certificate_sha256: &str,
    last_good: bool,
) -> ClientEndpoint {
    ClientEndpoint {
        transport: ClientEndpointTransport::H3,
        url: format!("https://{address}"),
        recovery_base_url: recovery_base_url.to_owned(),
        h3_certificate_sha256: certificate_sha256.to_owned(),
        h3_certificate_spki_sha256: String::new(),
        last_good,
    }
}

fn h2_endpoint(address: SocketAddr, recovery_base_url: &str, last_good: bool) -> ClientEndpoint {
    ClientEndpoint {
        transport: ClientEndpointTransport::H2,
        url: format!("http://{address}"),
        recovery_base_url: recovery_base_url.to_owned(),
        h3_certificate_sha256: String::new(),
        h3_certificate_spki_sha256: String::new(),
        last_good,
    }
}

fn record_thread_active(control_plane: &ControlPlane) {
    control_plane
        .mobile_session_service()
        .record_hook_lifecycle(
            &MobileHookPayload {
                hook_event_name: "UserPromptSubmit".to_owned(),
                session_id: Some(THREAD_ID.to_owned()),
                turn_id: None,
                cwd: None,
                last_assistant_message: None,
            },
            false,
        )
        .expect("record active hook lifecycle");
}

fn session_mini_projection_input(
    preset: Option<&str>,
    title: &str,
) -> MobileSessionMiniProjectionInput {
    MobileSessionMiniProjectionInput {
        session_id: THREAD_ID.to_owned(),
        assistant_surface: ASSISTANT_SURFACE.to_owned(),
        body_json: serde_json::json!({
            "id": THREAD_ID,
            "sessionId": THREAD_ID,
            "assistantSurface": ASSISTANT_SURFACE,
            "title": title,
            "status": "active",
            "lifecycle": "active",
            "effectiveMode": preset,
            "replyable": true,
            "canSendPrompt": true,
            "queueCount": 0,
        }),
    }
}

async fn wait_for_prompt_ack_and_empty_outbox(
    runtime: &LooperClientCoreSessionRuntime,
    client_mutation_id: &str,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| {
        snapshot.outbox_depth == 0
            && snapshot.pending_mutations.is_empty()
            && snapshot
                .recent_command_acks
                .iter()
                .any(|ack| ack.client_mutation_id == client_mutation_id && ack.accepted)
            && runtime
                .local_snapshot()
                .map(|local| local.pending_commands.is_empty())
                .unwrap_or(false)
    })
    .await
}

async fn wait_for_mode_ack_and_projection(
    control_plane: &ControlPlane,
    runtime: &LooperClientCoreSessionRuntime,
    client_mutation_id: &str,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| {
        let server_preset = control_plane
            .mobile_session_service()
            .state()
            .ok()
            .and_then(|state| {
                state
                    .sessions
                    .get(THREAD_ID)
                    .and_then(|session| session.preset.clone())
            });
        snapshot.outbox_depth == 0
            && snapshot.pending_mutations.is_empty()
            && server_preset.as_deref() == Some(PRESET_AWAIT_REPLY)
            && snapshot
                .recent_command_acks
                .iter()
                .any(|ack| ack.client_mutation_id == client_mutation_id && ack.accepted)
    })
    .await
}

async fn wait_for_h3_ready_session(
    runtime: &LooperClientCoreSessionRuntime,
    title: &str,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| {
        snapshot.endpoint_transport == ClientEndpointTransport::H3
            && session_payload_contains(snapshot, "title", title)
    })
    .await
}

async fn wait_for_session_title(
    runtime: &LooperClientCoreSessionRuntime,
    title: &str,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| {
        session_payload_contains(snapshot, "title", title)
    })
    .await
}

async fn wait_for_effective_mode(
    runtime: &LooperClientCoreSessionRuntime,
    preset: &str,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| {
        session_payload_contains(snapshot, "effectiveMode", preset)
    })
    .await
}

async fn wait_for_transport(
    runtime: &LooperClientCoreSessionRuntime,
    transport: ClientEndpointTransport,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| snapshot.endpoint_transport == transport).await
}

async fn wait_for_snapshot(
    runtime: &LooperClientCoreSessionRuntime,
    mut predicate: impl FnMut(&ClientStateSnapshot) -> bool,
) -> ClientStateSnapshot {
    let mut last_snapshot = runtime.state_snapshot().expect("initial runtime snapshot");
    for _ in 0..POLL_ATTEMPTS {
        if predicate(&last_snapshot) {
            return last_snapshot;
        }

        match tokio::time::timeout(POLL_INTERVAL, runtime.observe()).await {
            Ok(Ok(update)) => last_snapshot = update.snapshot,
            Ok(Err(_)) | Err(_) => {
                last_snapshot = runtime.state_snapshot().expect("runtime snapshot");
            }
        }
    }

    panic!(
        "timed out waiting for client-core snapshot; last snapshot: {:?}",
        last_snapshot
    );
}

async fn wait_for_queued_prompt_count(control_plane: &ControlPlane, expected_count: i64) {
    let mut observed_count = None;
    for _ in 0..POLL_ATTEMPTS {
        let counts = control_plane
            .mobile_session_service()
            .queued_prompt_counts()
            .expect("queued prompt counts");
        observed_count = counts.get(THREAD_ID).copied();
        if observed_count == Some(expected_count) {
            return;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    panic!("timed out waiting for {expected_count} queued prompts; observed {observed_count:?}");
}

fn session_payload_contains(snapshot: &ClientStateSnapshot, key: &str, value: &str) -> bool {
    snapshot
        .state_minis
        .iter()
        .filter(|mini| mini.session_id == THREAD_ID)
        .any(|mini| {
            serde_json::from_str::<serde_json::Value>(&mini.payload_json)
                .ok()
                .and_then(|payload| {
                    payload
                        .get(key)
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .as_deref()
                == Some(value)
        })
}

fn path_string(path: &Path) -> String {
    path.to_str().expect("utf-8 temp path").to_owned()
}

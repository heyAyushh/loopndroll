mod support;

use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig, HostEnvironment};
use agent_control_plane::events::{MobileSessionMiniProjectionInput, MobileStateEventInput};
use agent_control_plane::grpc::proto::{self, looper_realtime_server::LooperRealtime};
use agent_control_plane::http::build_router;
use agent_control_plane::mobile::events::{MobileEventInput, MobileEventKind};
use agent_control_plane::mobile::session::MobileHookPayload;
use looper_client_core::{
    ClientEndpoint, ClientEndpointTransport, ClientStateSnapshot, LooperClientCoreSessionRuntime,
};
use tempfile::TempDir;
use tokio::sync::oneshot;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::transport::Server;
use tonic::{Request, Response, Status};

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
const FULL_BLACKOUT_RECOVERED_TITLE: &str = "E2E full blackout recovered";
const BACKGROUND_RESTART_TITLE: &str = "E2E background restart delta";
const BEARER_PREFIX: &str = "Bearer ";
const ACTIVE_SESSION_STATUS: &str = "active";
const STOPPED_SESSION_STATUS: &str = "stopped";
const PROMPT_INTENT_QUEUE: &str = "queue";
const PROMPT_INTENT_STEER: &str = "steer";
const COMMAND_KIND_SEND_SESSION_PROMPT: &str = "SendSessionPrompt";
const DETAIL_COMMAND_ACK: &str = "command-ack";
const DETAIL_SESSION_START: &str = "SessionStart";
const DETAIL_PROMPT_RESUMED: &str = "prompt-resumed";
const PROMPT_DELIVERY_FAILED_DETAIL: &str = "prompt-delivery-failed";
const MODE_REQUIRED_ERROR_CODE: &str = "mode_required";
const SESSION_BUSY_ERROR_CODE: &str = "session_busy";
const CURRENT_STATE_IDLE: &str = "idle";
const CURRENT_STATE_DISPATCHED: &str = "dispatched";
const DISPATCHED_SEED_MUTATION_ID: &str = "e2e-dispatched-seed";
const DUPLICATE_PROMPT_MUTATION_ID: &str = "e2e-duplicate-prompt";
const REJECTED_STEER_OUTBOX_FILE: &str = "steer-rejected-outbox.json";
const BACKGROUNDING_OUTBOX_FILE: &str = "backgrounding-warm-store.json";
const E2E_SERVER_TIME: &str = "2026-07-02T00:00:00Z";
const POLL_ATTEMPTS: usize = 160;
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const HTTP_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const GRPC_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
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
async fn steer_prompt_while_agent_running_is_accepted_and_dispatched() {
    let codex_stub_dir = TempDir::new().expect("codex stub temp dir");
    let codex_executable = write_codex_app_server_stub(codex_stub_dir.path());
    let harness = E2eHarness::new_with_codex_executable(codex_executable).await;
    harness.seed_agent_running_session(PRESET_INFINITE, INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime("steer-agent-running.json");
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let steer = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            PROMPT_INTENT_STEER.to_owned(),
        )
        .await
        .expect("steer prompt intent should be accepted locally");
    assert!(steer.accepted);

    wait_for_prompt_ack_and_empty_outbox(&runtime, &steer.client_mutation_id).await;
    wait_for_prompt_dispatch_recorded(&harness.control_plane).await;

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn rejected_steer_prompt_while_dispatched_finalizes_outbox_without_retry_wedge() {
    let harness = E2eHarness::new().await;
    harness.seed_dispatched_session(PRESET_INFINITE, INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime(REJECTED_STEER_OUTBOX_FILE);
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let steer = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            PROMPT_INTENT_STEER.to_owned(),
        )
        .await
        .expect("steer prompt should enqueue locally before server rejection");
    assert!(steer.accepted);

    let rejected = wait_for_rejected_ack_and_empty_outbox(
        &runtime,
        &steer.client_mutation_id,
        SESSION_BUSY_ERROR_CODE,
        CURRENT_STATE_DISPATCHED,
    )
    .await;
    assert!(
        rejected.reject_reason.contains("current_state=dispatched"),
        "rejected ack should preserve the FSM state in the wire reason: {:?}",
        rejected
    );

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn prompt_enqueued_during_full_blackout_delivers_after_both_listeners_restart() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = RestartableH2::spawn(harness.control_plane.clone(), None).await;
    let h3 = spawn_loopback_h3(harness.control_plane.clone()).await;
    let runtime = harness.runtime("full-blackout-reconnect.json");
    start_runtime(
        &runtime,
        vec![
            h3_endpoint(h3.address, &http.base_url(), &h3.certificate_sha256, false),
            h2_endpoint(h2.address, &http.base_url(), false),
        ],
        &harness.bearer_token,
    );
    wait_for_h3_ready_session(&runtime, INITIAL_TITLE).await;

    let h2_address = h2.abort().await;
    let h3_address = h3.shutdown().await;
    let prompt = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            PROMPT_INTENT_QUEUE.to_owned(),
        )
        .await
        .expect("prompt should enqueue locally during full listener blackout");
    assert!(prompt.accepted);
    assert_eq!(
        runtime
            .local_snapshot()
            .expect("local snapshot")
            .pending_commands
            .len(),
        1,
        "offline prompt must stay durable until a listener recovers"
    );

    let restarted_h2 = RestartableH2::spawn(harness.control_plane.clone(), Some(h2_address)).await;
    let restarted_h3 = spawn_h3(harness.control_plane.clone(), h3_address).await;
    restarted_h3.wait_until_ready().await;
    harness.emit_session_mini_event(Some(PRESET_AWAIT_REPLY), FULL_BLACKOUT_RECOVERED_TITLE);
    let recovered_seq = harness
        .control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest mobile state seq after full blackout recovery marker");

    wait_for_latest_seq_at_least(&runtime, recovered_seq).await;
    wait_for_prompt_ack_and_empty_outbox(&runtime, &prompt.client_mutation_id).await;
    wait_for_queued_prompt_count(&harness.control_plane, QUEUED_PROMPT_COUNT).await;

    stop_runtime(runtime).await;
    restarted_h3.shutdown().await;
    restarted_h2.abort().await;
    http.shutdown().await;
}

#[tokio::test]
async fn runtime_restart_renders_local_store_and_resumes_from_persisted_cursor() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime(BACKGROUNDING_OUTBOX_FILE);
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    let initial = wait_for_session_title(&runtime, INITIAL_TITLE).await;
    assert!(
        initial.latest_seq > 0,
        "warm local store needs a non-zero cursor before restart: {:?}",
        initial
    );
    stop_runtime(runtime).await;
    h2.shutdown().await;

    let restarted = harness.runtime(BACKGROUNDING_OUTBOX_FILE);
    let pre_start_snapshot = restarted
        .state_snapshot()
        .expect("restarted runtime seeded from local store before start");
    assert!(
        !pre_start_snapshot.state_minis.is_empty(),
        "state minis must render from local store before any server frame"
    );
    assert_eq!(
        pre_start_snapshot.latest_seq, initial.latest_seq,
        "fresh runtime should seed its resume cursor from the persisted local store"
    );
    assert_eq!(
        session_mini_count(&pre_start_snapshot),
        1,
        "pre-start local render should not duplicate already-seen minis"
    );

    let recorder = SpawnedResumeRecordingH2::spawn(state_delta_for_title(
        initial.latest_seq + 1,
        BACKGROUND_RESTART_TITLE,
    ))
    .await;
    start_runtime(
        &restarted,
        vec![h2_endpoint(recorder.address, &http.base_url(), false)],
        &harness.bearer_token,
    );

    let observed_after_seq =
        wait_for_recorded_resume_after_seq(&recorder, initial.latest_seq).await;
    assert!(
        observed_after_seq > 0,
        "background restart must resume from a non-zero cursor"
    );
    let recovered = wait_for_session_title(&restarted, BACKGROUND_RESTART_TITLE).await;
    assert_eq!(
        session_mini_count(&recovered),
        1,
        "post-resume delta should replace the existing mini instead of replaying a duplicate"
    );

    stop_runtime(restarted).await;
    recorder.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn stopped_session_steer_rejects_until_mode_is_armed_then_accepts_prompt() {
    let harness = E2eHarness::new().await;
    harness.seed_idle_session(INITIAL_TITLE);

    let http = harness.spawn_http().await;
    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let runtime = harness.runtime("stopped-session-mode-armed-flow.json");
    start_runtime(
        &runtime,
        h2_only_endpoints(&h2, &http),
        &harness.bearer_token,
    );
    wait_for_session_title(&runtime, INITIAL_TITLE).await;

    let rejected_prompt = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            PROMPT_INTENT_STEER.to_owned(),
        )
        .await
        .expect("stopped steer should enqueue locally before server rejection");
    assert!(rejected_prompt.accepted);
    wait_for_rejected_ack_and_empty_outbox(
        &runtime,
        &rejected_prompt.client_mutation_id,
        MODE_REQUIRED_ERROR_CODE,
        CURRENT_STATE_IDLE,
    )
    .await;

    let mode = runtime
        .set_mode(THREAD_ID.to_owned(), PRESET_AWAIT_REPLY.to_owned())
        .await
        .expect("set mode intent should be accepted locally");
    assert!(mode.accepted);
    wait_for_mode_ack_and_projection(&harness.control_plane, &runtime, &mode.client_mutation_id)
        .await;

    let accepted_prompt = runtime
        .send_prompt(
            THREAD_ID.to_owned(),
            PROMPT_TEXT.to_owned(),
            ASSISTANT_SURFACE.to_owned(),
            PROMPT_INTENT_STEER.to_owned(),
        )
        .await
        .expect("armed steer prompt should enqueue locally");
    assert!(accepted_prompt.accepted);
    wait_for_prompt_ack_and_empty_outbox(&runtime, &accepted_prompt.client_mutation_id).await;

    stop_runtime(runtime).await;
    h2.shutdown().await;
    http.shutdown().await;
}

#[tokio::test]
async fn duplicate_client_mutation_id_send_prompt_replays_ack_and_executes_once() {
    let harness = E2eHarness::new().await;
    harness.seed_hook_session(Some(PRESET_AWAIT_REPLY), INITIAL_TITLE);

    let h2 = spawn_h2_client(harness.control_plane.clone()).await;
    let mut client = h2.client.clone();
    let request = proto::SendSessionPromptRequest {
        thread_id: THREAD_ID.to_owned(),
        prompt: PROMPT_TEXT.to_owned(),
        assistant_surface: ASSISTANT_SURFACE.to_owned(),
        client_mutation_id: DUPLICATE_PROMPT_MUTATION_ID.to_owned(),
        prompt_intent: PROMPT_INTENT_QUEUE.to_owned(),
    };

    let first = client
        .send_session_prompt(authorized_grpc_request(
            request.clone(),
            &harness.bearer_token,
        ))
        .await
        .expect("first prompt command should ack")
        .into_inner();
    let second = client
        .send_session_prompt(authorized_grpc_request(request, &harness.bearer_token))
        .await
        .expect("duplicate prompt command should replay ack")
        .into_inner();

    assert!(first.accepted, "first ack should accept: {:?}", first);
    assert!(second.accepted, "replayed ack should accept: {:?}", second);
    assert!(!first.idempotent_replay);
    assert!(second.idempotent_replay);
    assert_eq!(second.client_mutation_id, first.client_mutation_id);
    assert_eq!(second.ack_seq, first.ack_seq);
    assert_eq!(second.revision, first.revision);
    wait_for_queued_prompt_count(&harness.control_plane, QUEUED_PROMPT_COUNT).await;

    h2.shutdown().await;
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
    // Wait on the server-side FSM gate rather than a projected title: the
    // session-mini reconciler can legitimately regenerate minis from source
    // truth and overwrite the marker title without affecting the gate.
    wait_for_server_fsm_to_allow_prompt(&harness.control_plane).await;

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
    _standard_fixture: Option<TestControlPlaneFixture>,
    _custom_fixture: Option<TempDir>,
    runtime_dir: TempDir,
    control_plane: ControlPlane,
    bearer_token: String,
}

impl E2eHarness {
    async fn new() -> Self {
        let fixture = TestControlPlaneFixture::new();
        fixture.write_state_db();
        let control_plane = fixture.control_plane();
        Self::from_control_plane(Some(fixture), None, control_plane).await
    }

    async fn new_with_codex_executable(codex_executable: String) -> Self {
        let temp_dir = TempDir::new().expect("custom fixture temp dir");
        let codex_home = temp_dir.path().join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).expect("codex dirs");
        fs::create_dir_all(temp_dir.path().join(".grok/sessions")).expect("grok dirs");
        write_e2e_state_db(&codex_home);
        let control_plane = ControlPlane::new(ControlPlaneConfig {
            codex_home,
            codex_executable: Some(codex_executable),
            store_path: temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
            host_environment: HostEnvironment::hermetic(temp_dir.path().to_path_buf()),
        });
        Self::from_control_plane(None, Some(temp_dir), control_plane).await
    }

    async fn from_control_plane(
        standard_fixture: Option<TestControlPlaneFixture>,
        custom_fixture: Option<TempDir>,
        control_plane: ControlPlane,
    ) -> Self {
        let authorization_header =
            issue_mobile_authorization_header(&build_router(control_plane.clone())).await;
        let bearer_token = authorization_header
            .strip_prefix(BEARER_PREFIX)
            .unwrap_or(authorization_header.as_str())
            .to_owned();
        Self {
            _standard_fixture: standard_fixture,
            _custom_fixture: custom_fixture,
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

    fn seed_agent_running_session(&self, preset: &str, title: &str) {
        record_thread_active(&self.control_plane);
        self.control_plane
            .mobile_session_service()
            .set_session_preset(THREAD_ID, Some(preset))
            .expect("set agent-running session preset");
        self.emit_session_event_with_detail(Some(preset), title, DETAIL_SESSION_START);
    }

    fn seed_dispatched_session(&self, preset: &str, title: &str) {
        self.seed_agent_running_session(preset, title);
        self.record_prompt_accepted_without_delivery(DISPATCHED_SEED_MUTATION_ID);
    }

    fn seed_idle_session(&self, title: &str) {
        self.control_plane
            .emit_mobile_session_event_with_cached_minis(
                MobileEventInput {
                    kind: MobileEventKind::SessionChanged,
                    thread_id: Some(THREAD_ID.to_owned()),
                    prompt_id: None,
                    detail: Some(title.to_owned()),
                },
                vec![session_mini_projection_input_with_status(
                    None,
                    title,
                    STOPPED_SESSION_STATUS,
                    STOPPED_SESSION_STATUS,
                    true,
                    true,
                )],
            );
    }

    fn emit_session_mini_event(&self, preset: Option<&str>, title: &str) {
        self.emit_session_event_with_detail(preset, title, title);
    }

    fn emit_session_event_with_detail(&self, preset: Option<&str>, title: &str, detail: &str) {
        self.control_plane
            .emit_mobile_session_event_with_cached_minis(
                MobileEventInput {
                    kind: MobileEventKind::SessionChanged,
                    thread_id: Some(THREAD_ID.to_owned()),
                    prompt_id: None,
                    detail: Some(detail.to_owned()),
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

    fn record_prompt_accepted_without_delivery(&self, client_mutation_id: &str) {
        let revision = format!("e2e-accepted-{client_mutation_id}");
        self.control_plane
            .store()
            .record_mobile_state_event(MobileStateEventInput {
                entity_id: THREAD_ID.to_owned(),
                kind: MobileEventKind::SessionChanged,
                revision: revision.clone(),
                server_time: E2E_SERVER_TIME.to_owned(),
                payload_json: serde_json::json!({
                    "threadId": THREAD_ID,
                    "detail": DETAIL_COMMAND_ACK,
                }),
                client_mutation_id: Some(client_mutation_id.to_owned()),
                command_kind: Some(COMMAND_KIND_SEND_SESSION_PROMPT.to_owned()),
                command_request_hash: Some(format!("e2e-request-{client_mutation_id}")),
                command_response_json: Some(serde_json::json!({
                    "accepted": true,
                    "dispatchKind": "accepted",
                    "entityId": THREAD_ID,
                    "threadId": THREAD_ID,
                    "revision": revision,
                    "serverTime": E2E_SERVER_TIME,
                })),
            })
            .expect("record accepted prompt state event");
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

struct SpawnedResumeRecordingH2 {
    address: SocketAddr,
    observed_after_seq: Arc<tokio::sync::Mutex<Option<i64>>>,
    shutdown_sender: Option<oneshot::Sender<()>>,
    server_task: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl SpawnedResumeRecordingH2 {
    async fn spawn(delta: proto::StateMiniDelta) -> Self {
        let listener =
            tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                .await
                .expect("bind resume-recording H2 listener");
        let address = listener.local_addr().expect("resume-recording H2 address");
        let observed_after_seq = Arc::new(tokio::sync::Mutex::new(None));
        let service = ResumeRecordingRealtimeService {
            observed_after_seq: observed_after_seq.clone(),
            delta,
        };
        let (shutdown_sender, shutdown_receiver) = oneshot::channel();
        let server_task = tokio::spawn(async move {
            Server::builder()
                .add_service(proto::looper_realtime_server::LooperRealtimeServer::new(
                    service,
                ))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = shutdown_receiver.await;
                })
                .await
        });
        Self {
            address,
            observed_after_seq,
            shutdown_sender: Some(shutdown_sender),
            server_task,
        }
    }

    async fn observed_after_seq(&self) -> Option<i64> {
        *self.observed_after_seq.lock().await
    }

    async fn shutdown(mut self) {
        if let Some(sender) = self.shutdown_sender.take() {
            let _ = sender.send(());
        }
        tokio::time::timeout(GRPC_SHUTDOWN_TIMEOUT, self.server_task)
            .await
            .expect("resume-recording H2 server task should shut down")
            .expect("resume-recording H2 server task should not panic")
            .expect("resume-recording H2 server should exit cleanly");
    }
}

#[derive(Clone)]
struct ResumeRecordingRealtimeService {
    observed_after_seq: Arc<tokio::sync::Mutex<Option<i64>>>,
    delta: proto::StateMiniDelta,
}

#[derive(Debug)]
struct ObservedRejectedAck {
    reject_reason: String,
}

fn unsupported_resume_recorder_unary() -> Result<Response<proto::CommandAck>, Status> {
    Err(Status::unimplemented(
        "resume recorder only supports Session",
    ))
}

#[tonic::async_trait]
impl LooperRealtime for ResumeRecordingRealtimeService {
    type SessionStream = ReceiverStream<Result<proto::ServerFrame, Status>>;

    async fn health(
        &self,
        _request: Request<proto::HealthRequest>,
    ) -> Result<Response<proto::HealthResponse>, Status> {
        Ok(Response::new(proto::HealthResponse {
            ok: true,
            service: "resume-recorder".to_owned(),
            server_time: E2E_SERVER_TIME.to_owned(),
        }))
    }

    async fn session(
        &self,
        request: Request<tonic::Streaming<proto::ClientFrame>>,
    ) -> Result<Response<Self::SessionStream>, Status> {
        let mut inbound = request.into_inner();
        let observed_after_seq = self.observed_after_seq.clone();
        let delta = self.delta.clone();
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        tokio::spawn(async move {
            if let Ok(Some(frame)) = inbound.message().await {
                if let Some(proto::client_frame::Frame::Resume(resume)) = frame.frame {
                    *observed_after_seq.lock().await = Some(resume.after_seq);
                }
            }
            let _ = sender
                .send(Ok(proto::ServerFrame {
                    frame: Some(proto::server_frame::Frame::StateDelta(delta)),
                }))
                .await;
        });
        Ok(Response::new(ReceiverStream::new(receiver)))
    }

    async fn set_session_mode(
        &self,
        _request: Request<proto::SetSessionModeRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn send_session_prompt(
        &self,
        _request: Request<proto::SendSessionPromptRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn submit_notification_reply(
        &self,
        _request: Request<proto::SubmitNotificationReplyRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_siri_current_session(
        &self,
        _request: Request<proto::SetSiriCurrentSessionRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_siri_default_session(
        &self,
        _request: Request<proto::SetSiriDefaultSessionRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn save_default_prompt(
        &self,
        _request: Request<proto::SaveDefaultPromptRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_session_archived(
        &self,
        _request: Request<proto::SetSessionArchivedRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn delete_session(
        &self,
        _request: Request<proto::DeleteSessionRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn mute_session(
        &self,
        _request: Request<proto::MuteSessionRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_scope(
        &self,
        _request: Request<proto::SetScopeRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_global_preset(
        &self,
        _request: Request<proto::SetGlobalPresetRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_global_notification(
        &self,
        _request: Request<proto::SetGlobalNotificationRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_default_notification_targets(
        &self,
        _request: Request<proto::SetDefaultNotificationTargetsRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_global_completion_check(
        &self,
        _request: Request<proto::SetGlobalCompletionCheckRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn upsert_notification_route(
        &self,
        _request: Request<proto::UpsertNotificationRouteRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn delete_notification_route(
        &self,
        _request: Request<proto::DeleteNotificationRouteRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn upsert_completion_check(
        &self,
        _request: Request<proto::UpsertCompletionCheckRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn delete_completion_check(
        &self,
        _request: Request<proto::DeleteCompletionCheckRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_session_notifications(
        &self,
        _request: Request<proto::SetSessionNotificationsRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_session_completion_check(
        &self,
        _request: Request<proto::SetSessionCompletionCheckRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
    }

    async fn set_assistant_surface(
        &self,
        _request: Request<proto::SetAssistantSurfaceRequest>,
    ) -> Result<Response<proto::CommandAck>, Status> {
        unsupported_resume_recorder_unary()
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

fn write_e2e_state_db(codex_home: &Path) {
    let connection = rusqlite::Connection::open(codex_home.join("state_1.sqlite")).expect("state");
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
    rusqlite::Connection::open(codex_home.join("logs_1.sqlite")).expect("logs");
}

fn write_codex_app_server_stub(directory: &Path) -> String {
    let executable_path = directory.join("codex-stub");
    fs::write(
        &executable_path,
        r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"id":"looper-initialize"'*) printf '%s\n' '{"id":"looper-initialize","result":{}}' ;;
    *'"id":"looper-thread-resume"'*) printf '%s\n' '{"id":"looper-thread-resume","result":{"thread":{"id":"thread-main"}}}' ;;
    *'"id":"looper-turn-start"'*)
      printf '%s\n' '{"id":"looper-turn-start","result":{"turn":{"id":"turn-main","status":"inProgress"}}}'
      printf '%s\n' '{"method":"turn/completed","params":{"threadId":"thread-main","turn":{"id":"turn-main","status":"completed"}}}'
      ;;
  esac
done
"#,
    )
    .expect("write codex app-server stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(&executable_path)
            .expect("codex stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable_path, permissions).expect("chmod codex stub");
    }
    path_string(&executable_path)
}

fn session_mini_projection_input(
    preset: Option<&str>,
    title: &str,
) -> MobileSessionMiniProjectionInput {
    session_mini_projection_input_with_status(
        preset,
        title,
        ACTIVE_SESSION_STATUS,
        ACTIVE_SESSION_STATUS,
        true,
        true,
    )
}

fn session_mini_projection_input_with_status(
    preset: Option<&str>,
    title: &str,
    lifecycle: &str,
    status: &str,
    replyable: bool,
    can_send_prompt: bool,
) -> MobileSessionMiniProjectionInput {
    MobileSessionMiniProjectionInput {
        session_id: THREAD_ID.to_owned(),
        assistant_surface: ASSISTANT_SURFACE.to_owned(),
        body_json: session_mini_payload_json_with_status(
            preset,
            title,
            0,
            "",
            lifecycle,
            status,
            replyable,
            can_send_prompt,
        ),
    }
}

fn session_mini_payload_json(
    preset: Option<&str>,
    title: &str,
    seq: i64,
    revision: &str,
) -> serde_json::Value {
    session_mini_payload_json_with_status(
        preset,
        title,
        seq,
        revision,
        ACTIVE_SESSION_STATUS,
        ACTIVE_SESSION_STATUS,
        true,
        true,
    )
}

fn session_mini_payload_json_with_status(
    preset: Option<&str>,
    title: &str,
    seq: i64,
    revision: &str,
    lifecycle: &str,
    status: &str,
    replyable: bool,
    can_send_prompt: bool,
) -> serde_json::Value {
    serde_json::json!({
        "id": THREAD_ID,
        "sessionId": THREAD_ID,
        "sessionID": THREAD_ID,
        "assistantSurface": ASSISTANT_SURFACE,
        "title": title,
        "status": status,
        "lifecycle": lifecycle,
        "effectiveMode": preset,
        "replyable": replyable,
        "canSendPrompt": can_send_prompt,
        "queueCount": 0,
        "seq": seq,
        "revision": revision,
    })
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

async fn wait_for_rejected_ack_and_empty_outbox(
    runtime: &LooperClientCoreSessionRuntime,
    client_mutation_id: &str,
    error_code: &str,
    current_state: &str,
) -> ObservedRejectedAck {
    let snapshot = wait_for_snapshot(runtime, |snapshot| {
        snapshot.outbox_depth == 0
            && snapshot.pending_mutations.is_empty()
            && snapshot.recent_command_acks.iter().any(|ack| {
                ack.client_mutation_id == client_mutation_id
                    && !ack.accepted
                    && ack.error_code == error_code
                    && ack.current_state == current_state
            })
            && runtime
                .local_snapshot()
                .map(|local| local.pending_commands.is_empty())
                .unwrap_or(false)
    })
    .await;
    snapshot
        .recent_command_acks
        .into_iter()
        .find(|ack| ack.client_mutation_id == client_mutation_id)
        .map(|ack| ObservedRejectedAck {
            reject_reason: ack.reject_reason,
        })
        .expect("rejected ack in snapshot")
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

async fn wait_for_latest_seq_at_least(
    runtime: &LooperClientCoreSessionRuntime,
    target_seq: i64,
) -> ClientStateSnapshot {
    wait_for_snapshot(runtime, |snapshot| snapshot.latest_seq >= target_seq).await
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

/// Polls the same FSM resolution the realtime command gate uses until a new
/// prompt would be accepted for the e2e thread. Immune to session-mini
/// reconciler rewrites, which only affect projected presentation fields.
async fn wait_for_server_fsm_to_allow_prompt(control_plane: &ControlPlane) {
    use agent_control_plane::control_plane::reducer::session_state_for_thread;
    use agent_control_plane::control_plane::session_fsm::{SessionCommand, next};

    for _ in 0..POLL_ATTEMPTS {
        let minis = control_plane
            .store()
            .mobile_session_minis_for_session(THREAD_ID)
            .expect("session minis for fsm wait");
        let events = control_plane
            .store()
            .mobile_state_events_for_entity(THREAD_ID)
            .expect("state events for fsm wait");
        let state = session_state_for_thread(&events, &minis, THREAD_ID, Some(ASSISTANT_SURFACE));
        if next(
            state,
            SessionCommand::SendPrompt {
                client_mutation_id: "fsm-wait-probe".to_owned(),
            },
        )
        .is_ok()
        {
            return;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    panic!("timed out waiting for server FSM to accept a follow-up prompt");
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

async fn wait_for_prompt_dispatch_recorded(control_plane: &ControlPlane) {
    for _ in 0..POLL_ATTEMPTS {
        let queued_count = control_plane
            .mobile_session_service()
            .queued_prompt_counts()
            .expect("queued prompt counts")
            .get(THREAD_ID)
            .copied()
            .unwrap_or_default();
        if queued_count > 0 || has_session_detail_event(control_plane, DETAIL_PROMPT_RESUMED) {
            return;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    panic!("timed out waiting for prompt dispatch to be recorded");
}

async fn wait_for_recorded_resume_after_seq(
    recorder: &SpawnedResumeRecordingH2,
    expected_after_seq: i64,
) -> i64 {
    for _ in 0..POLL_ATTEMPTS {
        if let Some(after_seq) = recorder.observed_after_seq().await {
            assert_eq!(
                after_seq, expected_after_seq,
                "Session resume should use the local-store cursor"
            );
            return after_seq;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    panic!("timed out waiting for resume frame after_seq={expected_after_seq}");
}

fn has_session_detail_event(control_plane: &ControlPlane, detail: &str) -> bool {
    control_plane
        .store()
        .mobile_state_events_for_entity(THREAD_ID)
        .expect("state events for prompt dispatch")
        .iter()
        .any(|record| {
            serde_json::from_str::<serde_json::Value>(&record.payload_json)
                .ok()
                .and_then(|payload| {
                    payload
                        .get("detail")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .as_deref()
                == Some(detail)
        })
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

fn session_mini_count(snapshot: &ClientStateSnapshot) -> usize {
    snapshot
        .state_minis
        .iter()
        .filter(|mini| mini.session_id == THREAD_ID)
        .count()
}

fn path_string(path: &Path) -> String {
    path.to_str().expect("utf-8 temp path").to_owned()
}

fn authorized_grpc_request<T>(message: T, bearer_token: &str) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        "authorization",
        format!("{BEARER_PREFIX}{bearer_token}")
            .parse()
            .expect("authorization metadata"),
    );
    request
}

fn state_delta_for_title(seq: i64, title: &str) -> proto::StateMiniDelta {
    let revision = format!("e2e-delta-{seq}");
    proto::StateMiniDelta {
        seq,
        entity_id: THREAD_ID.to_owned(),
        kind: "session.changed".to_owned(),
        revision: revision.clone(),
        server_time: E2E_SERVER_TIME.to_owned(),
        payload_json: session_mini_payload_json(Some(PRESET_AWAIT_REPLY), title, seq, &revision)
            .to_string(),
    }
}

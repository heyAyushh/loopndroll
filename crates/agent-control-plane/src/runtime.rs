// allow: SIZE_OK — runtime lifecycle boundary centralizes signal, hook, and server shutdown ownership to avoid split-brain process control.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use tokio::net::TcpListener;

use crate::control_plane::{ControlPlane, ControlPlaneConfig, HostEnvironment};
use crate::grok_build::{GrokContinueRequest, spawn_session_continue};
use crate::hooks::adapter::{HookAdapterKind, StopDelivery, empty_hook_payload, parse_input_json};
use crate::http::build_router;
use crate::mobile::network::{
    BonjourAdvertisement, DEFAULT_AGENT_CONTROL_PLANE_PORT, default_grpc_listen_address,
};
use crate::mobile::session::MobileHookPayload;
use crate::scheduler::AutomationRunner;

const DEFAULT_STORE_RELATIVE_PATH: &str =
    "Library/Application Support/looper/agent-control-plane.sqlite";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const HOME_ENV: &str = "HOME";
const LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_LISTEN";
const STORE_ENV: &str = "AGENT_CONTROL_PLANE_STORE";
const AUTOMATION_TICK_SECONDS: u64 = 30;
const TELEGRAM_BRIDGE_TICK_SECONDS: u64 = 5;
const SESSION_MINI_RECONCILE_TICK: Duration = Duration::from_millis(250);
const NANOS_PER_MILLISECOND: i64 = 1_000_000;
const SERVER_EXECUTABLE_NAME: &str = "looper-server";
const DEFAULT_SERVER_SCHEME: &str = "http";
const STOP_HOOK_EVENT: &str = "Stop";

pub async fn run_server() -> Result<()> {
    let control_plane = default_control_plane()?;
    spawn_automation_runner(control_plane.clone());
    spawn_telegram_bridge(control_plane.clone());
    spawn_session_mini_projection_reconciler(control_plane.clone());

    let listener = TcpListener::bind(default_listen_address()?).await?;
    let local_address = listener.local_addr()?;
    let grpc_listener = TcpListener::bind(default_grpc_listen_address(local_address)?).await?;
    let grpc_local_address = grpc_listener.local_addr()?;
    spawn_grpc_server(control_plane.clone(), grpc_listener, grpc_local_address);
    spawn_grpc_h3_server(control_plane.clone(), grpc_local_address);
    let _bonjour_advertisement = match BonjourAdvertisement::start_for_listener(local_address) {
        Ok(advertisement) => advertisement,
        Err(error) => {
            eprintln!("mobile Bonjour advertisement failed: {error}");
            None
        }
    };
    axum::serve(
        listener,
        build_router(control_plane).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

fn spawn_grpc_server(
    control_plane: ControlPlane,
    listener: TcpListener,
    listen_address: SocketAddr,
) {
    tokio::spawn(async move {
        eprintln!("looper gRPC listening on {listen_address}");
        if let Err(error) =
            crate::grpc::serve_with_listener(control_plane, listener, shutdown_signal()).await
        {
            eprintln!("looper gRPC server failed: {error}");
        }
    });
}

fn spawn_grpc_h3_server(control_plane: ControlPlane, h2_listen_address: SocketAddr) {
    tokio::spawn(async move {
        let listen_address = match crate::grpc::default_h3_listen_address(h2_listen_address) {
            Ok(listen_address) => listen_address,
            Err(error) => {
                eprintln!("looper H3 gRPC listener address failed: {error}");
                return;
            }
        };
        match crate::grpc::spawn_h3_server(control_plane, listen_address, shutdown_signal()).await {
            Ok(server) => {
                eprintln!(
                    "looper H3 gRPC listening on {} pin={}",
                    server.listen_address, server.certificate_sha256
                );
                if let Err(error) = server.server_task.await {
                    eprintln!("looper H3 gRPC server task failed: {error}");
                }
            }
            Err(error) => eprintln!("looper H3 gRPC server failed: {error}"),
        }
    });
}

fn spawn_session_mini_projection_reconciler(control_plane: ControlPlane) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SESSION_MINI_RECONCILE_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            control_plane.spawn_mobile_session_mini_projection_reconcile_if_source_changed();
        }
    });
}

pub async fn run_hook_mode() -> Result<()> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    run_hook_mode_with_input(
        &input,
        HookInvocationContext::from_environment(),
        default_control_plane()?,
    )
    .await
}

struct HookInvocationContext {
    adapter: HookAdapterKind,
}

impl HookInvocationContext {
    fn from_environment() -> Self {
        Self {
            adapter: HookAdapterKind::from_environment(),
        }
    }
}

fn parse_hook_mode_payload(input: &str, context: &HookInvocationContext) -> MobileHookPayload {
    let Ok(stdin_json) = parse_input_json(input, "parse hook payload JSON from stdin") else {
        return empty_hook_payload();
    };
    context
        .adapter
        .adapter()
        .parse_payload(&stdin_json)
        .unwrap_or_else(empty_hook_payload)
}

async fn run_hook_mode_with_input(
    input: &str,
    context: HookInvocationContext,
    control_plane: ControlPlane,
) -> Result<()> {
    let payload = parse_hook_mode_payload(input, &context);
    let service = control_plane.mobile_session_service();
    let outcome = service.hook_outcome_for_payload(&payload)?;
    service.record_hook_lifecycle(&payload, outcome.decision.is_some())?;
    if let Some(thread_id) = payload.session_id.as_deref() {
        control_plane.emit_mobile_session_event(
            crate::mobile::events::MobileEventInput {
                kind: crate::mobile::events::MobileEventKind::SessionChanged,
                thread_id: Some(thread_id.to_owned()),
                prompt_id: None,
                detail: Some(payload.hook_event_name.clone()),
            },
            thread_id,
        );
        if let Some(prompt_id) = outcome.delivered_prompt_id.as_deref() {
            control_plane.emit_mobile_session_event(
                crate::mobile::events::MobileEventInput {
                    kind: crate::mobile::events::MobileEventKind::PromptDelivered,
                    thread_id: Some(thread_id.to_owned()),
                    prompt_id: Some(prompt_id.to_owned()),
                    detail: None,
                },
                thread_id,
            );
        }
        if let Some(decision) = outcome.decision.as_ref() {
            control_plane.emit_mobile_session_event(
                crate::mobile::events::MobileEventInput {
                    kind: crate::mobile::events::MobileEventKind::LifecycleChanged,
                    thread_id: Some(thread_id.to_owned()),
                    prompt_id: None,
                    detail: Some(decision.reason.clone()),
                },
                thread_id,
            );
        }
    }
    if outcome.decision.is_none()
        && payload.hook_event_name == STOP_HOOK_EVENT
        && let Err(error) =
            crate::hook_notifications::send_stop_notifications(&control_plane, &payload).await
    {
        eprintln!("stop notification delivery failed: {error}");
    }
    if let Some(decision) = outcome.decision {
        match context.adapter.adapter().deliver_stop_decision(&decision) {
            StopDelivery::Stdout(decision) => {
                println!("{}", serde_json::to_string(&decision)?);
            }
            StopDelivery::SpawnContinue { prompt } => {
                if let Some(session_id) = payload.session_id.as_deref()
                    && let Err(error) = spawn_session_continue(&GrokContinueRequest {
                        session_id: session_id.to_owned(),
                        prompt,
                        cwd: payload.cwd.clone(),
                        grok_executable: None,
                        grok_home: Some(control_plane.grok_home().clone()),
                    })
                {
                    eprintln!("grok session continue failed: {error}");
                }
            }
        }
    }
    Ok(())
}

pub fn default_control_plane() -> Result<ControlPlane> {
    let home_path = home_dir();
    let grok_home = crate::grok_build::default_grok_home(&home_path);
    Ok(ControlPlane::new(ControlPlaneConfig {
        codex_home: default_codex_home(),
        codex_executable: None,
        claude_executable: None,
        store_path: default_store_path(),
        hook_command: Some(default_hook_command()?),
        host_environment: HostEnvironment::real_with_grok_home(home_path, grok_home),
    }))
}

pub fn default_server_base_url() -> String {
    std::env::var(LISTEN_ENV)
        .ok()
        .and_then(|listen| server_base_url_for_listen(&listen))
        .unwrap_or_else(|| {
            format!("{DEFAULT_SERVER_SCHEME}://127.0.0.1:{DEFAULT_AGENT_CONTROL_PLANE_PORT}")
        })
}

pub fn default_store_path() -> PathBuf {
    std::env::var(STORE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(DEFAULT_STORE_RELATIVE_PATH))
}

pub fn quote_shell_path(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let Ok(mut interrupt) = signal(SignalKind::interrupt()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        let Ok(mut terminate) = signal(SignalKind::terminate()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        let Ok(mut hangup) = signal(SignalKind::hangup()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };

        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
            _ = hangup.recv() => {}
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn default_codex_home() -> PathBuf {
    std::env::var(CODEX_HOME_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| home_dir().join(".codex"))
}

fn default_listen_address() -> Result<SocketAddr> {
    Ok(std::env::var(LISTEN_ENV)
        .unwrap_or_else(|_| format!("127.0.0.1:{DEFAULT_AGENT_CONTROL_PLANE_PORT}"))
        .parse::<SocketAddr>()?)
}

fn server_base_url_for_listen(listen: &str) -> Option<String> {
    let address = listen.parse::<SocketAddr>().ok()?;
    Some(format!(
        "{DEFAULT_SERVER_SCHEME}://{}:{}",
        client_host_for_listen_ip(address.ip()),
        address.port()
    ))
}

fn client_host_for_listen_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) if ip.is_unspecified() => Ipv4Addr::LOCALHOST.to_string(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => format!("[{}]", Ipv6Addr::LOCALHOST),
        IpAddr::V6(ip) => format!("[{ip}]"),
    }
}

fn default_hook_command() -> Result<String> {
    let executable = hook_executable_path(std::env::current_exe()?);
    Ok(format!(
        "{} --hook --managed-by looper",
        quote_shell_path(&executable)
    ))
}

fn hook_executable_path(current_executable: PathBuf) -> PathBuf {
    let executable_name = current_executable
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if matches!(
        executable_name,
        SERVER_EXECUTABLE_NAME | "agent-control-plane"
    ) {
        return current_executable;
    }
    let Some(parent) = current_executable.parent() else {
        return current_executable;
    };
    let sibling_server = parent.join(SERVER_EXECUTABLE_NAME);
    if sibling_server.is_file() {
        sibling_server
    } else {
        current_executable
    }
}

fn home_dir() -> PathBuf {
    std::env::var(HOME_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn spawn_automation_runner(control_plane: ControlPlane) {
    tokio::spawn(async move {
        let mut runner = AutomationRunner::new(control_plane);
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(AUTOMATION_TICK_SECONDS));
        loop {
            interval.tick().await;
            let now_ms = time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64
                / NANOS_PER_MILLISECOND;
            if let Err(error) = runner.tick(now_ms) {
                eprintln!("automation mirror tick failed: {error}");
            }
        }
    });
}

fn spawn_telegram_bridge(control_plane: ControlPlane) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(TELEGRAM_BRIDGE_TICK_SECONDS));
        loop {
            interval.tick().await;
            if let Err(error) = crate::telegram_bridge::poll_once(&control_plane).await {
                eprintln!("telegram bridge tick failed: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_HOOK_COMMAND: &str = "agent-control-plane --hook --managed-by looper";
    const TEST_STOP_HOOK_INPUT: &str = r#"{
        "hook_event_name": "Stop",
        "session_id": "thread-main",
        "last_assistant_message": "done"
    }"#;

    #[tokio::test]
    async fn stop_hook_mode_runs_inside_existing_runtime() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let control_plane = ControlPlane::new(ControlPlaneConfig {
            codex_home: temp_dir.path().join(".codex"),
            codex_executable: None,
            claude_executable: Some("/usr/bin/false".to_owned()),
            store_path: temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some(TEST_HOOK_COMMAND.to_owned()),
            host_environment: HostEnvironment::hermetic(temp_dir.path().to_path_buf()),
        });
        let context = HookInvocationContext {
            adapter: HookAdapterKind::Codex,
        };

        run_hook_mode_with_input(TEST_STOP_HOOK_INPUT, context, control_plane)
            .await
            .expect("run stop hook mode inside tokio runtime");
    }

    #[test]
    fn listen_wildcard_maps_to_loopback_client_url() {
        assert_eq!(
            server_base_url_for_listen("0.0.0.0:9000").as_deref(),
            Some("http://127.0.0.1:9000")
        );
    }

    #[test]
    fn listen_ipv6_wildcard_maps_to_loopback_client_url() {
        assert_eq!(
            server_base_url_for_listen("[::]:9001").as_deref(),
            Some("http://[::1]:9001")
        );
    }
}

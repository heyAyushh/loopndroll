use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::Result;
use tokio::net::TcpListener;

use crate::control_plane::{ControlPlane, ControlPlaneConfig};
use crate::grok_build::{
    GrokContinueRequest, is_grok_hook_invocation, parse_hook_payload, spawn_session_continue,
};
use crate::http::build_router;
use crate::mobile_network::{BonjourAdvertisement, DEFAULT_AGENT_CONTROL_PLANE_PORT};
use crate::mobile_session::MobileHookPayload;
use crate::scheduler::AutomationRunner;

const DEFAULT_STORE_RELATIVE_PATH: &str =
    "Library/Application Support/looper/agent-control-plane.sqlite";
const DEFAULT_LEGACY_BUN_STORE_RELATIVE_PATH: &str = "Library/Application Support/looper/app.db";
const CODEX_HOME_ENV: &str = "CODEX_HOME";
const HOME_ENV: &str = "HOME";
const LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_LISTEN";
const STORE_ENV: &str = "AGENT_CONTROL_PLANE_STORE";
const LEGACY_BUN_STORE_ENV: &str = "LOOPER_LEGACY_BUN_DB_PATH";
const AUTOMATION_TICK_SECONDS: u64 = 30;
const TELEGRAM_BRIDGE_TICK_SECONDS: u64 = 5;
const NANOS_PER_MILLISECOND: i64 = 1_000_000;
const SERVER_EXECUTABLE_NAME: &str = "looper-server";
const DEFAULT_SERVER_SCHEME: &str = "http";
const STOP_HOOK_EVENT: &str = "Stop";

pub async fn run_server() -> Result<()> {
    let control_plane = default_control_plane()?;
    import_legacy_bun_mobile_config(&control_plane);
    spawn_automation_runner(control_plane.clone());
    spawn_telegram_bridge(control_plane.clone());

    let listener = TcpListener::bind(default_listen_address()?).await?;
    let local_address = listener.local_addr()?;
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

pub fn run_hook_mode() -> Result<()> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    let payload = parse_hook_payload(&input).unwrap_or(MobileHookPayload {
        hook_event_name: String::new(),
        session_id: None,
        turn_id: None,
        cwd: None,
        last_assistant_message: None,
    });
    let grok_hook = is_grok_hook_invocation();
    let control_plane = default_control_plane()?;
    let service = control_plane.mobile_session_service();
    let outcome = service.hook_outcome_for_payload(&payload)?;
    service.record_hook_lifecycle(&payload, outcome.decision.is_some())?;
    if let Some(thread_id) = payload.session_id.as_deref() {
        control_plane.emit_mobile_event(crate::mobile_events::MobileEventInput {
            kind: crate::mobile_events::MobileEventKind::SessionChanged,
            thread_id: Some(thread_id.to_owned()),
            prompt_id: None,
            detail: Some(payload.hook_event_name.clone()),
        });
        if let Some(prompt_id) = outcome.delivered_prompt_id.as_deref() {
            control_plane.emit_mobile_event(crate::mobile_events::MobileEventInput {
                kind: crate::mobile_events::MobileEventKind::PromptDelivered,
                thread_id: Some(thread_id.to_owned()),
                prompt_id: Some(prompt_id.to_owned()),
                detail: None,
            });
        }
        if let Some(decision) = outcome.decision.as_ref() {
            control_plane.emit_mobile_event(crate::mobile_events::MobileEventInput {
                kind: crate::mobile_events::MobileEventKind::LifecycleChanged,
                thread_id: Some(thread_id.to_owned()),
                prompt_id: None,
                detail: Some(decision.reason.clone()),
            });
        }
    }
    if outcome.decision.is_none() && payload.hook_event_name == STOP_HOOK_EVENT {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            if let Err(error) =
                crate::hook_notifications::send_stop_notifications(&control_plane, &payload).await
            {
                eprintln!("stop notification delivery failed: {error}");
            }
        });
    }
    if let Some(decision) = outcome.decision {
        if grok_hook && decision.decision == "block" {
            if let Some(session_id) = payload.session_id.as_deref() {
                if let Err(error) = spawn_session_continue(&GrokContinueRequest {
                    session_id: session_id.to_owned(),
                    prompt: decision.reason.clone(),
                    cwd: payload.cwd.clone(),
                    grok_executable: None,
                    grok_home: Some(control_plane.grok_home().clone()),
                }) {
                    eprintln!("grok session continue failed: {error}");
                }
            }
        } else {
            println!("{}", serde_json::to_string(&decision)?);
        }
    }
    Ok(())
}

pub fn default_control_plane() -> Result<ControlPlane> {
    let home_path = home_dir();
    Ok(ControlPlane::new(ControlPlaneConfig {
        codex_home: default_codex_home(),
        codex_executable: None,
        grok_home: crate::grok_build::default_grok_home(&home_path),
        store_path: default_store_path(),
        hook_command: Some(default_hook_command()?),
        home_path,
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

        let mut interrupt = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        let mut hangup = signal(SignalKind::hangup()).expect("install SIGHUP handler");

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

fn default_legacy_bun_database_path() -> PathBuf {
    home_dir().join(DEFAULT_LEGACY_BUN_STORE_RELATIVE_PATH)
}

fn home_dir() -> PathBuf {
    std::env::var(HOME_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn import_legacy_bun_mobile_config(control_plane: &ControlPlane) {
    let Some(legacy_path) = legacy_bun_database_path() else {
        return;
    };
    if let Err(error) = control_plane
        .mobile_session_service()
        .import_legacy_bun_mobile_config(&legacy_path)
    {
        eprintln!(
            "legacy Bun mobile config import failed from {}: {error}",
            legacy_path.display()
        );
    }
}

fn legacy_bun_database_path() -> Option<PathBuf> {
    match std::env::var(LEGACY_BUN_STORE_ENV) {
        Ok(value) if value.trim().is_empty() => None,
        Ok(value) => Some(PathBuf::from(value)),
        Err(_) => Some(default_legacy_bun_database_path()),
    }
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

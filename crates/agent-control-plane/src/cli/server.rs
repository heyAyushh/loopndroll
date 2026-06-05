use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;

use super::transport::set_base_url;
use crate::mobile_network::DEFAULT_AGENT_CONTROL_PLANE_PORT;

const SERVER_BINARY_NAME: &str = "looper-server";
const SERVER_COMMAND: &str = "serve";
const HEALTH_PATH: &str = "/health";
const LISTEN_ENV: &str = "AGENT_CONTROL_PLANE_LISTEN";
const LOOPBACK_HOST: &str = "127.0.0.1";
const LOOPER_HEALTH_SERVICE: &str = "looper";
const LOOPER_HOOK_OWNER: &str = "looper-rust";
const FALLBACK_PORT_COUNT: u16 = 16;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(150);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_millis(300);

pub(crate) async fn ensure_server_ready() -> Result<()> {
    if let Some(candidate) = running_looper_candidate().await {
        set_base_url(candidate.base_url);
        return Ok(());
    }

    let candidate = spawn_candidate()?;
    spawn_local_server(&candidate.listen_address)?;
    wait_for_server_health(candidate).await
}

async fn wait_for_server_health(candidate: ServerCandidate) -> Result<()> {
    let started_at = Instant::now();
    while started_at.elapsed() < STARTUP_TIMEOUT {
        if server_health_is_reachable(&candidate).await {
            set_base_url(candidate.base_url);
            return Ok(());
        }
        tokio::time::sleep(STARTUP_POLL_INTERVAL).await;
    }
    bail!(
        "looper-server did not become reachable within {}s",
        STARTUP_TIMEOUT.as_secs()
    )
}

async fn running_looper_candidate() -> Option<ServerCandidate> {
    for candidate in server_candidates() {
        if server_health_is_reachable(&candidate).await {
            return Some(candidate);
        }
    }
    None
}

async fn server_health_is_reachable(candidate: &ServerCandidate) -> bool {
    let Ok(client) = Client::builder().timeout(HEALTH_REQUEST_TIMEOUT).build() else {
        return false;
    };
    let Ok(response) = client
        .get(format!("{}{}", candidate.base_url, HEALTH_PATH))
        .send()
        .await
    else {
        return false;
    };
    if !response.status().is_success() {
        return false;
    }
    response
        .json::<Value>()
        .await
        .is_ok_and(|health| is_looper_health_response(&health))
}

fn spawn_candidate() -> Result<ServerCandidate> {
    server_candidates()
        .into_iter()
        .find(|candidate| listen_address_is_available(&candidate.listen_address))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no Looper control-plane port is available; tried {} fallback ports from {}",
                FALLBACK_PORT_COUNT,
                DEFAULT_AGENT_CONTROL_PLANE_PORT
            )
        })
}

fn spawn_local_server(listen_address: &str) -> Result<()> {
    let executable = local_server_executable()?;
    Command::new(&executable)
        .arg(SERVER_COMMAND)
        .env(LISTEN_ENV, listen_address)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("start {} {}", executable.display(), SERVER_COMMAND))?;
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ServerCandidate {
    listen_address: String,
    base_url: String,
}

fn server_candidates() -> Vec<ServerCandidate> {
    if let Ok(listen_address) = std::env::var(LISTEN_ENV) {
        return vec![server_candidate_for_port(
            listen_address
                .rsplit_once(':')
                .and_then(|(_, port)| port.parse::<u16>().ok())
                .unwrap_or(DEFAULT_AGENT_CONTROL_PLANE_PORT),
        )];
    }

    (0..FALLBACK_PORT_COUNT)
        .map(|offset| server_candidate_for_port(DEFAULT_AGENT_CONTROL_PLANE_PORT + offset))
        .collect()
}

fn server_candidate_for_port(port: u16) -> ServerCandidate {
    ServerCandidate {
        listen_address: format!("{LOOPBACK_HOST}:{port}"),
        base_url: format!("http://{LOOPBACK_HOST}:{port}"),
    }
}

fn listen_address_is_available(listen_address: &str) -> bool {
    TcpListener::bind(listen_address).is_ok()
}

fn is_looper_health_response(health: &Value) -> bool {
    health.get("service").and_then(Value::as_str) == Some(LOOPER_HEALTH_SERVICE)
        || health
            .get("hooks")
            .and_then(|hooks| hooks.get("owner"))
            .and_then(Value::as_str)
            == Some(LOOPER_HOOK_OWNER)
}

fn local_server_executable() -> Result<PathBuf> {
    let current_executable = std::env::current_exe().context("resolve current executable")?;
    Ok(server_executable_for(&current_executable))
}

fn server_executable_for(current_executable: &Path) -> PathBuf {
    current_executable
        .parent()
        .map(|parent| parent.join(SERVER_BINARY_NAME))
        .filter(|candidate| candidate.is_file())
        .unwrap_or_else(|| current_executable.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn server_executable_prefers_sibling_looper_server() {
        let temp_dir =
            std::env::temp_dir().join(format!("looper-server-test-{}", std::process::id()));
        fs::create_dir_all(&temp_dir).unwrap();
        let current = temp_dir.join("looper");
        let sibling = temp_dir.join(SERVER_BINARY_NAME);
        fs::write(&current, b"").unwrap();
        fs::write(&sibling, b"").unwrap();

        assert_eq!(server_executable_for(&current), sibling);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn server_executable_falls_back_to_current_binary() {
        let current = PathBuf::from("/tmp/looper-missing-sibling");

        assert_eq!(server_executable_for(&current), current);
    }

    #[test]
    fn health_contract_rejects_other_local_services() {
        let other_service = serde_json::json!({
            "status": "ok",
            "platform": "hermes-agent",
        });

        assert!(!is_looper_health_response(&other_service));
    }

    #[test]
    fn health_contract_accepts_looper_service_marker() {
        let looper_service = serde_json::json!({
            "service": "looper",
            "ok": true,
        });

        assert!(is_looper_health_response(&looper_service));
    }

    #[test]
    fn default_candidates_cover_fallback_ports() {
        let candidates = server_candidates();

        assert_eq!(candidates.len(), FALLBACK_PORT_COUNT as usize);
        assert_eq!(
            candidates.first().unwrap(),
            &server_candidate_for_port(DEFAULT_AGENT_CONTROL_PLANE_PORT)
        );
        assert_eq!(
            candidates.last().unwrap(),
            &server_candidate_for_port(DEFAULT_AGENT_CONTROL_PLANE_PORT + FALLBACK_PORT_COUNT - 1)
        );
    }
}

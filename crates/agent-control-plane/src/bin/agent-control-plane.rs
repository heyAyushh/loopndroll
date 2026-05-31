use std::net::SocketAddr;
use std::path::PathBuf;

use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig};
use agent_control_plane::http::build_router;
use agent_control_plane::scheduler::AutomationRunner;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|argument| argument == "--hook") {
        return run_hook_mode();
    }

    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_owned());
            PathBuf::from(home).join(".codex")
        });
    let store_path = std::env::var("AGENT_CONTROL_PLANE_STORE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_owned());
            PathBuf::from(home)
                .join("Library/Application Support/looper/agent-control-plane.sqlite")
        });
    let listen = std::env::var("AGENT_CONTROL_PLANE_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8765".to_owned())
        .parse::<SocketAddr>()?;

    let control_plane = ControlPlane::new(ControlPlaneConfig {
        codex_home,
        store_path,
        hook_command: Some(default_hook_command()?),
    });
    let runner_control_plane = control_plane.clone();
    tokio::spawn(async move {
        let mut runner = AutomationRunner::new(runner_control_plane);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            let now_ms = time::OffsetDateTime::now_utc().unix_timestamp_nanos() as i64 / 1_000_000;
            if let Err(error) = runner.tick(now_ms) {
                eprintln!("automation mirror tick failed: {error}");
            }
        }
    });
    let listener = tokio::net::TcpListener::bind(listen).await?;
    axum::serve(listener, build_router(control_plane)).await?;
    Ok(())
}

fn run_hook_mode() -> anyhow::Result<()> {
    let mut input = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
    let _payload: serde_json::Value =
        serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
    Ok(())
}

fn default_hook_command() -> anyhow::Result<String> {
    let executable = std::env::current_exe()?;
    Ok(format!(
        "{} --hook --managed-by looper",
        quote_shell_path(&executable)
    ))
}

fn quote_shell_path(path: &std::path::Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

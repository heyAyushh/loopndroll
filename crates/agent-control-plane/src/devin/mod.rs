use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod bridge;
mod installations;
mod registry;

pub use self::bridge::{
    DevinAcpBridgeAgent, DevinAcpBridgeProbe, DevinAcpBridgeStatus, DevinAcpControlLevel,
    DevinAcpProbeStatus, build_acp_bridge_probe, build_acp_bridge_status,
};
pub use self::installations::DevinInstallationStatus;
pub use self::registry::{DevinAcpAgent, DevinAcpLaunchMetadata, DevinAcpRegistryStatus};

pub(super) const DEVIN_NEXT_CHANNEL: &str = "next";
pub(super) const DEVIN_NEXT_DATA_RELATIVE_PATH: &str = ".devin-next";
pub(super) const DEVIN_PROCESS_NEEDLES: &[&str] = &[
    "/applications/devin.app/",
    "/applications/devin - next.app/",
    "devin helper",
    "devin - next helper",
    "devin-desktop",
];
pub(super) const DEVIN_STABLE_CHANNEL: &str = "stable";

const HOME_ENV: &str = "HOME";
const DEVIN_STABLE_APP_SUPPORT_RELATIVE_PATH: &str = "Library/Application Support/Devin";
const DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH: &str = "Library/Application Support/Devin - Next";
const DEVIN_ACP_REGISTRY_RELATIVE_PATH: &str = "acp/registry.json";
const DEVIN_ARGV_RELATIVE_PATH: &str = "argv.json";
const DEVIN_EXTENSIONS_RELATIVE_PATH: &str = "extensions";
const DEVIN_STABLE_LABEL: &str = "Devin";
const DEVIN_NEXT_LABEL: &str = "Devin Next";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinDesktopStatus {
    pub installations: Vec<DevinInstallationStatus>,
    pub acp_registry: DevinAcpRegistryStatus,
    pub acp_bridge: DevinAcpBridgeStatus,
    pub argv_path: String,
    pub extensions_path: String,
}

pub fn inspect_devin_desktop() -> DevinDesktopStatus {
    inspect_devin_desktop_with_processes(&home_dir(), &current_process_commands())
}

pub fn inspect_devin_desktop_for_home(home: &Path) -> DevinDesktopStatus {
    inspect_devin_desktop_with_processes(home, &current_process_commands())
}

pub fn inspect_devin_desktop_with_processes(
    home: &Path,
    process_commands: &[String],
) -> DevinDesktopStatus {
    let devin_next_data_path = home.join(DEVIN_NEXT_DATA_RELATIVE_PATH);
    let installations = vec![
        installations::inspect_installation(
            home,
            DEVIN_STABLE_CHANNEL,
            DEVIN_STABLE_LABEL,
            DEVIN_STABLE_APP_SUPPORT_RELATIVE_PATH,
            process_commands,
        ),
        installations::inspect_installation(
            home,
            DEVIN_NEXT_CHANNEL,
            DEVIN_NEXT_LABEL,
            DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH,
            process_commands,
        ),
    ];
    let acp_registry = registry::inspect_acp_registry(
        &devin_next_data_path.join(DEVIN_ACP_REGISTRY_RELATIVE_PATH),
    );
    let acp_bridge = build_acp_bridge_status(&installations, &acp_registry);

    DevinDesktopStatus {
        installations,
        acp_registry,
        acp_bridge,
        argv_path: devin_next_data_path
            .join(DEVIN_ARGV_RELATIVE_PATH)
            .display()
            .to_string(),
        extensions_path: devin_next_data_path
            .join(DEVIN_EXTENSIONS_RELATIVE_PATH)
            .display()
            .to_string(),
    }
}

pub fn devin_connection_detail(
    installation: &DevinInstallationStatus,
    status: &DevinDesktopStatus,
) -> String {
    let acp_state = match installation.acp_enabled {
        Some(true) => "ACP enabled",
        Some(false) => "ACP disabled",
        None => "ACP setting unknown",
    };
    let preferred = installation
        .preferred_agent
        .as_deref()
        .unwrap_or("no preferred agent");
    format!(
        "{acp_state}; bridge: {}; preferred: {preferred}; enabled agents: {}; registry agents: {}",
        status.acp_bridge.summary,
        installation.enabled_agents.len(),
        status.acp_registry.agents.len()
    )
}

pub(super) fn read_json_object(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    value.is_object().then_some(value)
}

fn current_process_commands() -> Vec<String> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "command="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn home_dir() -> PathBuf {
    std::env::var(HOME_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devin_status_reads_settings_registry_and_running_state() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let home = temp_dir.path();
        let settings_path = home
            .join(DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH)
            .join(installations::DEVIN_USER_SETTINGS_RELATIVE_PATH);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(
            &settings_path,
            serde_json::json!({
                "devin.acp.enabled": true,
                "devin.acp.preferredAgent": "codex",
                "devin.acp.enabledAgents": {
                    "codex": true,
                    "disabled-agent": false,
                    "grok-build": true
                },
                "devin.acp.agentEnv": {
                    "SECRET_TOKEN": "must-not-be-copied"
                }
            })
            .to_string(),
        )
        .expect("write settings");
        let registry_path = home
            .join(DEVIN_NEXT_DATA_RELATIVE_PATH)
            .join(DEVIN_ACP_REGISTRY_RELATIVE_PATH);
        fs::create_dir_all(registry_path.parent().expect("registry parent"))
            .expect("create registry parent");
        fs::write(
            &registry_path,
            serde_json::json!({
                "version": "1.0.0",
                "agents": [
                    {
                        "id": "codex",
                        "name": "Codex",
                        "version": "0.0.44",
                        "description": "ACP adapter",
                        "distribution": {
                            "npx": {
                                "package": "@agentclientprotocol/codex-acp"
                            }
                        }
                    }
                ]
            })
            .to_string(),
        )
        .expect("write registry");

        let status = inspect_devin_desktop_with_processes(
            home,
            &["/Applications/Devin - Next.app/Contents/MacOS/Devin - Next".to_owned()],
        );
        let next = status
            .installations
            .iter()
            .find(|installation| installation.channel == DEVIN_NEXT_CHANNEL)
            .expect("next installation");

        assert!(next.running);
        assert!(next.installed);
        assert_eq!(next.acp_enabled, Some(true));
        assert_eq!(next.preferred_agent.as_deref(), Some("codex"));
        assert_eq!(next.enabled_agents, vec!["codex", "grok-build"]);
        assert_eq!(status.acp_registry.agents[0].id, "codex");
        assert_eq!(status.acp_registry.agents[0].launch_configured, true);
        assert_eq!(
            status.acp_bridge.control_level,
            DevinAcpControlLevel::ClientCapable
        );
        assert_eq!(status.acp_bridge.agents[0].enabled, true);
        assert_eq!(status.acp_bridge.agents[0].preferred, true);

        let json = serde_json::to_string(&status).expect("serialize status");
        assert!(!json.contains("SECRET_TOKEN"));
        assert!(!json.contains("@agentclientprotocol/codex-acp"));
    }
}

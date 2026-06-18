use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::acp_targets::AcpTarget;

mod acp;
mod bridge;
mod hooks;
mod installations;
mod registry;
mod sessions;

pub use self::acp::{
    DevinAcpDeliveredPrompt, DevinAcpRuntime, DevinAcpRuntimeSession, DevinAcpRuntimeStatus,
    LEGACY_LOOPER_ACP_ROUTE, LOOPER_ACP_AGENT_ID, LOOPER_ACP_AGENT_NAME, LOOPER_ACP_ROUTE,
    acp_session_id_for_public_thread_id, public_thread_id_for_acp_session,
    websocket_url_for_base_url,
};
pub use self::bridge::{
    DevinAcpBridgeAction, DevinAcpBridgeAgent, DevinAcpBridgeProbe, DevinAcpBridgeStatus,
    DevinAcpControlLevel, DevinAcpProbeStatus, build_acp_bridge_probe, build_acp_bridge_status,
};
pub use self::hooks::{
    DevinHookOwner, DevinHookRegistrationChange, DevinHookStatus, inspect_devin_hooks,
    is_devin_hook_invocation, parse_devin_hook_payload, register_owned_devin_hooks,
    unregister_owned_devin_hooks,
};
pub use self::installations::DevinInstallationStatus;
pub use self::registry::{DevinAcpAgent, DevinAcpLaunchMetadata, DevinAcpRegistryStatus};
pub use self::sessions::{
    DevinPromptTransport, DevinSessionDiscovery, DevinSessionDiscoveryError, DevinSessionRecord,
    DevinThreadIdentity, devin_prompt_transport_for_provider, devin_session_capabilities,
    devin_session_to_desktop_thread, devin_session_to_thread_record,
    devin_thread_identity_from_public_thread_id, discover_devin_sessions,
    discover_devin_sessions_with_previews, discover_devin_sessions_without_previews,
    discover_recent_devin_sessions, discover_recent_devin_sessions_with_previews,
};

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
pub(super) const DEVIN_STABLE_APP_SUPPORT_RELATIVE_PATH: &str = "Library/Application Support/Devin";
pub(super) const DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH: &str =
    "Library/Application Support/Devin - Next";
const DEVIN_ACP_REGISTRY_RELATIVE_PATH: &str = "acp/registry.json";
const DEVIN_ARGV_RELATIVE_PATH: &str = "argv.json";
const DEVIN_EXTENSIONS_RELATIVE_PATH: &str = "extensions";
const DEVIN_STABLE_LABEL: &str = "Devin";
const DEVIN_NEXT_LABEL: &str = "Devin Next";
const DEVIN_ACP_REGISTRY_VERSION: &str = "1.0.0";
const DEVIN_SETTINGS_ACP_ENABLED_KEY: &str = "devin.acp.enabled";
const DEVIN_SETTINGS_ENABLED_AGENTS_KEY: &str = "devin.acp.enabledAgents";
const DEVIN_SETTINGS_PREFERRED_AGENT_KEY: &str = "devin.acp.preferredAgent";
const LOOPER_ACP_AGENT_DESCRIPTION: &str =
    "Local Looper bridge for Devin Desktop sessions, Handoff, and mobile prompt delivery.";
const DEVIN_ACP_TARGET_CLIENT_ID: &str = "devin";
const DEVIN_ACP_TARGET_CLIENT_NAME: &str = "Devin Desktop";
const DEVIN_ACP_TARGET_READY_DETAIL: &str =
    "Devin Desktop ACP agent is enabled with sanitized launch metadata.";
const DEVIN_ACP_TARGET_BLOCKED_DETAIL: &str =
    "Devin Desktop ACP agent is visible but not launch-ready.";
const ACP_TARGET_READY_STATUS: &str = "ready";
const ACP_TARGET_BLOCKED_STATUS: &str = "blocked";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinDesktopStatus {
    pub installations: Vec<DevinInstallationStatus>,
    pub acp_registry: DevinAcpRegistryStatus,
    pub acp_bridge: DevinAcpBridgeStatus,
    pub argv_path: String,
    pub extensions_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpInstallResult {
    pub installed_agent_id: String,
    pub registry_path: String,
    pub settings_path: String,
    pub websocket_url: String,
    pub acp_enabled: bool,
    pub preferred_agent: String,
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

pub fn install_looper_acp_agent_for_home(
    home: &Path,
    server_base_url: &str,
) -> Result<DevinAcpInstallResult> {
    let websocket_url = websocket_url_for_base_url(server_base_url);
    let registry_path = home
        .join(DEVIN_NEXT_DATA_RELATIVE_PATH)
        .join(DEVIN_ACP_REGISTRY_RELATIVE_PATH);
    let settings_path = home
        .join(DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH)
        .join(installations::DEVIN_USER_SETTINGS_RELATIVE_PATH);

    upsert_looper_registry_agent(&registry_path, &websocket_url)?;
    enable_looper_acp_agent(&settings_path)?;

    Ok(DevinAcpInstallResult {
        installed_agent_id: LOOPER_ACP_AGENT_ID.to_owned(),
        registry_path: registry_path.display().to_string(),
        settings_path: settings_path.display().to_string(),
        websocket_url,
        acp_enabled: true,
        preferred_agent: LOOPER_ACP_AGENT_ID.to_owned(),
    })
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

pub fn devin_acp_targets(status: &DevinDesktopStatus) -> Vec<AcpTarget> {
    status
        .acp_registry
        .agents
        .iter()
        .map(|agent| {
            let bridge_agent = status
                .acp_bridge
                .agents
                .iter()
                .find(|bridge_agent| bridge_agent.id == agent.id);
            let ready = bridge_agent
                .map(|bridge_agent| {
                    bridge_agent.control_level == DevinAcpControlLevel::AgentConfigured
                })
                .unwrap_or(false);
            AcpTarget {
                id: format!("{DEVIN_ACP_TARGET_CLIENT_ID}:{}", agent.id),
                client: DEVIN_ACP_TARGET_CLIENT_ID.to_owned(),
                client_name: DEVIN_ACP_TARGET_CLIENT_NAME.to_owned(),
                agent_id: agent.id.clone(),
                name: agent.name.clone(),
                source: "devin-acp-registry".to_owned(),
                source_path: Some(status.acp_registry.path.clone()),
                enabled: bridge_agent
                    .map(|bridge_agent| bridge_agent.enabled)
                    .unwrap_or(false),
                preferred: bridge_agent
                    .map(|bridge_agent| bridge_agent.preferred)
                    .unwrap_or(false),
                launch_configured: agent.launch_configured,
                launch: agent.launch.clone(),
                ready,
                status: if ready {
                    ACP_TARGET_READY_STATUS.to_owned()
                } else {
                    ACP_TARGET_BLOCKED_STATUS.to_owned()
                },
                detail: if ready {
                    DEVIN_ACP_TARGET_READY_DETAIL.to_owned()
                } else {
                    DEVIN_ACP_TARGET_BLOCKED_DETAIL.to_owned()
                },
            }
        })
        .collect()
}

pub(super) fn read_json_object(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    value.is_object().then_some(value)
}

fn upsert_looper_registry_agent(path: &Path, websocket_url: &str) -> Result<()> {
    let mut registry = read_json_object(path)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_else(|| {
            let mut registry = Map::new();
            registry.insert(
                "version".to_owned(),
                Value::String(DEVIN_ACP_REGISTRY_VERSION.to_owned()),
            );
            registry.insert("agents".to_owned(), Value::Array(Vec::new()));
            registry
        });
    registry
        .entry("version".to_owned())
        .or_insert_with(|| Value::String(DEVIN_ACP_REGISTRY_VERSION.to_owned()));

    let agent = looper_acp_registry_agent(websocket_url);
    let agents_value = registry
        .entry("agents".to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    let agents = match agents_value {
        Value::Array(agents) => agents,
        _ => {
            *agents_value = Value::Array(Vec::new());
            agents_value.as_array_mut().expect("agents array")
        }
    };
    if let Some(existing) = agents
        .iter_mut()
        .find(|agent| agent.get("id").and_then(Value::as_str) == Some(LOOPER_ACP_AGENT_ID))
    {
        *existing = agent;
    } else {
        agents.push(agent);
    }
    agents.sort_by(|left, right| {
        left.get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .cmp(right.get("id").and_then(Value::as_str).unwrap_or_default())
    });

    write_pretty_json(path, &Value::Object(registry))
}

fn enable_looper_acp_agent(path: &Path) -> Result<()> {
    let mut settings = read_json_object(path)
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    settings.insert(DEVIN_SETTINGS_ACP_ENABLED_KEY.to_owned(), Value::Bool(true));
    settings.insert(
        DEVIN_SETTINGS_PREFERRED_AGENT_KEY.to_owned(),
        Value::String(LOOPER_ACP_AGENT_ID.to_owned()),
    );

    let enabled_agents_value = settings
        .entry(DEVIN_SETTINGS_ENABLED_AGENTS_KEY.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let enabled_agents = match enabled_agents_value {
        Value::Object(enabled_agents) => enabled_agents,
        _ => {
            *enabled_agents_value = Value::Object(Map::new());
            enabled_agents_value
                .as_object_mut()
                .expect("enabled agents object")
        }
    };
    enabled_agents.insert(LOOPER_ACP_AGENT_ID.to_owned(), Value::Bool(true));

    write_pretty_json(path, &Value::Object(settings))
}

fn looper_acp_registry_agent(websocket_url: &str) -> Value {
    json!({
        "id": LOOPER_ACP_AGENT_ID,
        "name": LOOPER_ACP_AGENT_NAME,
        "version": env!("CARGO_PKG_VERSION"),
        "description": LOOPER_ACP_AGENT_DESCRIPTION,
        "distribution": {
            "websocket": {
                "url": websocket_url
            }
        }
    })
}

fn write_pretty_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut content = serde_json::to_string_pretty(value)?;
    content.push('\n');
    fs::write(path, content)?;
    Ok(())
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
            DevinAcpControlLevel::AgentConfigured
        );
        assert_eq!(status.acp_bridge.agents[0].enabled, true);
        assert_eq!(status.acp_bridge.agents[0].preferred, true);

        let json = serde_json::to_string(&status).expect("serialize status");
        assert!(!json.contains("SECRET_TOKEN"));
        assert!(!json.contains("@agentclientprotocol/codex-acp"));
    }

    #[test]
    fn install_looper_acp_agent_preserves_registry_and_enables_agent() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let home = temp_dir.path();
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
        let settings_path = home
            .join(DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH)
            .join(installations::DEVIN_USER_SETTINGS_RELATIVE_PATH);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(
            &settings_path,
            serde_json::json!({
                "devin.acp.enabled": false,
                "devin.acp.preferredAgent": "codex",
                "devin.acp.enabledAgents": { "codex": true }
            })
            .to_string(),
        )
        .expect("write settings");

        let result = install_looper_acp_agent_for_home(home, "http://127.0.0.1:8765")
            .expect("install looper acp");

        assert_eq!(result.installed_agent_id, LOOPER_ACP_AGENT_ID);
        assert_eq!(
            result.websocket_url,
            "ws://127.0.0.1:8765/acp/client-hosts/devin"
        );
        let registry = read_json_object(&registry_path).expect("registry");
        let agents = registry["agents"].as_array().expect("agents");
        assert_eq!(agents.len(), 2);
        let looper = agents
            .iter()
            .find(|agent| agent["id"] == LOOPER_ACP_AGENT_ID)
            .expect("looper agent");
        assert_eq!(
            looper["distribution"]["websocket"]["url"],
            "ws://127.0.0.1:8765/acp/client-hosts/devin"
        );
        assert!(
            !serde_json::to_string(looper)
                .expect("looper json")
                .contains("@agentclientprotocol")
        );

        let settings = read_json_object(&settings_path).expect("settings");
        assert_eq!(settings["devin.acp.enabled"], true);
        assert_eq!(settings["devin.acp.preferredAgent"], LOOPER_ACP_AGENT_ID);
        assert_eq!(
            settings["devin.acp.enabledAgents"][LOOPER_ACP_AGENT_ID],
            true
        );
        assert_eq!(settings["devin.acp.enabledAgents"]["codex"], true);

        let status = inspect_devin_desktop_with_processes(home, &[]);
        let looper_status = status
            .acp_registry
            .agents
            .iter()
            .find(|agent| agent.id == LOOPER_ACP_AGENT_ID)
            .expect("sanitized looper agent");
        assert!(looper_status.launch_configured);
        assert_eq!(looper_status.launch.methods, vec!["websocket"]);
    }
}

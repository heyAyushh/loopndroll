use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DEVIN_NEXT_CHANNEL, DEVIN_PROCESS_NEEDLES, DEVIN_STABLE_CHANNEL, read_json_object};

pub(super) const DEVIN_USER_SETTINGS_RELATIVE_PATH: &str = "User/settings.json";

const DEVIN_SETTINGS_ACP_ENABLED_KEY: &str = "devin.acp.enabled";
const DEVIN_SETTINGS_ENABLED_AGENTS_KEY: &str = "devin.acp.enabledAgents";
const DEVIN_SETTINGS_PREFERRED_AGENT_KEY: &str = "devin.acp.preferredAgent";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinInstallationStatus {
    pub id: String,
    pub label: String,
    pub channel: String,
    pub running: bool,
    pub installed: bool,
    pub app_support_path: String,
    pub settings_path: String,
    pub settings_exists: bool,
    pub acp_enabled: Option<bool>,
    pub preferred_agent: Option<String>,
    pub enabled_agents: Vec<String>,
}

pub(super) fn inspect_installation(
    home: &Path,
    channel: &str,
    label: &str,
    app_support_relative_path: &str,
    process_commands: &[String],
) -> DevinInstallationStatus {
    let app_support_path = home.join(app_support_relative_path);
    let settings_path = app_support_path.join(DEVIN_USER_SETTINGS_RELATIVE_PATH);
    let settings = read_json_object(&settings_path);
    DevinInstallationStatus {
        id: format!("devin-desktop-{channel}"),
        label: label.to_owned(),
        channel: channel.to_owned(),
        running: installation_is_running(channel, process_commands),
        installed: app_support_path.exists(),
        app_support_path: app_support_path.display().to_string(),
        settings_path: settings_path.display().to_string(),
        settings_exists: settings.is_some(),
        acp_enabled: settings
            .as_ref()
            .and_then(|settings| settings.get(DEVIN_SETTINGS_ACP_ENABLED_KEY))
            .and_then(Value::as_bool),
        preferred_agent: settings
            .as_ref()
            .and_then(|settings| settings.get(DEVIN_SETTINGS_PREFERRED_AGENT_KEY))
            .and_then(Value::as_str)
            .map(str::to_owned),
        enabled_agents: settings
            .as_ref()
            .and_then(|settings| settings.get(DEVIN_SETTINGS_ENABLED_AGENTS_KEY))
            .and_then(Value::as_object)
            .map(enabled_agent_ids)
            .unwrap_or_default(),
    }
}

fn enabled_agent_ids(agents: &serde_json::Map<String, Value>) -> Vec<String> {
    let mut ids = agents
        .iter()
        .filter(|(_, enabled)| enabled.as_bool().unwrap_or(false))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn installation_is_running(channel: &str, process_commands: &[String]) -> bool {
    process_commands.iter().any(|command| {
        let normalized = command.to_ascii_lowercase();
        contains_devin_process(&normalized) && matches_devin_channel(channel, &normalized)
    })
}

fn contains_devin_process(command: &str) -> bool {
    DEVIN_PROCESS_NEEDLES
        .iter()
        .any(|needle| command.contains(needle))
}

fn matches_devin_channel(channel: &str, command: &str) -> bool {
    match channel {
        DEVIN_NEXT_CHANNEL => {
            command.contains("devin - next")
                || command.contains(".devin-next")
                || command.contains("devin-desktop-next")
        }
        DEVIN_STABLE_CHANNEL => {
            command.contains("/applications/devin.app/") && !command.contains("devin - next")
        }
        _ => false,
    }
}

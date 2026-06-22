use serde::{Deserialize, Serialize};

use crate::acp::targets::AcpLaunchMetadata;

mod install;
mod settings;
mod targets;

pub use install::install_looper_zed_acp_agent_for_home;
pub use settings::{inspect_zed_for_home, inspect_zed_for_home_with_processes};
pub use targets::zed_acp_targets;

pub const ZED_CLIENT_ID: &str = "zed";
pub const ZED_CLIENT_NAME: &str = "Zed";
pub const ZED_DEFAULT_CONTROLLED_AGENT_ID: &str = "codex-acp";

pub(super) const HOME_ZED_SETTINGS_RELATIVE_PATH: &str = ".zed/settings.json";
pub(super) const XDG_ZED_SETTINGS_RELATIVE_PATH: &str = ".config/zed/settings.json";
pub(super) const ZED_AGENT_SERVERS_KEY: &str = "agent_servers";
pub(super) const ZED_ACP_TARGET_SOURCE: &str = "zed-agent-servers";
pub(super) const ZED_READY_STATUS: &str = "ready";
pub(super) const ZED_READ_ONLY_STATUS: &str = "read-only";
pub(super) const ZED_BLOCKED_STATUS: &str = "blocked";
pub(super) const ZED_READY_DETAIL: &str =
    "Zed target launch path is wrapped by Looper host control.";
pub(super) const ZED_READ_ONLY_DETAIL: &str = "Zed External Agent target is configured; Looper reports it read-only and does not execute its command.";
pub(super) const ZED_NOT_RUNNING_DETAIL: &str =
    "Zed target is configured through Looper, but no running Zed host is visible.";
pub(super) const ZED_MISSING_LAUNCH_DETAIL: &str =
    "Zed External Agent target lacks command or transport metadata.";
pub(super) const ZED_LOOPER_MISSING_LAUNCH_DETAIL: &str =
    "Zed target is marked for Looper host control, but its launch command is missing.";
pub(super) const ZED_LOOPER_ACP_COMMAND_ARGS: &[&str] = &["acp", "stdio", ZED_CLIENT_ID];
pub(super) const ZED_LOOPER_ACP_COMMAND_FALLBACK: &str = "looper";
pub(super) const ZED_DEFAULT_CONTROLLED_AGENT_NAME: &str = "Codex CLI";
pub(super) const ZED_PROXY_ARG_SEPARATOR: &str = "--";
pub(super) const ZED_EXTERNAL_AGENTS_REGISTRY_RELATIVE_PATH: &str =
    "Library/Application Support/Zed/external_agents/registry";
pub(super) const ZED_PROCESS_NEEDLES: &[&str] = &[
    "/applications/zed.app/",
    "zed.app/contents/macos/zed",
    "com.zed.dev",
    "zed --foreground",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedStatus {
    pub settings_path: String,
    pub settings_exists: bool,
    pub settings_error: Option<String>,
    pub running: bool,
    pub installed: bool,
    pub summary: String,
    pub acp_target_count: usize,
    pub acp_targets: Vec<ZedAcpTarget>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedAcpTarget {
    pub id: String,
    pub name: String,
    pub target_type: Option<String>,
    pub launch_configured: bool,
    pub looper_managed: bool,
    pub launch: AcpLaunchMetadata,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedAcpInstallResult {
    pub installed_agent_id: String,
    pub settings_path: String,
    pub command: String,
    pub args: Vec<String>,
    pub wrapped_command: String,
    pub command_line: String,
    pub preferred_agent: String,
}

#[cfg(test)]
mod tests;

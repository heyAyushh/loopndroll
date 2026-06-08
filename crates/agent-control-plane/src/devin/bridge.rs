use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::installations::DevinInstallationStatus;
use super::registry::{DevinAcpAgent, DevinAcpRegistryStatus};

const ACP_BRIDGE_SOURCE: &str = "devin-acp-registry";
const ACP_BRIDGE_PROBE_ACTION_ID: &str = "probe";
const ACP_BRIDGE_PROBE_ACTION_LABEL: &str = "Probe Devin agent";
const ACP_BRIDGE_PROBE_METHOD: &str = "POST";
const ACP_BRIDGE_PROBE_PATH: &str = "/desktop/devin/acp-bridge/probe";
const ACP_BRIDGE_INSTALL_ACTION_ID: &str = "install";
const ACP_BRIDGE_INSTALL_ACTION_LABEL: &str = "Install Looper ACP agent";
const ACP_BRIDGE_INSTALL_METHOD: &str = "POST";
const ACP_BRIDGE_INSTALL_PATH: &str = "/desktop/devin/acp-bridge/install";
const ACP_METHODS: &[&str] = &[
    "initialize",
    "session/new",
    "session/load",
    "session/list",
    "session/resume",
    "session/prompt",
    "session/cancel",
    "session/close",
    "session/setMode",
    "ext/*",
];
const AUTO_EXECUTION_LIMITATION: &str =
    "Looper reads Devin Desktop state and never auto-executes registry commands.";
const DEVIN_TRANSPORT_LIMITATION: &str = "Existing Devin Desktop-owned sessions are read-only for prompt delivery until Looper owns ACP authentication or a Devin-side bridge is installed.";
const MISSING_LAUNCH_METADATA_LIMITATION: &str =
    "No configured launchable enabled ACP agent was found.";
const LAUNCH_PREFLIGHT_PROBE_KIND: &str = "launch-preflight";
const PROBE_READY_DETAIL: &str =
    "Devin agent configuration is visible; no registry command was executed.";
const PROBE_BLOCKED_DETAIL: &str =
    "Devin agent configuration is incomplete; inspect blockers before relying on it.";
const ACP_DISABLED_BLOCKER: &str = "Devin ACP is disabled or missing in Desktop settings.";
const ACP_REGISTRY_MISSING_BLOCKER: &str = "Devin ACP registry is missing.";
const ACP_AGENT_MISSING_BLOCKER: &str = "Requested ACP agent was not found in the registry.";
const ACP_AGENT_DISABLED_BLOCKER: &str = "ACP agent is not enabled or preferred in Devin settings.";
const ACP_AGENT_LAUNCH_MISSING_BLOCKER: &str = "ACP agent has no sanitized launch metadata.";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DevinAcpControlLevel {
    VisibilityOnly,
    AgentConfigured,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeStatus {
    pub available: bool,
    pub control_level: DevinAcpControlLevel,
    pub summary: String,
    pub supported_methods: Vec<String>,
    pub limitations: Vec<String>,
    pub actions: Vec<DevinAcpBridgeAction>,
    pub agents: Vec<DevinAcpBridgeAgent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeAction {
    pub id: String,
    pub label: String,
    pub method: String,
    pub path: String,
    pub default_agent_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeAgent {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub enabled: bool,
    pub preferred: bool,
    pub launch_configured: bool,
    pub control_level: DevinAcpControlLevel,
    pub supports_sessions: bool,
    pub supports_prompt: bool,
    pub supports_cancel: bool,
    pub source: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DevinAcpProbeStatus {
    Ready,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeProbe {
    pub ok: bool,
    pub status: DevinAcpProbeStatus,
    pub agent_id: Option<String>,
    pub name: Option<String>,
    pub control_level: DevinAcpControlLevel,
    pub ready: bool,
    pub probe_kind: String,
    pub launch_configured: bool,
    pub launch_methods: Vec<String>,
    pub supported_methods: Vec<String>,
    pub blockers: Vec<String>,
    pub detail: String,
}

pub fn build_acp_bridge_status(
    installations: &[DevinInstallationStatus],
    registry: &DevinAcpRegistryStatus,
) -> DevinAcpBridgeStatus {
    let settings = BridgeSettings::from_installations(installations);
    let agents = registry
        .agents
        .iter()
        .map(|agent| bridge_agent(agent, registry.exists, &settings))
        .collect::<Vec<_>>();
    let available = settings.acp_enabled && registry.exists && !registry.agents.is_empty();
    let control_level = bridge_control_level(&agents);

    DevinAcpBridgeStatus {
        available,
        control_level,
        summary: bridge_summary(settings.acp_enabled, available, registry, control_level),
        supported_methods: static_strings(ACP_METHODS),
        limitations: bridge_limitations(control_level),
        actions: bridge_actions(&agents),
        agents,
    }
}

pub fn build_acp_bridge_probe(
    installations: &[DevinInstallationStatus],
    registry: &DevinAcpRegistryStatus,
    requested_agent_id: Option<&str>,
) -> DevinAcpBridgeProbe {
    let settings = BridgeSettings::from_installations(installations);
    let bridge = build_acp_bridge_status(installations, registry);
    let Some(agent) = select_probe_agent(registry, &settings, requested_agent_id) else {
        return missing_agent_probe(requested_agent_id, bridge.control_level, registry.exists);
    };
    let blockers = probe_blockers(registry.exists, &settings, agent);
    let control_level = if blockers.is_empty() {
        DevinAcpControlLevel::AgentConfigured
    } else {
        DevinAcpControlLevel::VisibilityOnly
    };
    ready_or_blocked_probe(agent, control_level, blockers)
}

fn bridge_agent(
    agent: &DevinAcpAgent,
    registry_exists: bool,
    settings: &BridgeSettings,
) -> DevinAcpBridgeAgent {
    let enabled = settings.agent_is_enabled(&agent.id);
    let control_level =
        if settings.acp_enabled && registry_exists && enabled && agent.launch_configured {
            DevinAcpControlLevel::AgentConfigured
        } else {
            DevinAcpControlLevel::VisibilityOnly
        };

    DevinAcpBridgeAgent {
        id: agent.id.clone(),
        name: agent.name.clone(),
        version: agent.version.clone(),
        description: agent.description.clone(),
        enabled,
        preferred: settings.preferred_agent.as_deref() == Some(agent.id.as_str()),
        launch_configured: agent.launch_configured,
        control_level,
        supports_sessions: control_level == DevinAcpControlLevel::AgentConfigured,
        supports_prompt: control_level == DevinAcpControlLevel::AgentConfigured,
        supports_cancel: false,
        source: ACP_BRIDGE_SOURCE.to_owned(),
    }
}

fn bridge_control_level(agents: &[DevinAcpBridgeAgent]) -> DevinAcpControlLevel {
    if agents
        .iter()
        .any(|agent| agent.control_level == DevinAcpControlLevel::AgentConfigured)
    {
        DevinAcpControlLevel::AgentConfigured
    } else {
        DevinAcpControlLevel::VisibilityOnly
    }
}

fn bridge_actions(agents: &[DevinAcpBridgeAgent]) -> Vec<DevinAcpBridgeAction> {
    let mut actions = vec![DevinAcpBridgeAction {
        id: ACP_BRIDGE_INSTALL_ACTION_ID.to_owned(),
        label: ACP_BRIDGE_INSTALL_ACTION_LABEL.to_owned(),
        method: ACP_BRIDGE_INSTALL_METHOD.to_owned(),
        path: ACP_BRIDGE_INSTALL_PATH.to_owned(),
        default_agent_id: None,
    }];
    actions.extend(
        default_probe_agent_id(agents)
            .map(|default_agent_id| DevinAcpBridgeAction {
                id: ACP_BRIDGE_PROBE_ACTION_ID.to_owned(),
                label: ACP_BRIDGE_PROBE_ACTION_LABEL.to_owned(),
                method: ACP_BRIDGE_PROBE_METHOD.to_owned(),
                path: ACP_BRIDGE_PROBE_PATH.to_owned(),
                default_agent_id: Some(default_agent_id),
            })
            .into_iter(),
    );
    actions
}

fn default_probe_agent_id(agents: &[DevinAcpBridgeAgent]) -> Option<String> {
    agents
        .iter()
        .find(|agent| {
            agent.preferred && agent.control_level == DevinAcpControlLevel::AgentConfigured
        })
        .or_else(|| {
            agents
                .iter()
                .find(|agent| agent.control_level == DevinAcpControlLevel::AgentConfigured)
        })
        .map(|agent| agent.id.clone())
}

fn select_probe_agent<'a>(
    registry: &'a DevinAcpRegistryStatus,
    settings: &BridgeSettings,
    requested_agent_id: Option<&str>,
) -> Option<&'a DevinAcpAgent> {
    if let Some(agent_id) = requested_agent_id {
        return registry.agents.iter().find(|agent| agent.id == agent_id);
    }
    settings
        .preferred_agent
        .as_deref()
        .and_then(|agent_id| registry.agents.iter().find(|agent| agent.id == agent_id))
        .or_else(|| {
            registry
                .agents
                .iter()
                .find(|agent| settings.agent_is_enabled(&agent.id) && agent.launch_configured)
        })
        .or_else(|| registry.agents.first())
}

fn missing_agent_probe(
    requested_agent_id: Option<&str>,
    control_level: DevinAcpControlLevel,
    registry_exists: bool,
) -> DevinAcpBridgeProbe {
    let mut blockers = Vec::new();
    if !registry_exists {
        blockers.push(ACP_REGISTRY_MISSING_BLOCKER.to_owned());
    }
    blockers.push(ACP_AGENT_MISSING_BLOCKER.to_owned());
    DevinAcpBridgeProbe {
        ok: false,
        status: DevinAcpProbeStatus::Blocked,
        agent_id: requested_agent_id.map(str::to_owned),
        name: None,
        control_level,
        ready: false,
        probe_kind: LAUNCH_PREFLIGHT_PROBE_KIND.to_owned(),
        launch_configured: false,
        launch_methods: Vec::new(),
        supported_methods: static_strings(ACP_METHODS),
        blockers,
        detail: PROBE_BLOCKED_DETAIL.to_owned(),
    }
}

fn ready_or_blocked_probe(
    agent: &DevinAcpAgent,
    control_level: DevinAcpControlLevel,
    blockers: Vec<String>,
) -> DevinAcpBridgeProbe {
    let ok = blockers.is_empty();
    DevinAcpBridgeProbe {
        ok,
        status: if ok {
            DevinAcpProbeStatus::Ready
        } else {
            DevinAcpProbeStatus::Blocked
        },
        agent_id: Some(agent.id.clone()),
        name: Some(agent.name.clone()),
        control_level,
        ready: ok,
        probe_kind: LAUNCH_PREFLIGHT_PROBE_KIND.to_owned(),
        launch_configured: agent.launch_configured,
        launch_methods: agent.launch.methods.clone(),
        supported_methods: static_strings(ACP_METHODS),
        blockers,
        detail: if ok {
            PROBE_READY_DETAIL.to_owned()
        } else {
            PROBE_BLOCKED_DETAIL.to_owned()
        },
    }
}

fn probe_blockers(
    registry_exists: bool,
    settings: &BridgeSettings,
    agent: &DevinAcpAgent,
) -> Vec<String> {
    let mut blockers = Vec::new();
    if !settings.acp_enabled {
        blockers.push(ACP_DISABLED_BLOCKER.to_owned());
    }
    if !registry_exists {
        blockers.push(ACP_REGISTRY_MISSING_BLOCKER.to_owned());
    }
    if !settings.agent_is_enabled(&agent.id) {
        blockers.push(ACP_AGENT_DISABLED_BLOCKER.to_owned());
    }
    if !agent.launch_configured {
        blockers.push(ACP_AGENT_LAUNCH_MISSING_BLOCKER.to_owned());
    }
    blockers
}

fn bridge_summary(
    acp_enabled: bool,
    available: bool,
    registry: &DevinAcpRegistryStatus,
    control_level: DevinAcpControlLevel,
) -> String {
    if !acp_enabled {
        return "Devin ACP disabled or not configured in Desktop settings".to_owned();
    }
    if !available && !registry.exists {
        return "Devin ACP settings found no registry".to_owned();
    }
    if registry.exists && registry.agents.is_empty() {
        return "Devin ACP registry has no agents".to_owned();
    }
    match control_level {
        DevinAcpControlLevel::AgentConfigured => {
            "Devin Desktop agents are visible from the local ACP registry".to_owned()
        }
        DevinAcpControlLevel::VisibilityOnly => {
            "Devin Desktop agent registry is visibility-only until a launchable enabled agent is configured"
                .to_owned()
        }
    }
}

fn bridge_limitations(control_level: DevinAcpControlLevel) -> Vec<String> {
    let mut limitations = Vec::with_capacity(3);
    limitations.push(AUTO_EXECUTION_LIMITATION.to_owned());
    limitations.push(DEVIN_TRANSPORT_LIMITATION.to_owned());
    match control_level {
        DevinAcpControlLevel::AgentConfigured => {}
        DevinAcpControlLevel::VisibilityOnly => {
            limitations.push(MISSING_LAUNCH_METADATA_LIMITATION.to_owned());
        }
    }
    limitations
}

fn static_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

struct BridgeSettings {
    acp_enabled: bool,
    enabled_agents: BTreeSet<String>,
    preferred_agent: Option<String>,
}

impl BridgeSettings {
    fn from_installations(installations: &[DevinInstallationStatus]) -> Self {
        let enabled_agents = installations
            .iter()
            .flat_map(|installation| installation.enabled_agents.iter().cloned())
            .collect::<BTreeSet<_>>();
        let preferred_agent = installations
            .iter()
            .find_map(|installation| installation.preferred_agent.clone());
        Self {
            acp_enabled: installations
                .iter()
                .any(|installation| installation.acp_enabled == Some(true)),
            enabled_agents,
            preferred_agent,
        }
    }

    fn agent_is_enabled(&self, agent_id: &str) -> bool {
        self.enabled_agents.contains(agent_id) || self.preferred_agent.as_deref() == Some(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use super::super::registry::DevinAcpLaunchMetadata;
    use super::*;

    fn installation(
        enabled_agents: &[&str],
        preferred_agent: Option<&str>,
    ) -> DevinInstallationStatus {
        DevinInstallationStatus {
            id: "devin-desktop-next".to_owned(),
            label: "Devin Next".to_owned(),
            channel: "next".to_owned(),
            running: true,
            installed: true,
            app_support_path: "/tmp/devin".to_owned(),
            settings_path: "/tmp/devin/User/settings.json".to_owned(),
            settings_exists: true,
            acp_enabled: Some(true),
            preferred_agent: preferred_agent.map(str::to_owned),
            enabled_agents: enabled_agents
                .iter()
                .map(|agent| (*agent).to_owned())
                .collect(),
        }
    }

    fn registry(launch_configured: bool) -> DevinAcpRegistryStatus {
        DevinAcpRegistryStatus {
            path: "/tmp/.devin-next/acp/registry.json".to_owned(),
            exists: true,
            version: Some("1.0.0".to_owned()),
            agents: vec![DevinAcpAgent {
                id: "codex".to_owned(),
                name: "Codex".to_owned(),
                version: None,
                description: None,
                launch_configured,
                launch: DevinAcpLaunchMetadata {
                    configured: launch_configured,
                    methods: if launch_configured {
                        vec!["npx".to_owned()]
                    } else {
                        Vec::new()
                    },
                },
            }],
        }
    }

    #[test]
    fn launchable_enabled_agents_are_probeable_metadata() {
        let status =
            build_acp_bridge_status(&[installation(&["codex"], Some("codex"))], &registry(true));

        assert!(status.available);
        assert_eq!(status.control_level, DevinAcpControlLevel::AgentConfigured);
        assert_eq!(
            status.agents[0].control_level,
            DevinAcpControlLevel::AgentConfigured
        );
        assert!(status.agents[0].supports_prompt);
        assert!(status.agents[0].supports_sessions);
        assert!(!status.agents[0].supports_cancel);
        assert!(
            status
                .actions
                .iter()
                .any(|action| action.id == ACP_BRIDGE_INSTALL_ACTION_ID)
        );
        let probe = status
            .actions
            .iter()
            .find(|action| action.id == ACP_BRIDGE_PROBE_ACTION_ID)
            .expect("probe action");
        assert_eq!(probe.default_agent_id.as_deref(), Some("codex"));
        assert!(
            status
                .limitations
                .iter()
                .any(|limitation| limitation.contains("never auto-executes"))
        );
    }

    #[test]
    fn missing_launch_metadata_is_visibility_only() {
        let status =
            build_acp_bridge_status(&[installation(&["codex"], Some("codex"))], &registry(false));

        assert_eq!(status.control_level, DevinAcpControlLevel::VisibilityOnly);
        assert!(!status.agents[0].supports_sessions);
        assert!(
            status
                .limitations
                .iter()
                .any(|limitation| limitation.contains("No configured launchable"))
        );
    }

    #[test]
    fn disabled_acp_is_visible_without_becoming_available() {
        let mut disabled = installation(&["codex"], Some("codex"));
        disabled.acp_enabled = Some(false);
        let status = build_acp_bridge_status(&[disabled], &registry(true));

        assert!(!status.available);
        assert_eq!(
            status.summary,
            "Devin ACP disabled or not configured in Desktop settings"
        );
        assert_eq!(
            status.agents[0].control_level,
            DevinAcpControlLevel::VisibilityOnly
        );
    }

    #[test]
    fn probe_selects_preferred_ready_agent_without_launching() {
        let probe = build_acp_bridge_probe(
            &[installation(&["codex"], Some("codex"))],
            &registry(true),
            None,
        );

        assert!(probe.ok);
        assert_eq!(probe.status, DevinAcpProbeStatus::Ready);
        assert_eq!(probe.agent_id.as_deref(), Some("codex"));
        assert!(probe.ready);
        assert_eq!(probe.launch_methods, vec!["npx"]);
        assert!(probe.detail.contains("no registry command was executed"));
    }

    #[test]
    fn probe_reports_blockers_for_disabled_agent() {
        let probe =
            build_acp_bridge_probe(&[installation(&[], None)], &registry(true), Some("codex"));

        assert!(!probe.ok);
        assert_eq!(probe.status, DevinAcpProbeStatus::Blocked);
        assert!(
            probe
                .blockers
                .iter()
                .any(|blocker| blocker.contains("not enabled"))
        );
    }
}

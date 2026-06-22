//! Devin Desktop ACP client-host mapping.
//!
//! Devin is the only ACP host here where Looper owns an install/probe bridge
//! and can observe live runtime sessions directly.

use crate::devin::{
    DevinAcpBridgeAction, DevinAcpBridgeAgent, DevinAcpBridgeProbe, DevinAcpProbeStatus,
    DevinAcpRuntimeStatus, DevinDesktopStatus, DevinSessionRecord,
};

use super::{
    ACP_CLIENT_HOST_INSTALL_ACTION_ID, ACP_CLIENT_HOST_PROBE_ACTION_ID, AcpClientHost,
    AcpClientHostAction, AcpClientHostAgent, AcpClientHostInstall, AcpClientHostProbe,
    AcpClientHostRegistry, AcpClientHostRuntime, AcpClientHostSession, acp_client_host_action_path,
};

pub const DEVIN_ACP_CLIENT_HOST_ID: &str = "devin";
const DEVIN_ACP_CLIENT_HOST_LABEL: &str = "Devin Desktop";

pub fn devin_acp_client_host(
    status: &DevinDesktopStatus,
    sessions: &[DevinSessionRecord],
    runtime: &DevinAcpRuntimeStatus,
) -> AcpClientHost {
    AcpClientHost {
        id: DEVIN_ACP_CLIENT_HOST_ID.to_owned(),
        label: DEVIN_ACP_CLIENT_HOST_LABEL.to_owned(),
        running: status
            .installations
            .iter()
            .any(|installation| installation.running),
        installed: status
            .installations
            .iter()
            .any(|installation| installation.installed),
        registry: AcpClientHostRegistry {
            path: status.acp_registry.path.clone(),
            exists: status.acp_registry.exists,
            version: status.acp_registry.version.clone(),
            agent_count: status.acp_registry.agents.len(),
        },
        agents: status
            .acp_bridge
            .agents
            .iter()
            .map(acp_client_host_agent)
            .collect(),
        sessions: sessions.iter().map(acp_client_host_session).collect(),
        actions: acp_client_host_actions(DEVIN_ACP_CLIENT_HOST_ID, &status.acp_bridge.actions),
        limitations: status.acp_bridge.limitations.clone(),
        runtime: Some(AcpClientHostRuntime {
            connected: runtime.connected,
            connection_count: runtime.connection_count,
            session_count: runtime.session_count,
        }),
    }
}

pub fn acp_client_host_probe(probe: DevinAcpBridgeProbe) -> AcpClientHostProbe {
    AcpClientHostProbe {
        ok: probe.ok,
        status: probe_status_name(probe.status).to_owned(),
        agent_id: probe.agent_id,
        name: probe.name,
        control_level: serde_plain_control_level(probe.control_level),
        ready: probe.ready,
        probe_kind: probe.probe_kind,
        launch_configured: probe.launch_configured,
        launch_methods: probe.launch_methods,
        supported_methods: probe.supported_methods,
        blockers: probe.blockers,
        detail: probe.detail,
    }
}

pub fn acp_client_host_install(
    client_id: &str,
    install: crate::devin::DevinAcpInstallResult,
) -> AcpClientHostInstall {
    AcpClientHostInstall {
        client_id: client_id.to_owned(),
        installed_agent_id: install.installed_agent_id,
        registry_path: install.registry_path,
        settings_path: install.settings_path,
        transport_url: install.websocket_url,
        preferred_agent: install.preferred_agent,
    }
}

fn acp_client_host_agent(agent: &DevinAcpBridgeAgent) -> AcpClientHostAgent {
    AcpClientHostAgent {
        id: agent.id.clone(),
        name: agent.name.clone(),
        version: agent.version.clone(),
        description: agent.description.clone(),
        enabled: agent.enabled,
        preferred: agent.preferred,
        launch_configured: agent.launch_configured,
        control_level: serde_plain_control_level(agent.control_level),
        supports_sessions: agent.supports_sessions,
        supports_prompt: agent.supports_prompt,
        supports_cancel: agent.supports_cancel,
        source: agent.source.clone(),
    }
}

fn acp_client_host_session(session: &DevinSessionRecord) -> AcpClientHostSession {
    AcpClientHostSession {
        thread_id: session.thread_id.clone(),
        session_id: session.session_id.clone(),
        provider_id: session.provider_id.clone(),
        title: session.title.clone(),
        cwd: session.cwd.clone(),
        status: session.status.clone(),
        archived: session.archived,
        updated_at_ms: session.updated_at_ms,
    }
}

fn acp_client_host_actions(
    client_id: &str,
    actions: &[DevinAcpBridgeAction],
) -> Vec<AcpClientHostAction> {
    actions
        .iter()
        .map(|action| AcpClientHostAction {
            id: action.id.clone(),
            label: action.label.clone(),
            method: action.method.clone(),
            path: match action.id.as_str() {
                ACP_CLIENT_HOST_INSTALL_ACTION_ID | ACP_CLIENT_HOST_PROBE_ACTION_ID => {
                    acp_client_host_action_path(client_id, &action.id)
                }
                _ => action.path.clone(),
            },
            default_agent_id: action.default_agent_id.clone(),
        })
        .collect()
}

fn probe_status_name(status: DevinAcpProbeStatus) -> &'static str {
    match status {
        DevinAcpProbeStatus::Ready => "ready",
        DevinAcpProbeStatus::Blocked => "blocked",
    }
}

fn serde_plain_control_level(control_level: crate::devin::DevinAcpControlLevel) -> String {
    match control_level {
        crate::devin::DevinAcpControlLevel::VisibilityOnly => "visibility-only".to_owned(),
        crate::devin::DevinAcpControlLevel::AgentConfigured => "agent-configured".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devin::{DevinAcpBridgeStatus, DevinAcpControlLevel, DevinAcpRegistryStatus};

    #[test]
    fn maps_devin_status_to_generic_acp_client_host() {
        let status = DevinDesktopStatus {
            installations: vec![crate::devin::DevinInstallationStatus {
                id: "devin-desktop-next".to_owned(),
                label: "Devin Next".to_owned(),
                channel: "next".to_owned(),
                running: true,
                installed: true,
                app_support_path: "/tmp/devin".to_owned(),
                settings_path: "/tmp/devin/User/settings.json".to_owned(),
                settings_exists: true,
                acp_enabled: Some(true),
                preferred_agent: Some("looper".to_owned()),
                enabled_agents: vec!["looper".to_owned()],
            }],
            acp_registry: DevinAcpRegistryStatus {
                path: "/tmp/.devin-next/acp/registry.json".to_owned(),
                exists: true,
                version: Some("1.0.0".to_owned()),
                agents: Vec::new(),
            },
            acp_bridge: DevinAcpBridgeStatus {
                available: true,
                control_level: DevinAcpControlLevel::AgentConfigured,
                summary: "ready".to_owned(),
                supported_methods: vec!["initialize".to_owned()],
                limitations: vec!["never auto-executes".to_owned()],
                actions: vec![DevinAcpBridgeAction {
                    id: "probe".to_owned(),
                    label: "Probe Devin agent".to_owned(),
                    method: "POST".to_owned(),
                    path: "/desktop/devin/acp-bridge/probe".to_owned(),
                    default_agent_id: Some("looper".to_owned()),
                }],
                agents: vec![DevinAcpBridgeAgent {
                    id: "looper".to_owned(),
                    name: "Looper".to_owned(),
                    version: Some("1.0.0".to_owned()),
                    description: None,
                    enabled: true,
                    preferred: true,
                    launch_configured: true,
                    control_level: DevinAcpControlLevel::AgentConfigured,
                    supports_sessions: true,
                    supports_prompt: true,
                    supports_cancel: true,
                    source: "devin-acp-registry".to_owned(),
                }],
            },
            argv_path: "/tmp/.devin-next/argv.json".to_owned(),
            extensions_path: "/tmp/.devin-next/extensions".to_owned(),
        };
        let runtime = DevinAcpRuntimeStatus {
            connected: true,
            connection_count: 1,
            session_count: 0,
            sessions: Vec::new(),
        };

        let host = devin_acp_client_host(&status, &[], &runtime);

        assert_eq!(host.id, "devin");
        assert!(host.running);
        assert_eq!(host.registry.agent_count, 0);
        assert_eq!(host.agents[0].control_level, "agent-configured");
        assert_eq!(
            host.actions[0].path,
            "/desktop/acp-client-hosts/devin/probe"
        );
        assert_eq!(host.runtime.expect("runtime").connection_count, 1);
    }

    #[test]
    fn generic_action_paths_use_client_host_id() {
        let actions = acp_client_host_actions(
            "zed",
            &[
                DevinAcpBridgeAction {
                    id: "install".to_owned(),
                    label: "Install bridge".to_owned(),
                    method: "POST".to_owned(),
                    path: "/desktop/devin/acp-bridge/install".to_owned(),
                    default_agent_id: None,
                },
                DevinAcpBridgeAction {
                    id: "probe".to_owned(),
                    label: "Probe agent".to_owned(),
                    method: "POST".to_owned(),
                    path: "/desktop/devin/acp-bridge/probe".to_owned(),
                    default_agent_id: Some("agent".to_owned()),
                },
                DevinAcpBridgeAction {
                    id: "custom".to_owned(),
                    label: "Custom action".to_owned(),
                    method: "POST".to_owned(),
                    path: "/desktop/custom/action".to_owned(),
                    default_agent_id: None,
                },
            ],
        );

        assert_eq!(actions[0].path, "/desktop/acp-client-hosts/zed/install");
        assert_eq!(actions[1].path, "/desktop/acp-client-hosts/zed/probe");
        assert_eq!(actions[2].path, "/desktop/custom/action");
    }
}

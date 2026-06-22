use crate::acp::runtime::{
    LOOPER_ACP_AGENT_ID, LooperAcpRuntimeStatus, public_agent_id_for_client_agent_id,
};
use crate::zed::{ZED_CLIENT_ID, ZED_CLIENT_NAME, ZedAcpInstallResult, ZedAcpTarget, ZedStatus};

mod probe;
mod session;
pub use probe::zed_acp_client_host_probe;
use session::zed_acp_client_host_session;

use super::{
    ACP_CLIENT_HOST_INSTALL_ACTION_ID, ACP_CLIENT_HOST_PROBE_ACTION_ID, AcpClientHost,
    AcpClientHostAction, AcpClientHostAgent, AcpClientHostInstall, AcpClientHostRegistry,
    AcpClientHostRuntime, acp_client_host_action_path,
};

pub const ZED_ACP_CLIENT_HOST_ID: &str = ZED_CLIENT_ID;
const ZED_ACP_CLIENT_HOST_INSTALL_ACTION_LABEL: &str = "Install Zed host control";
const ZED_ACP_CLIENT_HOST_PROBE_ACTION_LABEL: &str = "Probe Zed target";
pub(super) const AGENT_CONFIGURED_CONTROL_LEVEL: &str = "agent-configured";
pub(super) const VISIBILITY_ONLY_CONTROL_LEVEL: &str = "visibility-only";
const ZED_AGENT_SOURCE: &str = "zed-agent-servers";
const ZED_LIMITATION_WITH_MANAGED_TARGET: &str = "Looper controls Zed targets whose ACP launch path is wrapped by Looper; other Zed External Agents stay visibility-only.";
const ZED_LIMITATION_WITHOUT_MANAGED_TARGET: &str =
    "Install Zed host control to wrap the Codex CLI ACP target through Looper.";

pub fn zed_acp_client_host(status: &ZedStatus, runtime: &LooperAcpRuntimeStatus) -> AcpClientHost {
    AcpClientHost {
        id: ZED_ACP_CLIENT_HOST_ID.to_owned(),
        label: ZED_CLIENT_NAME.to_owned(),
        running: status.running,
        installed: status.installed || zed_has_looper_managed_target(status),
        registry: AcpClientHostRegistry {
            path: status.settings_path.clone(),
            exists: status.settings_exists,
            version: None,
            agent_count: status.acp_target_count,
        },
        agents: status
            .acp_targets
            .iter()
            .map(zed_acp_client_host_agent)
            .collect(),
        sessions: runtime
            .sessions
            .iter()
            .map(zed_acp_client_host_session)
            .collect(),
        actions: zed_acp_client_host_actions(status),
        limitations: zed_acp_client_host_limitations(status),
        runtime: Some(AcpClientHostRuntime {
            connected: runtime.connected,
            connection_count: runtime.connection_count,
            session_count: runtime.session_count,
        }),
    }
}

pub fn zed_acp_client_host_install(
    client_id: &str,
    install: ZedAcpInstallResult,
) -> AcpClientHostInstall {
    let installed_agent_id =
        public_agent_id_for_client_agent_id(client_id, &install.installed_agent_id);
    let preferred_agent = public_agent_id_for_client_agent_id(client_id, &install.preferred_agent);
    AcpClientHostInstall {
        client_id: client_id.to_owned(),
        installed_agent_id,
        registry_path: install.settings_path.clone(),
        settings_path: install.settings_path,
        transport_url: install.command_line,
        preferred_agent,
    }
}

fn zed_acp_client_host_limitations(status: &ZedStatus) -> Vec<String> {
    vec![
        if zed_has_looper_managed_target(status) {
            ZED_LIMITATION_WITH_MANAGED_TARGET
        } else {
            ZED_LIMITATION_WITHOUT_MANAGED_TARGET
        }
        .to_owned(),
    ]
}

fn zed_acp_client_host_actions(status: &ZedStatus) -> Vec<AcpClientHostAction> {
    let mut actions = vec![AcpClientHostAction {
        id: ACP_CLIENT_HOST_INSTALL_ACTION_ID.to_owned(),
        label: ZED_ACP_CLIENT_HOST_INSTALL_ACTION_LABEL.to_owned(),
        method: "POST".to_owned(),
        path: acp_client_host_action_path(
            ZED_ACP_CLIENT_HOST_ID,
            ACP_CLIENT_HOST_INSTALL_ACTION_ID,
        ),
        default_agent_id: None,
    }];
    actions.extend(
        default_probe_target(status).map(|default_target| AcpClientHostAction {
            id: ACP_CLIENT_HOST_PROBE_ACTION_ID.to_owned(),
            label: ZED_ACP_CLIENT_HOST_PROBE_ACTION_LABEL.to_owned(),
            method: "POST".to_owned(),
            path: acp_client_host_action_path(
                ZED_ACP_CLIENT_HOST_ID,
                ACP_CLIENT_HOST_PROBE_ACTION_ID,
            ),
            default_agent_id: Some(public_agent_id_for_client_agent_id(
                ZED_ACP_CLIENT_HOST_ID,
                &default_target.id,
            )),
        }),
    );
    actions
}

fn zed_acp_client_host_agent(agent: &ZedAcpTarget) -> AcpClientHostAgent {
    let managed = agent.looper_managed;
    let looper_owned_runtime = managed && agent.id == LOOPER_ACP_AGENT_ID;
    let public_agent_id = public_agent_id_for_client_agent_id(ZED_ACP_CLIENT_HOST_ID, &agent.id);
    AcpClientHostAgent {
        id: public_agent_id,
        name: agent.name.clone(),
        version: None,
        description: None,
        enabled: true,
        preferred: managed,
        launch_configured: agent.launch_configured,
        control_level: if managed {
            AGENT_CONFIGURED_CONTROL_LEVEL
        } else {
            VISIBILITY_ONLY_CONTROL_LEVEL
        }
        .to_owned(),
        supports_sessions: managed,
        supports_prompt: looper_owned_runtime,
        supports_cancel: looper_owned_runtime,
        source: ZED_AGENT_SOURCE.to_owned(),
    }
}

pub(super) fn default_probe_target(status: &ZedStatus) -> Option<&ZedAcpTarget> {
    status
        .acp_targets
        .iter()
        .find(|target| target.looper_managed)
        .or_else(|| status.acp_targets.first())
}

fn zed_has_looper_managed_target(status: &ZedStatus) -> bool {
    status
        .acp_targets
        .iter()
        .any(|target| target.looper_managed)
}

#[cfg(test)]
mod tests;

//! Zed ACP client-host mapping.
//!
//! Zed owns External Agent installation, authentication, and runtime control.
//! Looper deliberately reports configured targets as read-only visibility so
//! menu and mobile surfaces can show Zed support without implying control.

use crate::zed::{ZED_CLIENT_ID, ZED_CLIENT_NAME, ZedAcpTarget, ZedStatus};

use super::{
    ACP_CLIENT_HOST_PROBE_ACTION_ID, AcpClientHost, AcpClientHostAction, AcpClientHostAgent,
    AcpClientHostProbe, AcpClientHostRegistry, acp_client_host_action_path,
};

pub const ZED_ACP_CLIENT_HOST_ID: &str = ZED_CLIENT_ID;
const ZED_ACP_CLIENT_HOST_PROBE_ACTION_LABEL: &str = "Inspect Zed target";
const VISIBILITY_ONLY_CONTROL_LEVEL: &str = "visibility-only";
const ZED_AGENT_SOURCE: &str = "zed-agent-servers";
const ZED_HOST_LIMITATION: &str =
    "Read-only: Zed manages External Agent install, auth, and runtime inside Zed.";
const ZED_ACP_HOST_PROBE_STATUS: &str = "blocked";
const ZED_ACP_HOST_PROBE_KIND: &str = "read-only-visibility";
const ZED_ACP_HOST_PROBE_DETAIL: &str =
    "Zed External Agents are managed in Zed; Looper reports configured agent_servers read-only.";
const ZED_ACP_HOST_PROBE_BLOCKER: &str =
    "Install, authentication, and runtime control stay in Zed Agent Settings.";
const ZED_ACP_HOST_PROBE_UNKNOWN_AGENT_BLOCKER: &str =
    "Requested Zed External Agent target is not configured in agent_servers.";

pub fn zed_acp_client_host(status: &ZedStatus) -> AcpClientHost {
    AcpClientHost {
        id: ZED_ACP_CLIENT_HOST_ID.to_owned(),
        label: ZED_CLIENT_NAME.to_owned(),
        running: status.running,
        installed: status.installed,
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
        sessions: Vec::new(),
        actions: zed_acp_client_host_actions(status),
        limitations: vec![ZED_HOST_LIMITATION.to_owned()],
        runtime: None,
    }
}

pub fn zed_acp_client_host_probe(status: &ZedStatus, agent_id: Option<&str>) -> AcpClientHostProbe {
    let requested_target = agent_id.and_then(|requested_id| {
        status
            .acp_targets
            .iter()
            .find(|target| target.id == requested_id)
    });
    let target = if agent_id.is_some() {
        requested_target
    } else {
        status.acp_targets.first()
    };
    let requested_agent_missing = agent_id.is_some() && target.is_none();

    AcpClientHostProbe {
        ok: false,
        status: ZED_ACP_HOST_PROBE_STATUS.to_owned(),
        agent_id: agent_id
            .map(str::to_owned)
            .or_else(|| target.map(|target| target.id.clone())),
        name: target.map(|target| target.name.clone()),
        control_level: VISIBILITY_ONLY_CONTROL_LEVEL.to_owned(),
        ready: false,
        probe_kind: ZED_ACP_HOST_PROBE_KIND.to_owned(),
        launch_configured: target
            .map(|target| target.launch_configured)
            .unwrap_or(false),
        launch_methods: target
            .map(|target| target.launch.methods.clone())
            .unwrap_or_default(),
        supported_methods: Vec::new(),
        blockers: zed_acp_client_host_probe_blockers(requested_agent_missing),
        detail: ZED_ACP_HOST_PROBE_DETAIL.to_owned(),
    }
}

fn zed_acp_client_host_probe_blockers(requested_agent_missing: bool) -> Vec<String> {
    let mut blockers = vec![ZED_ACP_HOST_PROBE_BLOCKER.to_owned()];
    if requested_agent_missing {
        blockers.push(ZED_ACP_HOST_PROBE_UNKNOWN_AGENT_BLOCKER.to_owned());
    }
    blockers
}

fn zed_acp_client_host_actions(status: &ZedStatus) -> Vec<AcpClientHostAction> {
    let Some(default_target) = status.acp_targets.first() else {
        return Vec::new();
    };

    vec![AcpClientHostAction {
        id: ACP_CLIENT_HOST_PROBE_ACTION_ID.to_owned(),
        label: ZED_ACP_CLIENT_HOST_PROBE_ACTION_LABEL.to_owned(),
        method: "POST".to_owned(),
        path: acp_client_host_action_path(ZED_ACP_CLIENT_HOST_ID, ACP_CLIENT_HOST_PROBE_ACTION_ID),
        default_agent_id: Some(default_target.id.clone()),
    }]
}

fn zed_acp_client_host_agent(agent: &ZedAcpTarget) -> AcpClientHostAgent {
    AcpClientHostAgent {
        id: agent.id.clone(),
        name: agent.name.clone(),
        version: None,
        description: None,
        enabled: true,
        preferred: false,
        launch_configured: agent.launch_configured,
        control_level: VISIBILITY_ONLY_CONTROL_LEVEL.to_owned(),
        supports_sessions: false,
        supports_prompt: false,
        supports_cancel: false,
        source: ZED_AGENT_SOURCE.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp_targets::AcpLaunchMetadata;

    #[test]
    fn maps_zed_status_to_read_only_acp_client_host() {
        let status = ZedStatus {
            settings_path: "/tmp/.zed/settings.json".to_owned(),
            settings_exists: true,
            settings_error: None,
            running: true,
            installed: true,
            summary: "Zed is running with 1 configured ACP External Agent target".to_owned(),
            acp_target_count: 1,
            acp_targets: vec![ZedAcpTarget {
                id: "looper".to_owned(),
                name: "looper".to_owned(),
                target_type: Some("custom".to_owned()),
                launch_configured: true,
                launch: AcpLaunchMetadata {
                    configured: true,
                    methods: vec!["command".to_owned()],
                },
            }],
        };

        let host = zed_acp_client_host(&status);

        assert_eq!(host.id, "zed");
        assert_eq!(host.label, "Zed");
        assert!(host.running);
        assert_eq!(host.registry.path, "/tmp/.zed/settings.json");
        assert_eq!(host.registry.agent_count, 1);
        assert_eq!(host.agents[0].id, "looper");
        assert_eq!(host.agents[0].control_level, "visibility-only");
        assert_eq!(host.actions.len(), 1);
        assert_eq!(host.actions[0].id, "probe");
        assert_eq!(host.actions[0].path, "/desktop/acp-client-hosts/zed/probe");
        assert!(host.runtime.is_none());
        assert!(
            host.limitations
                .iter()
                .any(|limitation| limitation.contains("Read-only"))
        );
    }

    #[test]
    fn zed_probe_is_visibility_only_and_never_ready() {
        let status = ZedStatus {
            settings_path: "/tmp/.zed/settings.json".to_owned(),
            settings_exists: true,
            settings_error: None,
            running: true,
            installed: true,
            summary: "Zed is running with 1 configured ACP External Agent target".to_owned(),
            acp_target_count: 1,
            acp_targets: vec![ZedAcpTarget {
                id: "looper".to_owned(),
                name: "looper".to_owned(),
                target_type: Some("custom".to_owned()),
                launch_configured: true,
                launch: AcpLaunchMetadata {
                    configured: true,
                    methods: vec!["command".to_owned()],
                },
            }],
        };

        let probe = zed_acp_client_host_probe(&status, Some("looper"));

        assert!(!probe.ok);
        assert!(!probe.ready);
        assert_eq!(probe.status, "blocked");
        assert_eq!(probe.control_level, "visibility-only");
        assert_eq!(probe.probe_kind, "read-only-visibility");
        assert_eq!(probe.agent_id.as_deref(), Some("looper"));
        assert_eq!(probe.launch_methods, vec!["command"]);
        assert!(
            probe
                .blockers
                .iter()
                .any(|blocker| blocker.contains("runtime control stay in Zed"))
        );
    }

    #[test]
    fn zed_probe_reports_missing_requested_agent() {
        let status = ZedStatus {
            settings_path: "/tmp/.zed/settings.json".to_owned(),
            settings_exists: true,
            settings_error: None,
            running: true,
            installed: true,
            summary: "Zed is running with 1 configured ACP External Agent target".to_owned(),
            acp_target_count: 1,
            acp_targets: vec![ZedAcpTarget {
                id: "looper".to_owned(),
                name: "looper".to_owned(),
                target_type: Some("custom".to_owned()),
                launch_configured: true,
                launch: AcpLaunchMetadata {
                    configured: true,
                    methods: vec!["command".to_owned()],
                },
            }],
        };

        let probe = zed_acp_client_host_probe(&status, Some("missing"));

        assert_eq!(probe.agent_id.as_deref(), Some("missing"));
        assert_eq!(probe.name, None);
        assert!(!probe.launch_configured);
        assert!(probe.launch_methods.is_empty());
        assert!(
            probe
                .blockers
                .iter()
                .any(|blocker| blocker.contains("not configured"))
        );
    }
}

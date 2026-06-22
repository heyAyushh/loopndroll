use crate::acp::runtime::{public_agent_id_for_client_agent_id, supported_acp_methods};
use crate::zed::{ZedAcpTarget, ZedStatus};

use super::{
    AGENT_CONFIGURED_CONTROL_LEVEL, VISIBILITY_ONLY_CONTROL_LEVEL, ZED_ACP_CLIENT_HOST_ID,
    default_probe_target,
};
use crate::acp::client_host::AcpClientHostProbe;

const ZED_ACP_HOST_READY_STATUS: &str = "ready";
const ZED_ACP_HOST_BLOCKED_STATUS: &str = "blocked";
const ZED_ACP_HOST_READY_PROBE_KIND: &str = "looper-stdio";
const ZED_ACP_HOST_BLOCKED_PROBE_KIND: &str = "read-only-visibility";
const ZED_ACP_HOST_READY_DETAIL: &str =
    "Zed is configured to launch this ACP target through Looper host control.";
const ZED_ACP_HOST_BLOCKED_DETAIL: &str =
    "This Zed External Agent is visible but its host launch path is not wrapped by Looper.";
const ZED_ACP_HOST_NOT_RUNNING_DETAIL: &str =
    "Zed host control is configured for this target, but Zed is not running.";
const ZED_ACP_HOST_NOT_INSTALLED_DETAIL: &str =
    "Zed host control is configured for this target, but Zed is not installed.";
const ZED_ACP_HOST_MISSING_LAUNCH_DETAIL: &str =
    "Zed host control is configured for this target, but its launch command is missing.";
const ZED_ACP_HOST_BLOCKER: &str =
    "Install Zed host control or select a target whose launch path is wrapped by Looper.";
const ZED_ACP_HOST_NOT_RUNNING_BLOCKER: &str = "Start Zed before probing this ACP target.";
const ZED_ACP_HOST_NOT_INSTALLED_BLOCKER: &str = "Install Zed before probing this ACP target.";
const ZED_ACP_HOST_MISSING_LAUNCH_BLOCKER: &str =
    "Reinstall Zed host control to restore the wrapped ACP launch command.";
const ZED_ACP_HOST_UNKNOWN_AGENT_BLOCKER: &str =
    "Requested Zed External Agent target is not configured in agent_servers.";

pub fn zed_acp_client_host_probe(status: &ZedStatus, agent_id: Option<&str>) -> AcpClientHostProbe {
    let requested_target = agent_id.and_then(|requested_id| {
        status
            .acp_targets
            .iter()
            .find(|target| zed_target_matches_requested_agent_id(target, requested_id))
    });
    let target = if agent_id.is_some() {
        requested_target
    } else {
        default_probe_target(status)
    };
    let requested_agent_missing = agent_id.is_some() && target.is_none();
    let managed = target
        .map(|target| target.looper_managed)
        .unwrap_or_default();
    let command_configured = target
        .map(zed_target_has_command_launch)
        .unwrap_or_default();
    let ready = managed && command_configured && status.running && status.installed;

    AcpClientHostProbe {
        ok: ready,
        status: probe_status(ready).to_owned(),
        agent_id: target
            .map(|target| public_agent_id_for_client_agent_id(ZED_ACP_CLIENT_HOST_ID, &target.id))
            .or_else(|| agent_id.map(str::to_owned)),
        name: target.map(|target| target.name.clone()),
        control_level: control_level(managed).to_owned(),
        ready,
        probe_kind: probe_kind(ready).to_owned(),
        launch_configured: target
            .map(|target| target.launch_configured)
            .unwrap_or(false),
        launch_methods: target
            .map(|target| target.launch.methods.clone())
            .unwrap_or_default(),
        supported_methods: if ready {
            supported_acp_methods()
        } else {
            Vec::new()
        },
        blockers: zed_acp_client_host_probe_blockers(
            ready,
            managed,
            command_configured,
            status.running,
            status.installed,
            requested_agent_missing,
        ),
        detail: zed_acp_client_host_probe_detail(
            ready,
            managed,
            command_configured,
            status.running,
            status.installed,
            requested_agent_missing,
        )
        .to_owned(),
    }
}

fn probe_status(ready: bool) -> &'static str {
    if ready {
        ZED_ACP_HOST_READY_STATUS
    } else {
        ZED_ACP_HOST_BLOCKED_STATUS
    }
}

fn probe_kind(ready: bool) -> &'static str {
    if ready {
        ZED_ACP_HOST_READY_PROBE_KIND
    } else {
        ZED_ACP_HOST_BLOCKED_PROBE_KIND
    }
}

fn control_level(managed: bool) -> &'static str {
    if managed {
        AGENT_CONFIGURED_CONTROL_LEVEL
    } else {
        VISIBILITY_ONLY_CONTROL_LEVEL
    }
}

fn zed_target_has_command_launch(target: &ZedAcpTarget) -> bool {
    target
        .launch
        .methods
        .iter()
        .any(|method| method == "command")
}

fn zed_target_matches_requested_agent_id(target: &ZedAcpTarget, requested_id: &str) -> bool {
    let public_agent_id = public_agent_id_for_client_agent_id(ZED_ACP_CLIENT_HOST_ID, &target.id);
    public_agent_id == requested_id || (target.id == requested_id && public_agent_id == target.id)
}

fn zed_acp_client_host_probe_blockers(
    ready: bool,
    managed: bool,
    launch_configured: bool,
    running: bool,
    installed: bool,
    requested_agent_missing: bool,
) -> Vec<String> {
    if ready {
        return Vec::new();
    }
    let mut blockers = Vec::new();
    if requested_agent_missing {
        blockers.push(ZED_ACP_HOST_UNKNOWN_AGENT_BLOCKER.to_owned());
    }
    if !managed {
        blockers.push(ZED_ACP_HOST_BLOCKER.to_owned());
    }
    if managed && !launch_configured {
        blockers.push(ZED_ACP_HOST_MISSING_LAUNCH_BLOCKER.to_owned());
    }
    if managed && !running {
        blockers.push(ZED_ACP_HOST_NOT_RUNNING_BLOCKER.to_owned());
    }
    if managed && running && !installed {
        blockers.push(ZED_ACP_HOST_NOT_INSTALLED_BLOCKER.to_owned());
    }
    blockers
}

fn zed_acp_client_host_probe_detail(
    ready: bool,
    managed: bool,
    launch_configured: bool,
    running: bool,
    installed: bool,
    requested_agent_missing: bool,
) -> &'static str {
    if ready {
        return ZED_ACP_HOST_READY_DETAIL;
    }
    if requested_agent_missing || !managed {
        return ZED_ACP_HOST_BLOCKED_DETAIL;
    }
    if !launch_configured {
        return ZED_ACP_HOST_MISSING_LAUNCH_DETAIL;
    }
    if !running {
        return ZED_ACP_HOST_NOT_RUNNING_DETAIL;
    }
    if !installed {
        return ZED_ACP_HOST_NOT_INSTALLED_DETAIL;
    }
    ZED_ACP_HOST_BLOCKED_DETAIL
}

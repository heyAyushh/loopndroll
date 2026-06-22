use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::control_plane::GrokBuildStatus;
use crate::devin::{DevinDesktopStatus, DevinSessionDiscoveryError};
use crate::grok_build::GrokHookStatus;

pub(super) fn mobile_grok_build_status(grok_build: &GrokBuildStatus) -> Value {
    json!({
        "hooks": mobile_grok_hook_status(&grok_build.hooks),
        "sessionCount": grok_build.session_count,
        "activeSessionCount": grok_build.active_session_count,
    })
}

pub(super) fn mobile_devin_desktop_status(
    devin_desktop: &DevinDesktopStatus,
    session_count: usize,
    active_session_count: usize,
    session_errors: &[DevinSessionDiscoveryError],
) -> Value {
    let running = devin_desktop
        .installations
        .iter()
        .any(|installation| installation.running);
    let installed = devin_desktop
        .installations
        .iter()
        .any(|installation| installation.installed);
    let enabled_agent_ids = devin_desktop
        .installations
        .iter()
        .flat_map(|installation| installation.enabled_agents.iter().cloned())
        .collect::<BTreeSet<_>>();
    let preferred_agent_ids = devin_desktop
        .installations
        .iter()
        .filter_map(|installation| installation.preferred_agent.clone())
        .collect::<BTreeSet<_>>();

    json!({
        "running": running,
        "installed": installed,
        "acpAvailable": devin_desktop.acp_bridge.available,
        "registryExists": devin_desktop.acp_registry.exists,
        "registryAgentCount": devin_desktop.acp_registry.agents.len(),
        "enabledAgentCount": enabled_agent_ids.len(),
        "preferredAgentIds": preferred_agent_ids.into_iter().collect::<Vec<_>>(),
        "sessionCount": session_count,
        "activeSessionCount": active_session_count,
        "sessionDiagnostics": session_errors
            .iter()
            .map(mobile_devin_session_discovery_error)
            .collect::<Vec<_>>(),
    })
}

fn mobile_devin_session_discovery_error(error: &DevinSessionDiscoveryError) -> Value {
    json!({
        "source": error.source,
        "code": error.code,
        "detail": error.detail,
    })
}

fn mobile_grok_hook_status(hooks: &GrokHookStatus) -> Value {
    json!({
        "health": hooks.health,
        "owner": hooks.owner,
        "registeredEvents": hooks.registered_events,
        "hooksPath": hooks.hooks_path,
    })
}

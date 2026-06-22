use serde::{Deserialize, Serialize};

/// Response returned by the desktop ACP client-host collection endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostsResponse {
    /// Known ACP-capable client hosts in deterministic provider order.
    pub hosts: Vec<AcpClientHost>,
}

/// Response returned by the desktop ACP client-host detail endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostResponse {
    /// Host detail for the requested client id.
    pub host: AcpClientHost,
}

/// Response returned by a client-host probe action.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostProbeResponse {
    /// Fresh host snapshot collected with the probe result.
    pub host: AcpClientHost,
    /// Probe outcome for the requested host and optional agent id.
    pub probe: AcpClientHostProbe,
}

/// Response returned by a client-host install action.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostInstallResponse {
    /// Fresh host snapshot collected after installation.
    pub host: AcpClientHost,
    /// Installation details for hosts whose bridge is managed by Looper.
    pub install: AcpClientHostInstall,
}

/// Normalized desktop application that can expose ACP agents or sessions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHost {
    /// Stable lowercase client id, for example `devin` or `zed`.
    pub id: String,
    /// Human-readable desktop client label.
    pub label: String,
    /// Whether a matching client process is currently running.
    pub running: bool,
    /// Whether the client or its Looper-managed integration is installed.
    pub installed: bool,
    /// Registry or settings file used to discover agents.
    pub registry: AcpClientHostRegistry,
    /// Agents or targets exposed by this host.
    pub agents: Vec<AcpClientHostAgent>,
    /// Sessions owned by hosts that expose session metadata.
    pub sessions: Vec<AcpClientHostSession>,
    /// Host-specific operations the UI may invoke.
    pub actions: Vec<AcpClientHostAction>,
    /// Human-readable constraints that explain unsupported operations.
    pub limitations: Vec<String>,
    /// Live runtime information when Looper owns or observes a runtime bridge.
    pub runtime: Option<AcpClientHostRuntime>,
}

/// Registry or settings source used to discover a host's ACP agents.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostRegistry {
    /// Absolute settings or registry path used for discovery.
    pub path: String,
    /// Whether the backing settings or registry path exists.
    pub exists: bool,
    /// Version string reported by the registry when available.
    pub version: Option<String>,
    /// Count of configured agent entries in the registry source.
    pub agent_count: usize,
}

/// ACP agent target advertised by a desktop client host.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostAgent {
    /// Stable host-local agent id.
    pub id: String,
    /// Human-readable agent name.
    pub name: String,
    /// Agent version from the host registry when available.
    pub version: Option<String>,
    /// Optional registry description.
    pub description: Option<String>,
    /// Whether the host considers the agent enabled.
    pub enabled: bool,
    /// Whether the host marks this agent as preferred.
    pub preferred: bool,
    /// Whether launch metadata exists for this target.
    pub launch_configured: bool,
    /// Capability boundary such as `agent-configured` or `visibility-only`.
    pub control_level: String,
    /// Whether the host can list sessions for this agent.
    pub supports_sessions: bool,
    /// Whether Looper can send prompts through this host-agent path.
    pub supports_prompt: bool,
    /// Whether Looper can cancel running work through this host-agent path.
    pub supports_cancel: bool,
    /// Discovery source identifier used for diagnostics.
    pub source: String,
}

/// Session known to an ACP client host.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostSession {
    /// Public Looper thread id.
    pub thread_id: String,
    /// Host-local session id.
    pub session_id: String,
    /// Provider id reported by the host.
    pub provider_id: String,
    /// Optional session title.
    pub title: Option<String>,
    /// Optional session working directory.
    pub cwd: Option<String>,
    /// Host session status.
    pub status: String,
    /// Whether the session is archived.
    pub archived: bool,
    /// Last host update time in Unix milliseconds.
    pub updated_at_ms: Option<i64>,
}

/// Action URL that the menu bar can use for host-specific operations.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostAction {
    /// Stable action id, for example `install` or `probe`.
    pub id: String,
    /// Button label for desktop clients.
    pub label: String,
    /// HTTP method used by the action endpoint.
    pub method: String,
    /// Local control-plane route for this action.
    pub path: String,
    /// Agent id the UI should use when invoking this action by default.
    pub default_agent_id: Option<String>,
}

/// Live runtime connection summary for hosts that Looper can observe directly.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostRuntime {
    /// Whether at least one live runtime connection is active.
    pub connected: bool,
    /// Number of live runtime connections.
    pub connection_count: usize,
    /// Number of runtime sessions currently observed.
    pub session_count: usize,
}

/// Probe result for a host agent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostProbe {
    /// True only when the host-agent path is ready for Looper-owned operation.
    pub ok: bool,
    /// Machine-readable status such as `ready` or `blocked`.
    pub status: String,
    /// Requested or selected agent id.
    pub agent_id: Option<String>,
    /// Agent display name when the host knows it.
    pub name: Option<String>,
    /// Capability boundary for the probed path.
    pub control_level: String,
    /// Whether Looper can use the probed path now.
    pub ready: bool,
    /// Provider-specific probe category.
    pub probe_kind: String,
    /// Whether launch metadata is present.
    pub launch_configured: bool,
    /// Sanitized launch methods, never raw command args or environment values.
    pub launch_methods: Vec<String>,
    /// Protocol methods supported by managed runtime paths.
    pub supported_methods: Vec<String>,
    /// User-actionable reasons the path is not ready.
    pub blockers: Vec<String>,
    /// Human-readable status detail.
    pub detail: String,
}

/// Install result for hosts where Looper owns an ACP bridge install path.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpClientHostInstall {
    /// Client id that received the install action.
    pub client_id: String,
    /// Agent id installed into the host registry.
    pub installed_agent_id: String,
    /// Registry path changed by the install.
    pub registry_path: String,
    /// Host settings path changed by the install.
    pub settings_path: String,
    /// Local bridge transport URL registered with the host.
    pub transport_url: String,
    /// Preferred agent id after install.
    pub preferred_agent: String,
}

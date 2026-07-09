// allow: SIZE_OK — legacy control-plane facade kept as the public coordinator while new ACP responsibilities live in control_plane/acp_hosts/.
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::acp::client_host::{
    AcpClientHostInstallResponse, AcpClientHostProbeResponse, AcpClientHostResponse,
    AcpClientHostsResponse, DEVIN_ACP_CLIENT_HOST_ID, ZED_ACP_CLIENT_HOST_ID,
    acp_client_host_install, acp_client_host_probe, devin_acp_client_host, zed_acp_client_host,
    zed_acp_client_host_install, zed_acp_client_host_probe,
};
use crate::acp::runtime::{
    LOCAL_CONTROL_CONNECTION_ID, LooperAcpControlCancelResponse, LooperAcpControlError,
    LooperAcpControlPromptResponse, LooperAcpControlSessionResponse, LooperAcpObservedSession,
    LooperAcpRuntime, LooperAcpRuntimeSession, public_agent_id_for_client_agent_id,
};
use crate::acp::targets::AcpTarget;
use crate::assistant::{
    AssistantAdapterCapability, AssistantKind, adapter_capabilities,
    discover_assistant_adapters_from_sources, static_adapter_capabilities,
};
use crate::automations::{AutomationSummary, read_automations};
use crate::claude_code::{
    ClaudeHookOwner, ClaudeHookStatus, ClaudeSessionRecord, claude_session_to_desktop_thread,
    claude_transcript_source_signature, default_claude_home,
    discover_claude_sessions_with_processes, discover_recent_claude_sessions_with_processes,
    inspect_claude_hooks, register_owned_claude_hooks, unregister_owned_claude_hooks,
};
use crate::codex::{
    CodexServerOwner, CodexServerProcess, ControlPlaneStatus, DiffSummary, HookOwner, LaunchKind,
    SpawnGraph, StateData, ThreadCapabilities, ThreadRecord, ThreadRevisionRecord,
    capabilities_for_state_thread, discover_sources, inspect_control_plane_with_process_lines,
    inspect_hooks, read_snapshot_state_with_thread_limit, read_state, read_thread_revision_state,
    source_status,
};
use crate::compaction::{CompactionEvent, read_compaction_events, read_recent_compaction_events};
use crate::content_slices::{
    ContentSliceError, SessionContentSliceRequest, SessionContentSliceResponse,
    session_content_slice,
};
use crate::devin::{
    DevinAcpBridgeProbe, DevinAcpBridgeStatus, DevinAcpControlCancelResponse, DevinAcpControlError,
    DevinAcpControlPromptResponse, DevinAcpControlSessionResponse, DevinAcpRuntime,
    DevinAcpRuntimeSession, DevinAcpRuntimeStatus, DevinDesktopStatus, DevinHookOwner,
    DevinHookStatus, DevinInstallationStatus, DevinSessionDiscovery, DevinSessionDiscoveryError,
    build_acp_bridge_probe, devin_acp_targets, devin_connection_detail, devin_session_capabilities,
    devin_session_to_desktop_thread, devin_session_to_thread_record,
    discover_devin_sessions_with_previews, discover_devin_sessions_without_previews,
    discover_recent_devin_sessions, discover_recent_devin_sessions_with_previews,
    inspect_devin_desktop_with_processes, inspect_devin_hooks, install_looper_acp_agent_for_home,
    register_owned_devin_hooks, unregister_owned_devin_hooks,
};
use crate::events::{
    AutomationRunInput, AutomationRunRecord, EventStore, MobileSessionMiniProjectionInput,
    mobile_session_mini_content_fingerprint,
};
use crate::goals::{GoalSummary, ThreadGoalSummary, goal_for_thread, read_goals};
use crate::grok_build::{
    GrokHookOwner, GrokHookStatus, GrokSessionRecord, default_grok_home, discover_grok_sessions,
    grok_session_to_desktop_thread, inspect_grok_hooks, register_owned_grok_hooks,
    unregister_owned_grok_hooks,
};
use crate::grpc::frame_limits::SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES;
use crate::hook_registration::{register_owned_hooks, unregister_owned_hooks};
use crate::mobile::api::{
    latest_session_mini_revision, session_mini_projection_inputs,
    session_mini_projection_inputs_from_records,
};
use crate::mobile::auth::MobileAuthService;
use crate::mobile::events::{
    MobileEvent, MobileEventHub, MobileEventInput, MobileEventKind, MobileTextChunk,
    MobileTextChunkInput, build_mobile_event, mobile_event_now, mobile_state_seq_revision,
    snapshot_revision_changed_event,
};
use crate::mobile::prompt_delivery::{PromptDeliveryActionCache, prime_delivery_action_cache};
use crate::mobile::push::MobilePushService;
use crate::mobile::session::{MobileSessionService, MobileSessionState};
use crate::sync_manifest::SyncManifest;
use crate::telegram::TelegramService;
use crate::transcript_preview::transcript_preview_for_path_fast;
use crate::zed::{
    ZED_CLIENT_ID, ZED_CLIENT_NAME, ZedStatus, inspect_zed_for_home_with_processes,
    install_looper_zed_acp_agent_for_home, zed_acp_targets,
};

mod acp_hosts;
pub mod reducer;
pub mod session_fsm;
use acp_hosts::{
    ACP_CLIENT_HOST_SESSION_LIMIT, AcpClientHostProvider, active_zed_acp_runtime_session_count,
    devin_acp_runtime_session_capabilities, devin_acp_runtime_session_to_desktop_thread,
    zed_acp_connection, zed_acp_runtime_session_capabilities,
    zed_acp_runtime_session_to_desktop_thread,
};

const DESKTOP_COMPACTION_LIMIT: usize = 50;
const DESKTOP_COMPACTION_FILE_SCAN_LIMIT: usize = 250;
const DESKTOP_SNAPSHOT_THREAD_LIMIT: usize = 250;
const DESKTOP_MENU_COMPACTION_LIMIT: usize = 10;
const DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT: usize = 50;
const DESKTOP_MENU_THREAD_LIMIT: usize = 12;
const DESKTOP_MENU_RESPONSE_CACHE_TTL: Duration = Duration::from_secs(5);
const DESKTOP_MENU_INSPECTION_CACHE_TTL: Duration = Duration::from_secs(300);
const SESSION_MINI_PROJECTION_RECONCILE_INTERVAL: Duration = Duration::from_secs(1);
const HOST_PROCESS_COMMAND: &str = "/bin/ps";
const HOST_PROCESS_COMMAND_ARGS: &[&str] = &["-axo", "command="];
const BOUNDED_SNAPSHOT_STALE_HEALTH: &str = "stale";
const CODEX_HOOKS_CONNECTION_ID: &str = "codex-hooks";
const CODEX_HOOKS_CONNECTION_LABEL: &str = "Codex hooks";
const CODEX_CONNECTION_KIND: &str = "codex";
const CODEX_STATE_SOURCE: &str = "vscode";
const CODEX_ACP_SOURCE: &str = "codex-acp";
/// `assistantSurface` values whose sessions are polled from disk (codex rollouts, Claude Code
/// transcripts) rather than pushed by a live ACP host. Devin/Zed publish their own text chunks
/// from `observe_acp_client_host_session_response`, so they are deliberately excluded here to
/// avoid double-publishing.
const TEXT_CHUNK_POLLED_ASSISTANT_SURFACES: &[&str] = &["codex", "claude-code"];
const DEVIN_CONNECTION_KIND: &str = "devin";
const DEVIN_HOOKS_CONNECTION_ID: &str = "devin-hooks";
const DEVIN_HOOKS_CONNECTION_LABEL: &str = "Devin hooks";
const GROK_BUILD_CONNECTION_KIND: &str = "grok-build";
const GROK_BUILD_CONNECTION_ID: &str = "grok-build-cli";
const GROK_BUILD_CONNECTION_LABEL: &str = "Grok Build CLI";
const GROK_BUILD_CLI_RUNTIME_LABEL: &str = "Grok Build CLI";
const GROK_BUILD_SESSION_RUNTIME_LABEL: &str = "Grok Build session";
const CLAUDE_CODE_CONNECTION_KIND: &str = "claude-code";
const CLAUDE_CODE_HOOKS_CONNECTION_ID: &str = "claude-code-hooks";
const CLAUDE_CODE_HOOKS_CONNECTION_LABEL: &str = "Claude Code hooks";
const CLAUDE_CODE_HOOKS_CONNECTION_ACTION_HINT: &str =
    "Claude Code hooks in ~/.claude/settings.json; running-session prompts are delivered on Stop.";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SnapshotInspectionMode {
    Live,
    CachedMenu,
    Mobile,
}

impl SnapshotInspectionMode {
    fn includes_diagnostic_details(self) -> bool {
        self == Self::Live
    }

    fn includes_transcript_previews(self) -> bool {
        matches!(self, Self::Live | Self::Mobile)
    }

    fn discovers_live_external_sessions(self) -> bool {
        matches!(self, Self::Live | Self::Mobile)
    }
}

const MOBILE_CONNECTION_KIND: &str = "mobile";
const READ_ONLY_CONNECTION_ACTION_HINT: &str = "Detected from local Codex state.";
const DEVIN_CONNECTION_ACTION_HINT: &str =
    "Devin Desktop metadata plus local hook delivery for Devin Local sessions.";
const DEVIN_HOOKS_CONNECTION_ACTION_HINT: &str = "Devin Local hooks in ~/.config/devin/config.json; running-session prompts are delivered on Stop.";
const GROK_BUILD_CONNECTION_ACTION_HINT: &str =
    "Grok Build hooks at ~/.grok/hooks/looper.json; sessions read from ~/.grok/sessions/.";
const GROK_BUILD_HOOKS_CONNECTION_ID: &str = "grok-build-hooks";
const GROK_BUILD_HOOKS_CONNECTION_LABEL: &str = "Grok Build hooks";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookMutationTarget {
    Codex,
    Devin,
    GrokBuild,
    ClaudeCode,
}

impl HookMutationTarget {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "codex" => Some(Self::Codex),
            "devin" | "devin-local" => Some(Self::Devin),
            "grok" | "grok-build" => Some(Self::GrokBuild),
            "claude" | "claude-code" => Some(Self::ClaudeCode),
            _ => None,
        }
    }

    fn action_slug(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Devin => "devin",
            Self::GrokBuild => "grok-build",
            Self::ClaudeCode => "claude-code",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HostEnvironment {
    home_path: PathBuf,
    grok_home: PathBuf,
    process_commands: HostProcessCommandSource,
    assistant_cli_paths: HostAssistantCliPathSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HostProcessCommandSource {
    Real,
    Fixed(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HostAssistantCliPathSource {
    Real,
    Fixed(BTreeMap<String, String>),
}

impl HostEnvironment {
    pub fn real(home_path: PathBuf) -> Self {
        let grok_home = default_grok_home(&home_path);
        Self::real_with_grok_home(home_path, grok_home)
    }

    pub fn real_with_grok_home(home_path: PathBuf, grok_home: PathBuf) -> Self {
        Self {
            home_path,
            grok_home,
            process_commands: HostProcessCommandSource::Real,
            assistant_cli_paths: HostAssistantCliPathSource::Real,
        }
    }

    pub fn hermetic(home_path: PathBuf) -> Self {
        let grok_home = default_grok_home(&home_path);
        Self::hermetic_with_grok_home(home_path, grok_home)
    }

    pub fn hermetic_with_grok_home(home_path: PathBuf, grok_home: PathBuf) -> Self {
        Self {
            home_path,
            grok_home,
            process_commands: HostProcessCommandSource::Fixed(Vec::new()),
            assistant_cli_paths: HostAssistantCliPathSource::Fixed(BTreeMap::new()),
        }
    }

    pub fn with_process_commands(mut self, process_commands: Vec<String>) -> Self {
        self.process_commands = HostProcessCommandSource::Fixed(process_commands);
        self
    }

    pub fn with_assistant_cli_paths(mut self, cli_paths: BTreeMap<String, String>) -> Self {
        self.assistant_cli_paths = HostAssistantCliPathSource::Fixed(cli_paths);
        self
    }

    fn home_path(&self) -> &PathBuf {
        &self.home_path
    }

    fn grok_home(&self) -> &PathBuf {
        &self.grok_home
    }

    fn claude_home(&self) -> PathBuf {
        default_claude_home(&self.home_path)
    }

    fn process_commands(&self) -> Vec<String> {
        match &self.process_commands {
            HostProcessCommandSource::Real => current_process_commands(),
            HostProcessCommandSource::Fixed(process_commands) => process_commands.clone(),
        }
    }

    fn assistant_adapters(&self) -> Vec<AssistantAdapterCapability> {
        match (&self.process_commands, &self.assistant_cli_paths) {
            (HostProcessCommandSource::Real, HostAssistantCliPathSource::Real) => {
                adapter_capabilities()
            }
            _ => {
                let process_commands = self.process_commands();
                let cli_paths = match &self.assistant_cli_paths {
                    HostAssistantCliPathSource::Real => BTreeMap::new(),
                    HostAssistantCliPathSource::Fixed(cli_paths) => cli_paths.clone(),
                };
                discover_assistant_adapters_from_sources(&process_commands, &cli_paths)
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct ControlPlaneConfig {
    pub codex_home: PathBuf,
    pub codex_executable: Option<String>,
    pub claude_executable: Option<String>,
    pub store_path: PathBuf,
    pub hook_command: Option<String>,
    pub host_environment: HostEnvironment,
}

#[derive(Clone)]
pub struct ControlPlane {
    config: ControlPlaneConfig,
    store: EventStore,
    mobile_events: MobileEventHub,
    devin_acp_runtime: DevinAcpRuntime,
    zed_acp_runtime: LooperAcpRuntime,
    response_cache: Arc<ControlPlaneResponseCache>,
    prompt_delivery_cache: Arc<PromptDeliveryActionCache>,
    session_mini_reconciler: Arc<SessionMiniProjectionReconciler>,
    text_chunk_cursors: Arc<Mutex<BTreeMap<String, TextChunkCursor>>>,
    text_chunk_message_seq: Arc<AtomicU64>,
}

/// Tracks, per thread, the last mobile `TextChunk` published from a polled (codex/claude-code)
/// session so `publish_mobile_text_chunks_for_minis` can tell message growth (same message,
/// bigger `content`) apart from a brand-new assistant message starting.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TextChunkCursor {
    message_id: String,
    content: String,
    is_final: bool,
}

struct ControlPlaneResponseCache {
    managed_connections: TimedResponseCache<ManagedConnectionsResponse>,
    desktop_menu_snapshot: TimedResponseCache<DesktopSnapshot>,
    acp_client_hosts: TimedResponseCache<AcpClientHostsResponse>,
    devin_desktop_status: TimedResponseCache<DevinDesktopStatus>,
    zed_status: TimedResponseCache<ZedStatus>,
}

struct SessionMiniProjectionReconciler {
    state: Mutex<SessionMiniProjectionReconcileState>,
}

#[derive(Default)]
struct SessionMiniProjectionReconcileState {
    in_flight: bool,
    last_started_at: Option<Instant>,
    last_source_signature: Option<SessionMiniProjectionSourceSignature>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SessionMiniProjectionSourceSignature {
    state_db_path: PathBuf,
    modified_at_ms: i64,
    len: u64,
    transcript_signature: String,
    // In-process ACP runtimes (zed/devin) never touch the codex state DB; their
    // session activity is folded in here so the source-change reconciler wakes for
    // non-codex surfaces too.
    acp_runtime_signature: String,
    // Claude Code writes only to its own project transcripts (~/.claude/projects),
    // never the codex state DB, and its owned hooks fire only at turn boundaries —
    // without this component a claude session's mid-turn growth (and therefore its
    // live text chunks) would sit invisible until Stop.
    claude_transcript_signature: String,
}

struct SessionMiniProjectionReconcilePermit {
    reconciler: Arc<SessionMiniProjectionReconciler>,
}

impl SessionMiniProjectionReconciler {
    fn new() -> Self {
        Self {
            state: Mutex::new(SessionMiniProjectionReconcileState::default()),
        }
    }

    fn try_acquire(
        self: &Arc<Self>,
        now: Instant,
        min_interval: Duration,
    ) -> Option<SessionMiniProjectionReconcilePermit> {
        let mut state = self.state.lock().expect("session mini reconciler lock");
        if state.in_flight {
            return None;
        }
        if state
            .last_started_at
            .is_some_and(|last_started_at| now.duration_since(last_started_at) < min_interval)
        {
            return None;
        }
        state.in_flight = true;
        state.last_started_at = Some(now);
        Some(SessionMiniProjectionReconcilePermit {
            reconciler: self.clone(),
        })
    }

    fn try_acquire_for_source_change(
        self: &Arc<Self>,
        now: Instant,
        min_interval: Duration,
        source_signature: &SessionMiniProjectionSourceSignature,
    ) -> Option<SessionMiniProjectionReconcilePermit> {
        let mut state = self.state.lock().expect("session mini reconciler lock");
        if state.in_flight {
            return None;
        }
        if state.last_source_signature.as_ref() == Some(source_signature) {
            return None;
        }
        if state
            .last_started_at
            .is_some_and(|last_started_at| now.duration_since(last_started_at) < min_interval)
        {
            return None;
        }
        state.in_flight = true;
        state.last_started_at = Some(now);
        Some(SessionMiniProjectionReconcilePermit {
            reconciler: self.clone(),
        })
    }

    fn mark_source_signature(&self, source_signature: SessionMiniProjectionSourceSignature) {
        let mut state = self.state.lock().expect("session mini reconciler lock");
        state.last_source_signature = Some(source_signature);
    }

    fn finish(&self) {
        let mut state = self.state.lock().expect("session mini reconciler lock");
        state.in_flight = false;
    }
}

impl Drop for SessionMiniProjectionReconcilePermit {
    fn drop(&mut self) {
        self.reconciler.finish();
    }
}

impl ControlPlaneResponseCache {
    fn new() -> Self {
        Self {
            managed_connections: TimedResponseCache::new(),
            desktop_menu_snapshot: TimedResponseCache::new(),
            acp_client_hosts: TimedResponseCache::new(),
            devin_desktop_status: TimedResponseCache::new(),
            zed_status: TimedResponseCache::new(),
        }
    }

    fn invalidate_desktop_menu_surfaces(&self) {
        self.managed_connections.invalidate();
        self.desktop_menu_snapshot.invalidate();
        self.acp_client_hosts.invalidate();
        self.devin_desktop_status.invalidate();
        self.zed_status.invalidate();
    }
}

struct TimedResponseCache<T> {
    cached: Mutex<Option<CachedResponse<T>>>,
}

impl<T> TimedResponseCache<T> {
    fn new() -> Self {
        Self {
            cached: Mutex::new(None),
        }
    }

    fn invalidate(&self) {
        if let Ok(mut cached) = self.cached.lock() {
            *cached = None;
        }
    }
}

impl<T: Clone> TimedResponseCache<T> {
    fn get_or_refresh(&self, ttl: Duration, refresh: impl FnOnce() -> Result<T>) -> Result<T> {
        let Ok(mut cached) = self.cached.lock() else {
            return refresh();
        };
        let now = Instant::now();
        if let Some(snapshot) = cached.as_ref()
            && now.duration_since(snapshot.observed_at) < ttl
        {
            return Ok(snapshot.response.clone());
        }

        let response = refresh()?;
        *cached = Some(CachedResponse {
            response: response.clone(),
            observed_at: now,
        });
        Ok(response)
    }

    fn get_or_refresh_infallible(&self, ttl: Duration, refresh: impl FnOnce() -> T) -> T {
        let Ok(mut cached) = self.cached.lock() else {
            return refresh();
        };
        let now = Instant::now();
        if let Some(snapshot) = cached.as_ref()
            && now.duration_since(snapshot.observed_at) < ttl
        {
            return snapshot.response.clone();
        }

        let response = refresh();
        *cached = Some(CachedResponse {
            response: response.clone(),
            observed_at: now,
        });
        response
    }
}

struct CachedResponse<T> {
    response: T,
    observed_at: Instant,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationsResponse {
    pub automations: Vec<AutomationSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadsResponse {
    pub threads: Vec<ThreadRecord>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantAdaptersResponse {
    pub adapters: Vec<AssistantAdapterCapability>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpTargetsResponse {
    pub targets: Vec<AcpTarget>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinDesktopResponse {
    pub status: DevinDesktopStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedResponse {
    pub status: ZedStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeResponse {
    pub bridge: DevinAcpBridgeStatus,
    pub runtime: DevinAcpRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeProbeResponse {
    pub probe: DevinAcpBridgeProbe,
    pub bridge: DevinAcpBridgeStatus,
    pub runtime: DevinAcpRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpInstallResponse {
    pub install: crate::devin::DevinAcpInstallResult,
    pub bridge: DevinAcpBridgeStatus,
    pub runtime: DevinAcpRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionsResponse {
    pub events: Vec<CompactionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexServersResponse {
    pub servers: Vec<CodexServerProcess>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedConnectionsResponse {
    pub connections: Vec<ManagedConnection>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedConnection {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub status: String,
    pub subtitle: Option<String>,
    pub detail: Option<String>,
    pub created_at: Option<String>,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub can_rename: bool,
    pub can_revoke: bool,
    pub action_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalsResponse {
    pub goals: Vec<GoalSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookMutationResponse {
    pub action: String,
    pub removed_handlers: usize,
    pub installed_handlers: usize,
    pub hooks_auto_registration: bool,
    pub status: ControlPlaneStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadDetail {
    pub thread: ThreadRecord,
    pub capabilities: ThreadCapabilities,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopSnapshot {
    pub revision: String,
    pub control_plane: ControlPlaneStatus,
    pub thread_count: usize,
    pub active_thread_count: usize,
    pub archived_thread_count: usize,
    pub threads: Vec<DesktopThread>,
    pub automations: Vec<AutomationSummary>,
    pub automation_runs: Vec<AutomationRunRecord>,
    pub goals: Vec<GoalSummary>,
    pub sync_manifest: SyncManifest,
    pub assistant_adapters: Vec<AssistantAdapterCapability>,
    pub acp_targets: Vec<AcpTarget>,
    pub devin_desktop: DevinDesktopStatus,
    pub devin_session_count: usize,
    pub devin_active_session_count: usize,
    pub devin_session_errors: Vec<DevinSessionDiscoveryError>,
    pub zed: ZedStatus,
    pub grok_build: GrokBuildStatus,
    pub compactions: Vec<CompactionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrokBuildStatus {
    pub hooks: GrokHookStatus,
    pub session_count: usize,
    pub active_session_count: usize,
}

struct SnapshotExternalSessions {
    grok_sessions: Vec<GrokSessionRecord>,
    grok_build: GrokBuildStatus,
    claude_sessions: Vec<ClaudeSessionRecord>,
    devin_discovery: DevinSessionDiscovery,
}

impl SnapshotExternalSessions {
    fn stale() -> Self {
        Self {
            grok_sessions: Vec::new(),
            grok_build: stale_grok_build_status(),
            claude_sessions: Vec::new(),
            devin_discovery: empty_devin_session_discovery(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopThread {
    pub thread_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub transcript_path: Option<String>,
    pub source: Option<String>,
    pub originator: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub git_sha: Option<String>,
    pub git_branch: Option<String>,
    pub cli_version: Option<String>,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub agent_path: Option<String>,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub latest_message_at_ms: Option<i64>,
    pub assistant_preview: Option<String>,
    pub first_user_prompt: Option<String>,
    pub runtime_status: Option<String>,
    pub archived: bool,
    pub goal: Option<ThreadGoalSummary>,
    pub capabilities: ThreadCapabilities,
}

impl ControlPlane {
    pub fn new(config: ControlPlaneConfig) -> Self {
        let store = EventStore::new(config.store_path.clone());
        Self {
            config,
            store,
            mobile_events: MobileEventHub::new(),
            devin_acp_runtime: DevinAcpRuntime::default(),
            zed_acp_runtime: LooperAcpRuntime::new(ZED_CLIENT_ID),
            response_cache: Arc::new(ControlPlaneResponseCache::new()),
            prompt_delivery_cache: Arc::new(PromptDeliveryActionCache::default()),
            session_mini_reconciler: Arc::new(SessionMiniProjectionReconciler::new()),
            text_chunk_cursors: Arc::new(Mutex::new(BTreeMap::new())),
            text_chunk_message_seq: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn store_path(&self) -> &Path {
        &self.config.store_path
    }

    fn home_path(&self) -> &PathBuf {
        self.config.host_environment.home_path()
    }

    fn process_commands(&self) -> Vec<String> {
        self.config.host_environment.process_commands()
    }

    fn assistant_adapters(&self) -> Vec<AssistantAdapterCapability> {
        self.config.host_environment.assistant_adapters()
    }

    fn inspect_control_plane_status(&self) -> ControlPlaneStatus {
        inspect_control_plane_with_process_lines(&self.config.codex_home, &self.process_commands())
    }

    fn inspect_devin_desktop_status(&self) -> DevinDesktopStatus {
        inspect_devin_desktop_with_processes(self.home_path(), &self.process_commands())
    }

    fn claude_sessions(&self, thread_limit: Option<usize>) -> Result<Vec<ClaudeSessionRecord>> {
        let claude_home = self.claude_home();
        let process_commands = self.process_commands();
        match thread_limit {
            Some(limit) => discover_recent_claude_sessions_with_processes(
                &claude_home,
                limit,
                &process_commands,
            ),
            None => discover_claude_sessions_with_processes(&claude_home, &process_commands),
        }
    }

    pub fn mobile_event_hub(&self) -> &MobileEventHub {
        &self.mobile_events
    }

    pub fn devin_acp_runtime(&self) -> &DevinAcpRuntime {
        &self.devin_acp_runtime
    }

    pub fn acp_runtime_for_client(&self, client_id: &str) -> Option<&LooperAcpRuntime> {
        match self.acp_client_host_provider(client_id)? {
            AcpClientHostProvider::Devin => Some(&self.devin_acp_runtime),
            AcpClientHostProvider::Zed => Some(&self.zed_acp_runtime),
        }
    }

    pub fn emit_mobile_event(&self, input: MobileEventInput) {
        let mut event = build_mobile_event(input);
        event.revision = self.latest_cached_mobile_revision();
        self.persist_and_publish_mobile_event(event, None);
    }

    pub fn emit_mobile_session_event(&self, input: MobileEventInput, thread_id: &str) {
        let mut event = build_mobile_event(input);
        let minis = self.cached_session_mini_projection_inputs(thread_id);
        event.revision = minis
            .as_ref()
            .ok()
            .and_then(|records| latest_session_mini_revision(records))
            .or_else(|| self.latest_cached_mobile_revision());
        let minis = minis
            .ok()
            .map(|records| session_mini_projection_inputs_from_records(&records))
            .filter(|minis| !minis.is_empty());
        self.persist_and_publish_mobile_event(event, minis);
        // Hook-driven activity (claude/grok/acp) never touches the codex state DB, so
        // the source-change reconciler stays quiet for it; this throttled spawn is what
        // keeps non-codex surfaces' minis fresh — and creates them for brand-new
        // sessions whose republished cache above was empty.
        self.spawn_mobile_session_mini_projection_reconcile_if_due();
    }

    pub fn emit_mobile_session_event_without_projection(&self, input: MobileEventInput) {
        let mut event = build_mobile_event(input);
        event.revision = self
            .store
            .latest_mobile_session_mini_revision()
            .ok()
            .flatten();
        self.persist_and_publish_mobile_event(event, None);
    }

    pub fn publish_mobile_session_event_without_persisting(&self, input: MobileEventInput) {
        let mut event = build_mobile_event(input);
        event.revision = self
            .store
            .latest_mobile_session_mini_revision()
            .ok()
            .flatten();
        self.mobile_events.publish_ephemeral(event);
    }

    pub fn publish_mobile_text_chunk(&self, input: MobileTextChunkInput) {
        if input.content.is_empty() || input.content.len() > SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES {
            return;
        }
        let seq = self
            .store
            .latest_mobile_state_event_seq()
            .unwrap_or_default();
        let message_id = input
            .message_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("{}:latest-assistant", input.thread_id));
        self.mobile_events.publish_text_chunk(MobileTextChunk {
            seq,
            thread_id: input.thread_id,
            message_id,
            content: input.content,
            is_final: input.is_final,
            server_time: mobile_event_now(),
        });
    }

    /// Publishes mobile `TextChunk` frames for polled (codex/claude-code) sessions by diffing
    /// each mini's `assistantPreview` against the last chunk we sent for that thread.
    ///
    /// ACP-hosted sessions (Devin, Zed) already publish chunks the moment their host observes
    /// a turn (see `observe_acp_client_host_session_response`), so they carry no `assistantSurface`
    /// listed in `TEXT_CHUNK_POLLED_ASSISTANT_SURFACES` and are skipped here to avoid double
    /// publishing. Codex and Claude Code have no equivalent push hook: their latest-assistant-text
    /// is only ever recomputed by re-reading the rollout/transcript file on this reconcile pass
    /// (see `reconcile_mobile_session_mini_projection_with_options`), so this diff is the only
    /// place their live-typing chunks originate.
    fn publish_mobile_text_chunks_for_minis(&self, minis: &[MobileSessionMiniProjectionInput]) {
        let mut cursors = self
            .text_chunk_cursors
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut live_session_ids = BTreeSet::new();
        for mini in minis {
            if !TEXT_CHUNK_POLLED_ASSISTANT_SURFACES.contains(&mini.assistant_surface.as_str()) {
                continue;
            }
            let Some(content) = mini
                .body_json
                .get("assistantPreview")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            live_session_ids.insert(mini.session_id.clone());
            let is_final = mini
                .body_json
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status != session_fsm::ACTIVE_STATUS);
            self.publish_mobile_text_chunk_growth(
                &mut cursors,
                &mini.session_id,
                content,
                is_final,
            );
        }
        cursors.retain(|session_id, _| live_session_ids.contains(session_id));
    }

    /// Publishes one chunk for `session_id` if `content`/`is_final` moved on from the cached
    /// cursor, minting a fresh `message_id` when the previous message had already been sealed
    /// (`is_final`) and new content shows up — i.e. a new assistant turn started.
    fn publish_mobile_text_chunk_growth(
        &self,
        cursors: &mut BTreeMap<String, TextChunkCursor>,
        session_id: &str,
        content: &str,
        is_final: bool,
    ) {
        // publish_mobile_text_chunk silently drops oversized content; recording a
        // cursor for a chunk that never went out would dedupe away every later
        // update of the same message, so bail before touching the cursor.
        if content.is_empty() || content.len() > SESSION_TEXT_CHUNK_CONTENT_MAX_BYTES {
            return;
        }
        let existing = cursors.get(session_id).cloned();
        let message_id = match &existing {
            Some(cursor) if cursor.is_final && cursor.content != content => {
                self.next_text_chunk_message_id(session_id)
            }
            Some(cursor) => cursor.message_id.clone(),
            None => self.next_text_chunk_message_id(session_id),
        };
        let already_published = existing.as_ref().is_some_and(|cursor| {
            cursor.message_id == message_id
                && cursor.content == content
                && cursor.is_final == is_final
        });
        if already_published {
            return;
        }
        self.publish_mobile_text_chunk(MobileTextChunkInput {
            thread_id: session_id.to_owned(),
            message_id: Some(message_id.clone()),
            content: content.to_owned(),
            is_final,
        });
        cursors.insert(
            session_id.to_owned(),
            TextChunkCursor {
                message_id,
                content: content.to_owned(),
                is_final,
            },
        );
    }

    fn next_text_chunk_message_id(&self, session_id: &str) -> String {
        let sequence = self.text_chunk_message_seq.fetch_add(1, Ordering::Relaxed);
        format!("{session_id}:msg-{sequence}")
    }

    pub fn emit_mobile_session_event_with_cached_minis(
        &self,
        input: MobileEventInput,
        minis: Vec<MobileSessionMiniProjectionInput>,
    ) {
        let mut event = build_mobile_event(input);
        event.revision = self
            .store
            .latest_mobile_session_mini_revision()
            .ok()
            .flatten();
        self.persist_and_publish_mobile_event(event, Some(minis));
    }

    pub fn emit_mobile_all_sessions_event(&self, input: MobileEventInput) {
        let mut event = build_mobile_event(input);
        event.revision = self.latest_cached_mobile_revision();
        self.persist_and_publish_mobile_event(event, None);
        self.spawn_mobile_session_mini_projection_reconcile_if_due();
    }

    pub fn reconcile_mobile_session_mini_projection(&self) -> Result<bool> {
        self.reconcile_mobile_session_mini_projection_with_options(false, None)
    }

    /// Like `reconcile_mobile_session_mini_projection`, but tags the emitted event with
    /// `detail` when a write happens, instead of the generic snapshot-revision-changed detail.
    /// Still skips the write when the stored projection's content already matches (see
    /// `stored_mobile_session_mini_projection_matches`) — callers that need an unconditional
    /// write regardless of content should use `force_reconcile_mobile_session_mini_projection`.
    pub fn reconcile_mobile_session_mini_projection_with_detail(
        &self,
        detail: &str,
    ) -> Result<bool> {
        self.reconcile_mobile_session_mini_projection_with_options(false, Some(detail))
    }

    pub fn force_reconcile_mobile_session_mini_projection(&self, detail: &str) -> Result<bool> {
        self.reconcile_mobile_session_mini_projection_with_options(true, Some(detail))
    }

    fn reconcile_mobile_session_mini_projection_with_options(
        &self,
        force: bool,
        detail: Option<&str>,
    ) -> Result<bool> {
        let snapshot = self.desktop_snapshot_with_limits(
            None,
            DESKTOP_MENU_COMPACTION_LIMIT,
            DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT,
            SnapshotInspectionMode::Live,
        )?;
        let revision = snapshot.revision.trim().to_owned();
        if revision.trim().is_empty() {
            return Ok(false);
        }
        let session_state = self.mobile_session_service().state()?;
        prime_delivery_action_cache(self, &snapshot, &session_state);
        let queued_prompt_counts = self.mobile_session_service().queued_prompt_counts()?;
        let latest_seq = self.store.latest_mobile_state_event_seq()?;
        let minis = session_mini_projection_inputs(
            &snapshot,
            &session_state,
            &queued_prompt_counts,
            latest_seq,
            &revision,
        );
        // Diffs assistant-preview growth against our own per-thread cursor and publishes chunks
        // as needed. This must run before the "stored projection unchanged" early return below:
        // that check is about the mini-projection *event*, not about whether we've already sent
        // a TextChunk for this exact content, and the two can disagree (e.g. a chunk already
        // covers this content from an earlier reconcile pass in the same second).
        self.publish_mobile_text_chunks_for_minis(&minis);
        let stored_revision = self
            .store
            .latest_mobile_session_mini_revision()
            .ok()
            .flatten();
        if !force
            && stored_revision.as_deref() == Some(revision.as_str())
            && self.stored_mobile_session_mini_projection_matches(&minis)?
        {
            return Ok(false);
        }
        let event = match detail {
            Some(detail) => MobileEvent {
                event_type: MobileEventKind::SessionChanged,
                thread_id: None,
                prompt_id: None,
                detail: Some(detail.to_owned()),
                server_time: crate::mobile::events::mobile_event_now(),
                revision: Some(revision),
            },
            None => snapshot_revision_changed_event(revision),
        };
        let record = self
            .store
            .record_mobile_event_replacing_session_minis(&event, minis)?;
        self.mobile_events.publish_persisted(record);
        Ok(true)
    }

    /// Compares candidate projection bodies against what is already stored, by content rather
    /// than just by which (session_id, assistant_surface) keys are present. A key-set-only
    /// check cannot detect a session whose fields changed in place (e.g. `isArchived`,
    /// `lifecycle`, `effectiveMode`) without also gaining or losing a session, which is why a
    /// `force` flag previously existed as a workaround for callers that knew content-only
    /// changes needed to bypass this check.
    fn stored_mobile_session_mini_projection_matches(
        &self,
        minis: &[MobileSessionMiniProjectionInput],
    ) -> Result<bool> {
        let stored = self.store.mobile_session_minis()?;
        if stored.len() != minis.len() {
            return Ok(false);
        }

        let expected_fingerprints = minis
            .iter()
            .map(|mini| {
                (
                    (mini.session_id.as_str(), mini.assistant_surface.as_str()),
                    mobile_session_mini_content_fingerprint(
                        &mini.body_json,
                        &mini.session_id,
                        &mini.assistant_surface,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let stored_fingerprints = stored
            .iter()
            .filter_map(|record| {
                let body_json = serde_json::from_str::<Value>(&record.body_json).ok()?;
                Some((
                    (
                        record.session_id.as_str(),
                        record.assistant_surface.as_str(),
                    ),
                    mobile_session_mini_content_fingerprint(
                        &body_json,
                        &record.session_id,
                        &record.assistant_surface,
                    ),
                ))
            })
            .collect::<BTreeMap<_, _>>();
        Ok(stored_fingerprints == expected_fingerprints)
    }

    pub fn spawn_mobile_session_mini_projection_reconcile_if_due(&self) {
        let Some(permit) = self
            .session_mini_reconciler
            .try_acquire(Instant::now(), SESSION_MINI_PROJECTION_RECONCILE_INTERVAL)
        else {
            return;
        };
        let control_plane = self.clone();
        spawn_blocking_from_any_thread(move || {
            let _permit = permit;
            if let Err(error) = control_plane.reconcile_mobile_session_mini_projection() {
                eprintln!("mobile session mini reconcile failed: {error}");
            }
        });
    }

    pub fn spawn_mobile_session_mini_projection_reconcile_if_source_changed(&self) {
        let Some(source_signature) = self.mobile_session_mini_projection_source_signature() else {
            return;
        };
        let Some(permit) = self.session_mini_reconciler.try_acquire_for_source_change(
            Instant::now(),
            SESSION_MINI_PROJECTION_RECONCILE_INTERVAL,
            &source_signature,
        ) else {
            return;
        };
        let control_plane = self.clone();
        spawn_blocking_from_any_thread(move || {
            let _permit = permit;
            match control_plane.reconcile_mobile_session_mini_projection() {
                Ok(_) => control_plane
                    .session_mini_reconciler
                    .mark_source_signature(source_signature),
                Err(error) => eprintln!("mobile session mini reconcile failed: {error}"),
            }
        });
    }

    fn mobile_session_mini_projection_source_signature(
        &self,
    ) -> Option<SessionMiniProjectionSourceSignature> {
        let state_db_path = discover_sources(&self.config.codex_home).state_db?;
        let metadata = fs::metadata(&state_db_path).ok()?;
        let modified_at_ms = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
            .unwrap_or_default();
        Some(SessionMiniProjectionSourceSignature {
            state_db_path,
            modified_at_ms,
            len: metadata.len(),
            transcript_signature: self.session_mini_transcript_source_signature(),
            acp_runtime_signature: self.acp_runtime_source_signature(),
            claude_transcript_signature: claude_transcript_source_signature(
                &self.claude_home(),
                DESKTOP_SNAPSHOT_THREAD_LIMIT,
            ),
        })
    }

    fn session_mini_transcript_source_signature(&self) -> String {
        read_thread_revision_state(&self.config.codex_home, DESKTOP_SNAPSHOT_THREAD_LIMIT)
            .map(|state| transcript_source_signature(&state.threads))
            .unwrap_or_default()
    }

    fn acp_runtime_source_signature(&self) -> String {
        let zed_status = self.zed_acp_runtime.status();
        let devin_status = self.devin_acp_runtime.status();
        let mut parts = Vec::with_capacity(zed_status.sessions.len() + devin_status.sessions.len());
        parts.extend(zed_status.sessions.iter().map(|session| {
            format!(
                "zed:{}:{}:{}",
                session.public_thread_id, session.updated_at_ms, session.cancelled
            )
        }));
        parts.extend(devin_status.sessions.iter().map(|session| {
            format!(
                "devin:{}:{}:{}",
                session.session_id, session.updated_at_ms, session.cancelled
            )
        }));
        parts.sort();
        parts.join("|")
    }

    fn persist_and_publish_mobile_event(
        &self,
        event: MobileEvent,
        minis: Option<Vec<MobileSessionMiniProjectionInput>>,
    ) {
        let result = match minis {
            Some(minis) => self
                .store
                .record_mobile_event_with_session_minis(&event, minis),
            None => self.store.record_mobile_event(&event),
        };
        match result {
            Ok(record) => self.mobile_events.publish_persisted(record),
            Err(error) => {
                eprintln!("mobile event persistence failed: {error}");
                self.mobile_events.publish_ephemeral(event);
            }
        }
    }

    fn cached_session_mini_projection_inputs(
        &self,
        thread_id: &str,
    ) -> Result<Vec<crate::events::MobileSessionMiniRecord>> {
        self.store.mobile_session_minis_for_session(thread_id)
    }

    fn latest_cached_mobile_revision(&self) -> Option<String> {
        self.store
            .latest_mobile_session_mini_revision()
            .ok()
            .flatten()
            .or_else(|| {
                Some(mobile_state_seq_revision(
                    self.latest_mobile_state_event_seq(),
                ))
            })
    }

    /// The current mobile-state revision string, for callers that need one unconditionally.
    ///
    /// Prefers the minis projection's own revision; when there is none (no projection yet, or
    /// the store read failed) falls back to a synthetic `mobile-state:seq-{n}` revision derived
    /// from the latest state-event seq (0 if that read also fails). This was previously
    /// hand-copied in four places (http/mobile_state.rs, grpc/service.rs,
    /// mobile/realtime_ack.rs, control_plane.rs) — this is the single shared implementation;
    /// the others now delegate here.
    pub fn current_mobile_state_revision(&self) -> String {
        self.latest_cached_mobile_revision().unwrap_or_default()
    }

    fn latest_mobile_state_event_seq(&self) -> i64 {
        self.store
            .latest_mobile_state_event_seq()
            .unwrap_or_default()
    }

    pub fn mobile_snapshot_revision(&self) -> Result<String> {
        let state = read_thread_revision_state(&self.config.codex_home, DESKTOP_MENU_THREAD_LIMIT)?;
        let session_state = self.mobile_session_service().state()?;
        let mut thread_signature = state
            .threads
            .iter()
            .map(|thread| {
                let transcript_modified_at_ms = thread
                    .transcript_path
                    .as_deref()
                    .and_then(|path| metadata_modified_at_ms(Path::new(path)))
                    .unwrap_or_default();
                format!(
                    "{}:{}:{}:{}",
                    thread.thread_id,
                    thread.updated_at_ms.unwrap_or_default(),
                    transcript_modified_at_ms,
                    thread.archived
                )
            })
            .collect::<Vec<_>>();
        thread_signature.sort();
        let grok_sessions = discover_grok_sessions(self.grok_home()).unwrap_or_default();
        let mut grok_signature = grok_sessions
            .iter()
            .take(DESKTOP_MENU_THREAD_LIMIT)
            .map(|session| {
                format!(
                    "{}:{}:{}",
                    session.session_id, session.updated_at, session.running
                )
            })
            .collect::<Vec<_>>();
        grok_signature.sort();
        let devin_discovery =
            self.recent_devin_sessions_for_snapshot(ACP_CLIENT_HOST_SESSION_LIMIT);
        let mut devin_signature = devin_discovery
            .sessions
            .iter()
            .map(|session| {
                format!(
                    "{}:{}:{}:{}",
                    session.session_id,
                    session.updated_at_ms.unwrap_or_default(),
                    session.archived,
                    session.status
                )
            })
            .collect::<Vec<_>>();
        let devin_acp_runtime = self.devin_acp_runtime.status();
        devin_signature.extend(devin_acp_runtime.sessions.iter().map(|session| {
            format!(
                "runtime:{}:{}:{}:{}",
                session.session_id,
                session.updated_at_ms,
                session.cancelled,
                session
                    .latest_assistant_message
                    .as_deref()
                    .unwrap_or_default()
            )
        }));
        devin_signature.sort();
        let zed_acp_runtime = self.zed_acp_runtime.status();
        let mut zed_signature = zed_acp_runtime
            .sessions
            .iter()
            .map(|session| {
                format!(
                    "{}:{}:{}:{}:{}",
                    session.public_thread_id,
                    session.updated_at_ms,
                    session.cancelled,
                    session.cwd.as_deref().unwrap_or_default(),
                    session
                        .latest_assistant_message
                        .as_deref()
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        zed_signature.sort();
        let codex_active_thread_count = state.active_thread_count;
        let codex_archived_thread_count = state
            .total_thread_count
            .saturating_sub(codex_active_thread_count);
        let grok_active_thread_count = grok_sessions
            .iter()
            .filter(|session| session.running)
            .count();
        let devin_active_thread_count = devin_discovery.active_count;
        let claude_sessions = self
            .claude_sessions(Some(DESKTOP_MENU_THREAD_LIMIT))
            .unwrap_or_default();
        let mut claude_signature = claude_sessions
            .iter()
            .take(DESKTOP_MENU_THREAD_LIMIT)
            .map(|session| {
                format!(
                    "{}:{}:{}",
                    session.thread_id,
                    session.updated_at_ms.unwrap_or_default(),
                    session.running
                )
            })
            .collect::<Vec<_>>();
        claude_signature.sort();
        let claude_active_thread_count = claude_sessions
            .iter()
            .filter(|session| session.running)
            .count();
        let devin_archived_thread_count = devin_discovery.archived_count;
        let mut known_thread_ids = state
            .threads
            .iter()
            .map(|thread| thread.thread_id.clone())
            .collect::<BTreeSet<_>>();
        known_thread_ids.extend(
            grok_sessions
                .iter()
                .map(|session| session.session_id.clone()),
        );
        known_thread_ids.extend(
            claude_sessions
                .iter()
                .map(|session| session.thread_id.clone()),
        );
        known_thread_ids.extend(
            devin_discovery
                .sessions
                .iter()
                .map(|session| session.thread_id.clone()),
        );
        known_thread_ids.extend(
            devin_acp_runtime
                .sessions
                .iter()
                .map(|session| session.public_thread_id.clone()),
        );
        known_thread_ids.extend(
            zed_acp_runtime
                .sessions
                .iter()
                .map(|session| session.public_thread_id.clone()),
        );
        let mut goal_signature = read_goals(&self.config.codex_home, &known_thread_ids)?
            .into_iter()
            .map(|goal| {
                format!(
                    "{}:{}:{}:{}:{}",
                    goal.id,
                    goal.target_thread_id.unwrap_or_default(),
                    goal.running,
                    goal.updated_at_ms.unwrap_or_default(),
                    goal.content_hash
                )
            })
            .collect::<Vec<_>>();
        goal_signature.sort();
        let mut automation_signature = read_automations(&self.config.codex_home)?
            .into_iter()
            .map(|automation| automation.to_summary(&known_thread_ids))
            .map(|automation| {
                format!(
                    "{}:{}:{:?}:{}:{}",
                    automation.id,
                    automation.target_thread_id.unwrap_or_default(),
                    automation.status,
                    automation.rrule,
                    automation.control_plane_covered
                )
            })
            .collect::<Vec<_>>();
        automation_signature.sort();
        let queued_prompt_count = session_state
            .sessions
            .values()
            .filter(|session| !session.deleted)
            .count();
        let mobile_state_revision = mobile_session_state_revision(&session_state);
        Ok(format!(
            "threads={}:grok={}:devin={}:zed={}:claude={}:goals={}:automations={}:active={}:archived={}:overrides={}:surface={}:mobile-state={}",
            thread_signature.join("|"),
            grok_signature.join("|"),
            devin_signature.join("|"),
            zed_signature.join("|"),
            claude_signature.join("|"),
            goal_signature.join("|"),
            automation_signature.join("|"),
            codex_active_thread_count
                + grok_active_thread_count
                + devin_active_thread_count
                + devin_acp_runtime.sessions.len()
                + active_zed_acp_runtime_session_count(&zed_acp_runtime.sessions)
                + claude_active_thread_count,
            codex_archived_thread_count + devin_archived_thread_count,
            queued_prompt_count,
            session_state.assistant_surface,
            mobile_state_revision
        ))
    }

    pub fn codex_home(&self) -> &PathBuf {
        &self.config.codex_home
    }

    pub fn codex_executable(&self) -> Option<&str> {
        self.config.codex_executable.as_deref()
    }

    pub fn claude_executable(&self) -> Option<&str> {
        self.config.claude_executable.as_deref()
    }

    pub fn session_content_slice(
        &self,
        request: SessionContentSliceRequest<'_>,
    ) -> Result<SessionContentSliceResponse, ContentSliceError> {
        session_content_slice(&self.config.codex_home, request)
    }

    pub fn grok_home(&self) -> &PathBuf {
        self.config.host_environment.grok_home()
    }

    pub fn claude_home(&self) -> PathBuf {
        self.config.host_environment.claude_home()
    }

    pub fn store(&self) -> &EventStore {
        &self.store
    }

    pub fn prompt_delivery_action_cache(&self) -> &PromptDeliveryActionCache {
        &self.prompt_delivery_cache
    }

    pub fn mobile_auth_service(&self) -> MobileAuthService {
        MobileAuthService::new(self.config.store_path.clone())
    }

    pub fn mobile_push_service(&self) -> MobilePushService {
        MobilePushService::new(self.config.store_path.clone())
    }

    pub fn mobile_session_service(&self) -> MobileSessionService {
        MobileSessionService::new(self.config.store_path.clone())
    }

    pub fn telegram_service(&self) -> TelegramService {
        TelegramService::new(self.config.store_path.clone())
    }

    fn mobile_connections(&self) -> Result<Vec<ManagedConnection>> {
        Ok(self
            .mobile_auth_service()
            .managed_connections()?
            .into_iter()
            .map(|connection| ManagedConnection {
                id: connection.id,
                kind: MOBILE_CONNECTION_KIND.to_owned(),
                label: connection.label,
                status: connection.status,
                subtitle: connection.passkey_label,
                detail: connection
                    .passkey_credential_id
                    .map(|credential_id| format!("Passkey {credential_id}")),
                created_at: Some(connection.created_at),
                last_used_at: connection.last_used_at,
                revoked_at: connection.revoked_at,
                can_rename: connection.can_rename,
                can_revoke: connection.can_revoke,
                action_hint: None,
            })
            .collect())
    }

    fn devin_desktop_connections(&self) -> Vec<ManagedConnection> {
        let status = self.cached_devin_desktop_status();
        let hook_status = inspect_devin_hooks(self.home_path());
        let mut connections = status
            .installations
            .iter()
            .filter(|installation| devin_installation_should_render(installation))
            .map(|installation| devin_desktop_connection(installation, &status))
            .collect::<Vec<_>>();
        connections.push(devin_hook_connection(&hook_status));
        connections
    }

    fn grok_build_connections(&self) -> Vec<ManagedConnection> {
        let hook_status = inspect_grok_hooks(self.grok_home());
        let mut connections = grok_build_cli_connections_from_adapters(&self.assistant_adapters());
        connections.push(grok_hook_connection(&hook_status));
        connections
    }

    fn claude_code_connections(&self) -> Vec<ManagedConnection> {
        vec![claude_hook_connection(&inspect_claude_hooks(
            &self.claude_home(),
        ))]
    }

    fn zed_connections(&self) -> Vec<ManagedConnection> {
        let status = self.cached_zed_status();
        if !zed_connection_should_render(&status) {
            return Vec::new();
        }
        vec![zed_acp_connection(&status)]
    }

    pub fn status(&self) -> ControlPlaneStatus {
        self.inspect_control_plane_status()
    }

    fn snapshot_status(&self, inspection_mode: SnapshotInspectionMode) -> ControlPlaneStatus {
        if inspection_mode.includes_diagnostic_details() {
            return self.status();
        }

        ControlPlaneStatus {
            hooks: inspect_hooks(&self.config.codex_home),
            app_server: None,
            codex_servers: Vec::new(),
            source: source_status(&self.config.codex_home),
        }
    }

    fn cached_devin_desktop_status(&self) -> DevinDesktopStatus {
        self.response_cache
            .devin_desktop_status
            .get_or_refresh_infallible(DESKTOP_MENU_INSPECTION_CACHE_TTL, || {
                self.inspect_devin_desktop_status()
            })
    }

    fn cached_zed_status(&self) -> ZedStatus {
        self.response_cache
            .zed_status
            .get_or_refresh_infallible(DESKTOP_MENU_INSPECTION_CACHE_TTL, || {
                self.inspect_zed_status()
            })
    }

    fn inspect_zed_status(&self) -> ZedStatus {
        inspect_zed_for_home_with_processes(self.home_path(), &self.process_commands())
    }

    pub fn codex_servers_response(&self) -> CodexServersResponse {
        CodexServersResponse {
            servers: self.status().codex_servers,
        }
    }

    pub fn managed_connections_response(&self) -> Result<ManagedConnectionsResponse> {
        self.response_cache
            .managed_connections
            .get_or_refresh(DESKTOP_MENU_RESPONSE_CACHE_TTL, || {
                self.managed_connections_response_uncached()
            })
    }

    fn managed_connections_response_uncached(&self) -> Result<ManagedConnectionsResponse> {
        let status = self.status();
        let mut connections = self.mobile_connections()?;
        connections.extend(self.devin_desktop_connections());
        connections.extend(self.grok_build_connections());
        connections.extend(self.claude_code_connections());
        connections.extend(self.zed_connections());
        connections.push(hook_connection(&status));
        connections.extend(status.codex_servers.iter().map(codex_server_connection));
        Ok(ManagedConnectionsResponse { connections })
    }

    pub fn rename_mobile_connection(
        &self,
        connection_id: &str,
        label: &str,
    ) -> Result<ManagedConnectionsResponse> {
        self.mobile_auth_service()
            .rename_mobile_connection(connection_id, label)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        self.managed_connections_response_uncached()
    }

    pub fn revoke_mobile_connection(
        &self,
        connection_id: &str,
    ) -> Result<ManagedConnectionsResponse> {
        self.mobile_auth_service()
            .revoke_mobile_connection(connection_id)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        self.managed_connections_response_uncached()
    }

    pub fn threads(&self) -> Result<Vec<ThreadRecord>> {
        let state = read_state(&self.config.codex_home)?;
        let mut threads = state.threads;
        threads.extend(
            discover_devin_sessions_without_previews(self.home_path())
                .unwrap_or_default()
                .sessions
                .iter()
                .map(devin_session_to_thread_record),
        );
        threads.extend(
            discover_grok_sessions(self.grok_home())
                .unwrap_or_default()
                .iter()
                .map(grok_session_to_desktop_thread)
                .map(|thread| desktop_thread_to_thread_record(&thread)),
        );
        threads.extend(
            self.claude_sessions(None)
                .unwrap_or_default()
                .iter()
                .map(claude_session_to_desktop_thread)
                .map(|thread| desktop_thread_to_thread_record(&thread)),
        );
        threads.sort_by(|left, right| {
            right
                .updated_at_ms
                .unwrap_or_default()
                .cmp(&left.updated_at_ms.unwrap_or_default())
                .then_with(|| left.thread_id.cmp(&right.thread_id))
        });
        Ok(threads)
    }

    pub fn threads_response(&self) -> Result<ThreadsResponse> {
        Ok(ThreadsResponse {
            threads: self.threads()?,
        })
    }

    pub fn assistant_adapters_response(&self) -> AssistantAdaptersResponse {
        AssistantAdaptersResponse {
            adapters: self.assistant_adapters(),
        }
    }

    pub fn acp_targets_response(&self) -> AcpTargetsResponse {
        AcpTargetsResponse {
            targets: self.acp_targets(),
        }
    }

    pub fn devin_desktop_response(&self) -> DevinDesktopResponse {
        DevinDesktopResponse {
            status: self.inspect_devin_desktop_status(),
        }
    }

    pub fn zed_response(&self) -> ZedResponse {
        ZedResponse {
            status: self.inspect_zed_status(),
        }
    }

    fn acp_targets(&self) -> Vec<AcpTarget> {
        let devin_status = self.inspect_devin_desktop_status();
        let zed_status = self.inspect_zed_status();
        let mut targets = devin_acp_targets(&devin_status);
        targets.extend(zed_acp_targets(&zed_status));
        targets.sort_by(|left, right| left.id.cmp(&right.id));
        targets
    }

    pub fn compactions(&self) -> Result<Vec<CompactionEvent>> {
        read_compaction_events(&self.config.codex_home)
    }

    pub fn compactions_response(&self) -> Result<CompactionsResponse> {
        Ok(CompactionsResponse {
            events: self.compactions()?,
        })
    }

    pub fn unregister_hooks(&self) -> Result<HookMutationResponse> {
        let removed_handlers = unregister_owned_hooks(&self.config.codex_home)?
            + unregister_owned_devin_hooks(self.home_path())?
            + unregister_owned_grok_hooks(self.grok_home())?
            + unregister_owned_claude_hooks(&self.claude_home())?;
        let settings = self.store.set_hooks_auto_registration(false)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(HookMutationResponse {
            action: "unregister-hooks".to_owned(),
            removed_handlers,
            installed_handlers: 0,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn register_hooks(&self) -> Result<HookMutationResponse> {
        let hook_command = self
            .config
            .hook_command
            .as_deref()
            .unwrap_or("agent-control-plane --hook --managed-by looper");
        let codex_change = register_owned_hooks(&self.config.codex_home, hook_command)?;
        let devin_change = register_owned_devin_hooks(self.home_path(), hook_command)?;
        let grok_change = register_owned_grok_hooks(self.grok_home(), hook_command)?;
        let claude_change = register_owned_claude_hooks(&self.claude_home(), hook_command)?;
        let settings = self.store.set_hooks_auto_registration(true)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(HookMutationResponse {
            action: "register-hooks".to_owned(),
            removed_handlers: codex_change.removed_handlers
                + devin_change.removed_handlers
                + grok_change.removed_handlers
                + claude_change.removed_handlers,
            installed_handlers: codex_change.installed_handlers
                + devin_change.installed_handlers
                + grok_change.installed_handlers
                + claude_change.installed_handlers,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn register_hooks_for_target(
        &self,
        target: HookMutationTarget,
    ) -> Result<HookMutationResponse> {
        let hook_command = self
            .config
            .hook_command
            .as_deref()
            .unwrap_or("agent-control-plane --hook --managed-by looper");
        let (removed_handlers, installed_handlers) = match target {
            HookMutationTarget::Codex => {
                let change = register_owned_hooks(&self.config.codex_home, hook_command)?;
                (change.removed_handlers, change.installed_handlers)
            }
            HookMutationTarget::Devin => {
                let change = register_owned_devin_hooks(self.home_path(), hook_command)?;
                (change.removed_handlers, change.installed_handlers)
            }
            HookMutationTarget::GrokBuild => {
                let change = register_owned_grok_hooks(self.grok_home(), hook_command)?;
                (change.removed_handlers, change.installed_handlers)
            }
            HookMutationTarget::ClaudeCode => {
                let change = register_owned_claude_hooks(&self.claude_home(), hook_command)?;
                (change.removed_handlers, change.installed_handlers)
            }
        };
        let settings = self.store.set_hooks_auto_registration(true)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(HookMutationResponse {
            action: format!("register-{}-hooks", target.action_slug()),
            removed_handlers,
            installed_handlers,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn unregister_hooks_for_target(
        &self,
        target: HookMutationTarget,
    ) -> Result<HookMutationResponse> {
        let removed_handlers = self.unregister_owned_hooks_for_target(target)?;
        let settings = self.store.set_hooks_auto_registration(false)?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(HookMutationResponse {
            action: format!("unregister-{}-hooks", target.action_slug()),
            removed_handlers,
            installed_handlers: 0,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn unregister_live_hooks(&self) -> Result<HookMutationResponse> {
        let removed_handlers = unregister_owned_hooks(&self.config.codex_home)?
            + unregister_owned_devin_hooks(self.home_path())?
            + unregister_owned_grok_hooks(self.grok_home())?
            + unregister_owned_claude_hooks(&self.claude_home())?;
        let settings = self.store.service_settings()?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(HookMutationResponse {
            action: "unregister-live-hooks".to_owned(),
            removed_handlers,
            installed_handlers: 0,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn unregister_live_hooks_for_target(
        &self,
        target: HookMutationTarget,
    ) -> Result<HookMutationResponse> {
        let removed_handlers = self.unregister_owned_hooks_for_target(target)?;
        let settings = self.store.service_settings()?;
        self.response_cache.invalidate_desktop_menu_surfaces();
        Ok(HookMutationResponse {
            action: format!("unregister-live-{}-hooks", target.action_slug()),
            removed_handlers,
            installed_handlers: 0,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    fn unregister_owned_hooks_for_target(&self, target: HookMutationTarget) -> Result<usize> {
        match target {
            HookMutationTarget::Codex => unregister_owned_hooks(&self.config.codex_home),
            HookMutationTarget::Devin => unregister_owned_devin_hooks(self.home_path()),
            HookMutationTarget::GrokBuild => unregister_owned_grok_hooks(self.grok_home()),
            HookMutationTarget::ClaudeCode => unregister_owned_claude_hooks(&self.claude_home()),
        }
    }

    pub fn thread_detail(&self, thread_id: &str) -> Result<Option<ThreadDetail>> {
        let Some(thread) = self
            .threads()?
            .into_iter()
            .find(|thread| thread.thread_id == thread_id)
        else {
            return Ok(None);
        };
        Ok(Some(ThreadDetail {
            capabilities: self.capabilities(thread_id)?,
            thread,
        }))
    }

    pub fn capabilities(&self, thread_id: &str) -> Result<ThreadCapabilities> {
        let state = read_state(&self.config.codex_home)?;
        if state
            .threads
            .iter()
            .any(|thread| thread.thread_id == thread_id)
        {
            return Ok(capabilities_for_state_thread(&state, thread_id));
        }

        if let Some(session) = discover_devin_sessions_without_previews(self.home_path())
            .unwrap_or_default()
            .sessions
            .into_iter()
            .find(|session| session.thread_id == thread_id || session.session_id == thread_id)
        {
            return Ok(devin_session_capabilities(&session));
        }

        if let Some(session) =
            self.devin_acp_runtime
                .status()
                .sessions
                .into_iter()
                .find(|session| {
                    session.public_thread_id == thread_id || session.session_id == thread_id
                })
        {
            return Ok(devin_acp_runtime_session_capabilities(&session));
        }

        if let Some(session) = self
            .zed_acp_runtime
            .status()
            .sessions
            .into_iter()
            .find(|session| {
                session.public_thread_id == thread_id || session.session_id == thread_id
            })
        {
            return Ok(zed_acp_runtime_session_capabilities(&session));
        }

        if let Some(session) = discover_grok_sessions(self.grok_home())
            .unwrap_or_default()
            .into_iter()
            .find(|session| session.session_id == thread_id)
        {
            return Ok(grok_session_to_desktop_thread(&session).capabilities);
        }

        if let Some(session) = self
            .claude_sessions(None)
            .unwrap_or_default()
            .into_iter()
            .find(|session| session.thread_id == thread_id || session.session_id == thread_id)
        {
            return Ok(claude_session_to_desktop_thread(&session).capabilities);
        }

        Ok(capabilities_for_state_thread(&state, thread_id))
    }

    pub fn automations(&self) -> Result<Vec<AutomationSummary>> {
        let known_thread_ids = self.known_desktop_thread_ids()?;
        Ok(read_automations(&self.config.codex_home)?
            .into_iter()
            .map(|automation| automation.to_summary(&known_thread_ids))
            .collect())
    }

    pub fn automations_response(&self) -> Result<AutomationsResponse> {
        Ok(AutomationsResponse {
            automations: self.automations()?,
        })
    }

    pub fn goals(&self) -> Result<Vec<GoalSummary>> {
        let known_thread_ids = self.known_desktop_thread_ids()?;
        read_goals(&self.config.codex_home, &known_thread_ids)
    }

    pub fn goals_response(&self) -> Result<GoalsResponse> {
        Ok(GoalsResponse {
            goals: self.goals()?,
        })
    }

    pub fn sync_manifest(&self) -> Result<SyncManifest> {
        let state = read_state(&self.config.codex_home)?;
        let threads = state.threads.clone();
        let capabilities = self.capabilities_by_thread(&state)?;
        let known_thread_ids = self.known_desktop_thread_ids()?;
        let goals = read_goals(&self.config.codex_home, &known_thread_ids)?;
        let automations = read_automations(&self.config.codex_home)?
            .into_iter()
            .map(|automation| automation.to_summary(&known_thread_ids))
            .collect::<Vec<_>>();
        SyncManifest::metadata_only(
            &self.status(),
            &goals,
            &automations,
            &threads,
            &capabilities,
        )
    }

    pub fn sync_manifest_response(&self) -> Result<SyncManifest> {
        let manifest = self.sync_manifest()?;
        let body_json = serde_json::to_string(&manifest)?;
        self.store
            .record_sync_manifest_snapshot(manifest.generated_at_ms, &body_json)?;
        Ok(manifest)
    }

    pub fn desktop_snapshot(&self) -> Result<DesktopSnapshot> {
        self.desktop_snapshot_with_limits(
            Some(DESKTOP_SNAPSHOT_THREAD_LIMIT),
            DESKTOP_COMPACTION_LIMIT,
            DESKTOP_COMPACTION_FILE_SCAN_LIMIT,
            SnapshotInspectionMode::Live,
        )
    }

    pub fn desktop_snapshot_with_thread_limit(
        &self,
        thread_limit: usize,
    ) -> Result<DesktopSnapshot> {
        self.desktop_snapshot_with_limits(
            Some(thread_limit.min(DESKTOP_SNAPSHOT_THREAD_LIMIT)),
            DESKTOP_COMPACTION_LIMIT,
            DESKTOP_COMPACTION_FILE_SCAN_LIMIT,
            SnapshotInspectionMode::CachedMenu,
        )
    }

    pub fn desktop_menu_snapshot(&self) -> Result<DesktopSnapshot> {
        self.response_cache.desktop_menu_snapshot.get_or_refresh(
            DESKTOP_MENU_RESPONSE_CACHE_TTL,
            || {
                self.desktop_snapshot_with_limits(
                    Some(DESKTOP_MENU_THREAD_LIMIT),
                    DESKTOP_MENU_COMPACTION_LIMIT,
                    DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT,
                    SnapshotInspectionMode::CachedMenu,
                )
            },
        )
    }

    pub(crate) fn desktop_handoff_snapshot(&self) -> Result<DesktopSnapshot> {
        self.desktop_snapshot_with_limits(
            Some(DESKTOP_MENU_THREAD_LIMIT),
            DESKTOP_MENU_COMPACTION_LIMIT,
            DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT,
            SnapshotInspectionMode::Live,
        )
    }

    pub fn desktop_mobile_snapshot(&self) -> Result<DesktopSnapshot> {
        let mut snapshot = self.desktop_snapshot_with_limits(
            Some(DESKTOP_SNAPSHOT_THREAD_LIMIT),
            DESKTOP_MENU_COMPACTION_LIMIT,
            DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT,
            SnapshotInspectionMode::Mobile,
        )?;
        snapshot.revision = self.mobile_snapshot_revision()?;
        Ok(snapshot)
    }

    fn desktop_snapshot_with_limits(
        &self,
        thread_limit: Option<usize>,
        compaction_limit: usize,
        compaction_file_scan_limit: usize,
        inspection_mode: SnapshotInspectionMode,
    ) -> Result<DesktopSnapshot> {
        let include_diagnostic_details = inspection_mode.includes_diagnostic_details();
        let include_transcript_previews = inspection_mode.includes_transcript_previews();
        let prune_diagnostic_details = !include_diagnostic_details;
        let control_plane_status = self.snapshot_status(inspection_mode);
        let state = read_snapshot_state_with_thread_limit(&self.config.codex_home, thread_limit)?;
        let all_threads = state.threads.clone();
        let snapshot_codex_threads = codex_threads_for_snapshot(&all_threads, thread_limit);
        let capabilities = self.capabilities_by_threads(&state, &snapshot_codex_threads);
        let mut desktop_threads = snapshot_codex_threads
            .iter()
            .map(|thread| {
                let capabilities = capabilities
                    .get(&thread.thread_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("missing capabilities for {}", thread.thread_id))?;
                Ok(codex_thread_to_desktop_thread(
                    thread,
                    capabilities,
                    include_transcript_previews,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let external_sessions = self.external_sessions_for_snapshot(
            thread_limit,
            inspection_mode,
            include_transcript_previews,
        );
        let SnapshotExternalSessions {
            grok_sessions,
            grok_build,
            claude_sessions,
            devin_discovery,
        } = external_sessions;
        desktop_threads.extend(
            limited_items(&grok_sessions, thread_limit).map(grok_session_to_desktop_thread),
        );
        desktop_threads.extend(
            limited_items(&claude_sessions, thread_limit).map(claude_session_to_desktop_thread),
        );
        let devin_total_count = devin_discovery.total_count;
        let devin_active_thread_count = devin_discovery.active_count;
        let devin_archived_thread_count = devin_discovery.archived_count;
        let devin_session_errors = devin_discovery.errors;
        let devin_sessions = devin_discovery.sessions;
        desktop_threads.extend(
            limited_items(&devin_sessions, thread_limit).map(devin_session_to_desktop_thread),
        );
        let devin_acp_runtime = self.devin_acp_runtime.status();
        desktop_threads.extend(
            limited_items(&devin_acp_runtime.sessions, thread_limit)
                .map(devin_acp_runtime_session_to_desktop_thread),
        );
        let zed_acp_runtime = self.zed_acp_runtime.status();
        let mut zed_acp_threads = limited_items(&zed_acp_runtime.sessions, thread_limit)
            .map(zed_acp_runtime_session_to_desktop_thread)
            .collect::<Vec<_>>();
        merge_zed_acp_threads_with_codex_transcripts(&mut zed_acp_threads, &desktop_threads);
        desktop_threads.extend(zed_acp_threads);
        dedupe_desktop_threads_by_id(&mut desktop_threads);
        desktop_threads.sort_by(|left, right| {
            desktop_thread_activity_ms(right).cmp(&desktop_thread_activity_ms(left))
        });
        let visible_codex_threads = snapshot_codex_threads;

        let compactions = if prune_diagnostic_details {
            Vec::new()
        } else {
            read_recent_compaction_events(
                &self.config.codex_home,
                compaction_limit,
                compaction_file_scan_limit,
            )?
        };
        let mut known_thread_ids = state.known_thread_ids.clone();
        known_thread_ids.extend(
            grok_sessions
                .iter()
                .map(|session| session.session_id.clone()),
        );
        known_thread_ids.extend(
            claude_sessions
                .iter()
                .map(|session| session.thread_id.clone()),
        );
        known_thread_ids.extend(
            devin_sessions
                .iter()
                .map(|session| session.thread_id.clone()),
        );
        known_thread_ids.extend(
            devin_acp_runtime
                .sessions
                .iter()
                .map(|session| session.public_thread_id.clone()),
        );
        known_thread_ids.extend(
            zed_acp_runtime
                .sessions
                .iter()
                .map(|session| session.public_thread_id.clone()),
        );
        let goals = read_goals(&self.config.codex_home, &known_thread_ids)?;
        attach_goals_to_desktop_threads(&mut desktop_threads, &goals);
        if prune_diagnostic_details {
            thin_desktop_threads_for_bounded_snapshot(&mut desktop_threads);
        }
        let automations = read_automations(&self.config.codex_home)?
            .into_iter()
            .map(|automation| automation.to_summary(&known_thread_ids))
            .collect::<Vec<_>>();
        let codex_active_thread_count = state.active_thread_count;
        let codex_archived_thread_count = state
            .total_thread_count
            .saturating_sub(codex_active_thread_count);
        let grok_active_thread_count = grok_build.active_session_count;
        let claude_active_thread_count = claude_sessions
            .iter()
            .filter(|session| session.running)
            .count();
        let devin_acp_active_thread_count = devin_acp_runtime.sessions.len();
        let zed_acp_active_thread_count =
            active_zed_acp_runtime_session_count(&zed_acp_runtime.sessions);
        let active_thread_count = codex_active_thread_count
            + grok_active_thread_count
            + claude_active_thread_count
            + devin_active_thread_count
            + devin_acp_active_thread_count
            + zed_acp_active_thread_count;
        let archived_thread_count = codex_archived_thread_count + devin_archived_thread_count;
        let sync_manifest = if prune_diagnostic_details {
            SyncManifest::metadata_only(&control_plane_status, &[], &[], &[], &BTreeMap::new())?
        } else {
            SyncManifest::metadata_only(
                &control_plane_status,
                &goals,
                &automations,
                &visible_codex_threads,
                &capabilities,
            )?
        };

        let assistant_adapters = if prune_diagnostic_details {
            static_adapter_capabilities()
        } else {
            self.assistant_adapters()
        };
        let (devin_desktop, zed) = self.desktop_snapshot_inspections(inspection_mode);
        let mut acp_targets = devin_acp_targets(&devin_desktop);
        acp_targets.extend(zed_acp_targets(&zed));
        acp_targets.sort_by(|left, right| left.id.cmp(&right.id));

        let session_state = self.mobile_session_service().state()?;
        let revision = loaded_desktop_snapshot_revision(
            &desktop_threads,
            &goals,
            &automations,
            &session_state,
            active_thread_count,
            archived_thread_count,
        );

        Ok(DesktopSnapshot {
            revision,
            control_plane: control_plane_status,
            thread_count: state.total_thread_count
                + grok_build.session_count
                + claude_sessions.len()
                + devin_total_count
                + devin_acp_runtime.sessions.len()
                + zed_acp_runtime.sessions.len(),
            active_thread_count,
            archived_thread_count,
            threads: desktop_threads,
            automations,
            automation_runs: if prune_diagnostic_details {
                Vec::new()
            } else {
                self.store.automation_runs()?
            },
            goals,
            sync_manifest,
            assistant_adapters,
            acp_targets,
            devin_desktop,
            devin_session_count: devin_total_count,
            devin_active_session_count: devin_active_thread_count,
            devin_session_errors,
            zed,
            grok_build,
            compactions,
        })
    }

    fn desktop_snapshot_inspections(
        &self,
        inspection_mode: SnapshotInspectionMode,
    ) -> (DevinDesktopStatus, ZedStatus) {
        match inspection_mode {
            SnapshotInspectionMode::Live => (
                self.inspect_devin_desktop_status(),
                self.inspect_zed_status(),
            ),
            SnapshotInspectionMode::CachedMenu | SnapshotInspectionMode::Mobile => {
                (self.cached_devin_desktop_status(), self.cached_zed_status())
            }
        }
    }

    pub fn record_automation_fire(
        &self,
        input: AutomationRunInput<'_>,
    ) -> Result<Option<AutomationRunRecord>> {
        self.store.record_automation_run(input)
    }

    pub fn update_automation_fire_result(
        &self,
        run_id: &str,
        delivery_mode: &str,
        result: &str,
        detail: Option<&str>,
    ) -> Result<AutomationRunRecord> {
        self.store
            .update_automation_run_result(run_id, delivery_mode, result, detail)
    }

    fn capabilities_by_thread(
        &self,
        state: &StateData,
    ) -> Result<BTreeMap<String, ThreadCapabilities>> {
        Ok(self.capabilities_by_threads(state, &state.threads))
    }

    fn capabilities_by_threads(
        &self,
        state: &StateData,
        threads: &[ThreadRecord],
    ) -> BTreeMap<String, ThreadCapabilities> {
        threads
            .iter()
            .map(|thread| {
                (
                    thread.thread_id.clone(),
                    capabilities_for_state_thread(state, &thread.thread_id),
                )
            })
            .collect()
    }

    fn claude_sessions_for_snapshot(
        &self,
        thread_limit: Option<usize>,
    ) -> Vec<ClaudeSessionRecord> {
        self.claude_sessions(thread_limit).unwrap_or_default()
    }

    fn external_sessions_for_snapshot(
        &self,
        thread_limit: Option<usize>,
        inspection_mode: SnapshotInspectionMode,
        include_assistant_previews: bool,
    ) -> SnapshotExternalSessions {
        if !inspection_mode.discovers_live_external_sessions() {
            return SnapshotExternalSessions::stale();
        }

        let grok_sessions = discover_grok_sessions(self.grok_home()).unwrap_or_default();
        let grok_build = GrokBuildStatus {
            hooks: inspect_grok_hooks(self.grok_home()),
            session_count: grok_sessions.len(),
            active_session_count: grok_sessions
                .iter()
                .filter(|session| session.running)
                .count(),
        };
        let claude_sessions = self.claude_sessions_for_snapshot(thread_limit);
        let devin_discovery =
            self.devin_sessions_for_snapshot(thread_limit, include_assistant_previews);

        SnapshotExternalSessions {
            grok_sessions,
            grok_build,
            claude_sessions,
            devin_discovery,
        }
    }

    fn devin_sessions_for_snapshot(
        &self,
        thread_limit: Option<usize>,
        include_assistant_previews: bool,
    ) -> DevinSessionDiscovery {
        match (thread_limit, include_assistant_previews) {
            (Some(limit), true) => {
                discover_recent_devin_sessions_with_previews(self.home_path(), limit)
                    .unwrap_or_else(|_| empty_devin_session_discovery())
            }
            (Some(limit), false) => discover_recent_devin_sessions(self.home_path(), limit)
                .unwrap_or_else(|_| empty_devin_session_discovery()),
            (None, true) => discover_devin_sessions_with_previews(self.home_path())
                .unwrap_or_else(|_| empty_devin_session_discovery()),
            (None, false) => discover_devin_sessions_without_previews(self.home_path())
                .unwrap_or_else(|_| empty_devin_session_discovery()),
        }
    }

    fn recent_devin_sessions_for_snapshot(&self, limit: usize) -> DevinSessionDiscovery {
        discover_recent_devin_sessions(self.home_path(), limit)
            .unwrap_or_else(|_| empty_devin_session_discovery())
    }

    fn known_desktop_thread_ids(&self) -> Result<BTreeSet<String>> {
        let state = read_state(&self.config.codex_home)?;
        let mut thread_ids = known_thread_ids(&state.threads);
        thread_ids.extend(
            discover_devin_sessions_without_previews(self.home_path())
                .unwrap_or_default()
                .sessions
                .into_iter()
                .map(|session| session.thread_id),
        );
        thread_ids.extend(
            discover_grok_sessions(self.grok_home())
                .unwrap_or_default()
                .into_iter()
                .map(|session| session.session_id),
        );
        thread_ids.extend(
            self.claude_sessions(None)
                .unwrap_or_default()
                .into_iter()
                .map(|session| session.thread_id),
        );
        Ok(thread_ids)
    }
}

fn current_process_commands() -> Vec<String> {
    let output = std::process::Command::new(HOST_PROCESS_COMMAND)
        .args(HOST_PROCESS_COMMAND_ARGS)
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

/// Callers include bare OS threads (e.g. the prompt-delivery worker), where
/// `tokio::task::spawn_blocking` panics with "no reactor running". Prompt
/// delivery must never die because a projection reconcile was scheduled from
/// the wrong thread, so fall back to a plain thread outside a runtime.
fn spawn_blocking_from_any_thread(work: impl FnOnce() + Send + 'static) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn_blocking(work);
        }
        Err(_) => {
            if let Err(error) = std::thread::Builder::new()
                .name("looper-session-mini-reconcile".to_owned())
                .spawn(work)
            {
                eprintln!("session mini reconcile worker failed to start: {error}");
            }
        }
    }
}

fn codex_server_connection(server: &CodexServerProcess) -> ManagedConnection {
    ManagedConnection {
        id: format!("codex-server-{}", server.pid),
        kind: CODEX_CONNECTION_KIND.to_owned(),
        label: codex_server_owner_label(&server.owner),
        status: "connected".to_owned(),
        subtitle: server.tty.clone(),
        detail: Some(server.command.clone()),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(READ_ONLY_CONNECTION_ACTION_HINT.to_owned()),
    }
}

fn hook_connection(status: &ControlPlaneStatus) -> ManagedConnection {
    ManagedConnection {
        id: CODEX_HOOKS_CONNECTION_ID.to_owned(),
        kind: CODEX_CONNECTION_KIND.to_owned(),
        label: CODEX_HOOKS_CONNECTION_LABEL.to_owned(),
        status: status.hooks.health.clone(),
        subtitle: Some(hook_owner_label(&status.hooks.owner)),
        detail: status.hooks.active_command.clone(),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(READ_ONLY_CONNECTION_ACTION_HINT.to_owned()),
    }
}

fn grok_build_cli_connections_from_adapters(
    adapters: &[AssistantAdapterCapability],
) -> Vec<ManagedConnection> {
    let Some(adapter) = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::GrokBuild)
    else {
        return Vec::new();
    };

    let cli_runtime = adapter
        .runtimes
        .iter()
        .find(|runtime| runtime.label == GROK_BUILD_CLI_RUNTIME_LABEL);
    let session_runtime = adapter
        .runtimes
        .iter()
        .find(|runtime| runtime.label == GROK_BUILD_SESSION_RUNTIME_LABEL);
    let cli_running = cli_runtime.is_some_and(|runtime| runtime.running);
    let session_running = session_runtime.is_some_and(|runtime| runtime.running);
    let cli_installed = cli_runtime.is_some_and(|runtime| runtime.installed);

    if !cli_running && !session_running && !cli_installed {
        return Vec::new();
    }

    let status = if cli_running || session_running {
        "connected"
    } else {
        "installed"
    }
    .to_owned();
    let subtitle = if session_running {
        Some("session active".to_owned())
    } else if cli_running {
        Some("CLI running".to_owned())
    } else {
        cli_runtime.and_then(|runtime| runtime.executable.clone())
    };

    vec![ManagedConnection {
        id: GROK_BUILD_CONNECTION_ID.to_owned(),
        kind: GROK_BUILD_CONNECTION_KIND.to_owned(),
        label: GROK_BUILD_CONNECTION_LABEL.to_owned(),
        status,
        subtitle,
        detail: Some(adapter.detail.clone()),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(GROK_BUILD_CONNECTION_ACTION_HINT.to_owned()),
    }]
}

fn grok_hook_connection(status: &GrokHookStatus) -> ManagedConnection {
    ManagedConnection {
        id: GROK_BUILD_HOOKS_CONNECTION_ID.to_owned(),
        kind: GROK_BUILD_CONNECTION_KIND.to_owned(),
        label: GROK_BUILD_HOOKS_CONNECTION_LABEL.to_owned(),
        status: status.health.clone(),
        subtitle: Some(grok_hook_owner_label(&status.owner)),
        detail: status.active_command.clone(),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(GROK_BUILD_CONNECTION_ACTION_HINT.to_owned()),
    }
}

fn grok_hook_owner_label(owner: &GrokHookOwner) -> String {
    match owner {
        GrokHookOwner::LooperRust => "looper Rust".to_owned(),
        GrokHookOwner::Unknown => "Unknown owner".to_owned(),
        GrokHookOwner::None => "Not registered".to_owned(),
    }
}

fn claude_hook_connection(status: &ClaudeHookStatus) -> ManagedConnection {
    ManagedConnection {
        id: CLAUDE_CODE_HOOKS_CONNECTION_ID.to_owned(),
        kind: CLAUDE_CODE_CONNECTION_KIND.to_owned(),
        label: CLAUDE_CODE_HOOKS_CONNECTION_LABEL.to_owned(),
        status: status.health.clone(),
        subtitle: Some(claude_hook_owner_label(&status.owner)),
        detail: status.active_command.clone(),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(CLAUDE_CODE_HOOKS_CONNECTION_ACTION_HINT.to_owned()),
    }
}

fn claude_hook_owner_label(owner: &ClaudeHookOwner) -> String {
    match owner {
        ClaudeHookOwner::LooperRust => "looper Rust".to_owned(),
        ClaudeHookOwner::Unknown => "Unknown owner".to_owned(),
        ClaudeHookOwner::None => "Not registered".to_owned(),
    }
}

fn zed_connection_should_render(status: &ZedStatus) -> bool {
    status.installed || status.running || status.settings_exists || !status.acp_targets.is_empty()
}

fn devin_hook_connection(status: &DevinHookStatus) -> ManagedConnection {
    ManagedConnection {
        id: DEVIN_HOOKS_CONNECTION_ID.to_owned(),
        kind: DEVIN_CONNECTION_KIND.to_owned(),
        label: DEVIN_HOOKS_CONNECTION_LABEL.to_owned(),
        status: status.health.clone(),
        subtitle: Some(devin_hook_owner_label(&status.owner)),
        detail: status.active_command.clone(),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(DEVIN_HOOKS_CONNECTION_ACTION_HINT.to_owned()),
    }
}

fn devin_hook_owner_label(owner: &DevinHookOwner) -> String {
    match owner {
        DevinHookOwner::LooperRust => "looper Rust".to_owned(),
        DevinHookOwner::Unknown => "Unknown owner".to_owned(),
        DevinHookOwner::None => "Not registered".to_owned(),
    }
}

fn dedupe_desktop_threads_by_id(threads: &mut Vec<DesktopThread>) {
    let mut threads_by_id = BTreeMap::<String, DesktopThread>::new();
    for thread in std::mem::take(threads) {
        match threads_by_id.remove(&thread.thread_id) {
            Some(existing) => {
                threads_by_id.insert(
                    thread.thread_id.clone(),
                    preferred_desktop_thread(existing, thread),
                );
            }
            None => {
                threads_by_id.insert(thread.thread_id.clone(), thread);
            }
        }
    }
    *threads = threads_by_id.into_values().collect();
}

fn preferred_desktop_thread(left: DesktopThread, right: DesktopThread) -> DesktopThread {
    let left_score = desktop_thread_source_score(&left);
    let right_score = desktop_thread_source_score(&right);
    if right_score > left_score {
        return merge_desktop_thread(right, left);
    }
    if left_score > right_score {
        return merge_desktop_thread(left, right);
    }

    if desktop_thread_activity_ms(&right) > desktop_thread_activity_ms(&left) {
        merge_desktop_thread(right, left)
    } else {
        merge_desktop_thread(left, right)
    }
}

fn desktop_thread_source_score(thread: &DesktopThread) -> u8 {
    match thread.source.as_deref() {
        Some(CODEX_STATE_SOURCE) => 3,
        Some(CODEX_ACP_SOURCE) => 2,
        _ => 1,
    }
}

fn codex_thread_to_desktop_thread(
    thread: &ThreadRecord,
    capabilities: ThreadCapabilities,
    include_diagnostic_details: bool,
) -> DesktopThread {
    let transcript_preview = include_diagnostic_details
        .then(|| {
            thread
                .transcript_path
                .as_deref()
                .and_then(|path| transcript_preview_for_path_fast(Path::new(path)))
        })
        .flatten();
    let transcript_modified_at_ms = include_diagnostic_details
        .then(|| {
            thread
                .transcript_path
                .as_deref()
                .and_then(|path| metadata_modified_at_ms(Path::new(path)))
        })
        .flatten();
    let latest_message_at_ms = transcript_preview
        .as_ref()
        .and_then(|preview| preview.latest_message_at_ms);
    let latest_transcript_activity_at_ms = transcript_preview
        .as_ref()
        .and_then(|preview| preview.latest_activity_at_ms);
    let assistant_preview = transcript_preview.as_ref().and_then(|preview| {
        preview
            .latest_assistant_message
            .as_ref()
            .map(|message| message.text.clone())
    });
    let first_user_prompt = transcript_preview
        .as_ref()
        .and_then(|preview| preview.first_user_prompt.clone());
    let updated_at_ms = latest_millis([
        thread.updated_at_ms,
        transcript_modified_at_ms,
        latest_transcript_activity_at_ms,
        latest_message_at_ms,
    ]);
    DesktopThread {
        thread_id: thread.thread_id.clone(),
        title: thread.title.clone(),
        cwd: thread.cwd.clone(),
        transcript_path: thread.transcript_path.clone(),
        source: thread.source.clone(),
        originator: thread.originator.clone(),
        model: thread.model.clone(),
        reasoning_effort: thread.reasoning_effort.clone(),
        git_sha: thread.git_sha.clone(),
        git_branch: thread.git_branch.clone(),
        cli_version: thread.cli_version.clone(),
        agent_nickname: thread.agent_nickname.clone(),
        agent_role: thread.agent_role.clone(),
        agent_path: thread.agent_path.clone(),
        created_at_ms: thread.created_at_ms,
        updated_at_ms,
        latest_message_at_ms,
        assistant_preview,
        first_user_prompt,
        runtime_status: None,
        archived: thread.archived,
        goal: None,
        capabilities,
    }
}

fn desktop_thread_activity_ms(thread: &DesktopThread) -> i64 {
    latest_millis([
        thread.updated_at_ms,
        thread.latest_message_at_ms,
        thread.created_at_ms,
    ])
    .unwrap_or_default()
}

fn merge_desktop_thread(mut preferred: DesktopThread, fallback: DesktopThread) -> DesktopThread {
    preferred.updated_at_ms = latest_millis([preferred.updated_at_ms, fallback.updated_at_ms]);
    preferred.latest_message_at_ms = latest_millis([
        preferred.latest_message_at_ms,
        fallback.latest_message_at_ms,
    ]);
    if preferred.assistant_preview.is_none() {
        preferred.assistant_preview = fallback.assistant_preview;
    }
    if preferred.first_user_prompt.is_none() {
        preferred.first_user_prompt = fallback.first_user_prompt;
    }
    if preferred.transcript_path.is_none() {
        preferred.transcript_path = fallback.transcript_path;
    }
    if preferred.runtime_status.is_none() {
        preferred.runtime_status = fallback.runtime_status;
    }
    preferred
}

fn merge_zed_acp_threads_with_codex_transcripts(
    zed_threads: &mut [DesktopThread],
    existing_threads: &[DesktopThread],
) {
    for zed_thread in zed_threads {
        let Some(agent_id) = zed_public_agent_id_from_thread_id(&zed_thread.thread_id) else {
            continue;
        };
        if agent_id != "codex" {
            continue;
        }
        let Some(codex_thread) =
            matching_codex_thread_for_zed_acp_thread(zed_thread, existing_threads)
        else {
            continue;
        };
        merge_codex_transcript_into_zed_acp_thread(zed_thread, codex_thread);
    }
}

fn matching_codex_thread_for_zed_acp_thread<'a>(
    zed_thread: &DesktopThread,
    existing_threads: &'a [DesktopThread],
) -> Option<&'a DesktopThread> {
    let zed_cwd = normalized_path_text(zed_thread.cwd.as_deref())?;
    existing_threads
        .iter()
        .filter(|thread| thread.capabilities.assistant_kind == AssistantKind::Codex)
        .filter(|thread| !thread.archived)
        .filter(|thread| {
            normalized_path_text(thread.cwd.as_deref()).as_deref() == Some(zed_cwd.as_str())
        })
        .max_by_key(|thread| desktop_thread_activity_ms(thread))
}

fn merge_codex_transcript_into_zed_acp_thread(
    zed_thread: &mut DesktopThread,
    codex_thread: &DesktopThread,
) {
    zed_thread.title = codex_thread
        .title
        .clone()
        .or_else(|| zed_thread.title.clone());
    zed_thread.transcript_path = codex_thread.transcript_path.clone();
    zed_thread.model = codex_thread.model.clone();
    zed_thread.reasoning_effort = codex_thread.reasoning_effort.clone();
    zed_thread.git_sha = codex_thread.git_sha.clone();
    zed_thread.git_branch = codex_thread.git_branch.clone();
    zed_thread.cli_version = codex_thread.cli_version.clone();
    zed_thread.agent_path = codex_thread.agent_path.clone();
    zed_thread.created_at_ms =
        latest_millis([zed_thread.created_at_ms, codex_thread.created_at_ms]);
    zed_thread.updated_at_ms =
        latest_millis([zed_thread.updated_at_ms, codex_thread.updated_at_ms]);
    zed_thread.latest_message_at_ms = latest_millis([
        zed_thread.latest_message_at_ms,
        codex_thread.latest_message_at_ms,
    ]);
    zed_thread.assistant_preview = codex_thread
        .assistant_preview
        .clone()
        .or_else(|| zed_thread.assistant_preview.clone());
    zed_thread.first_user_prompt = codex_thread
        .first_user_prompt
        .clone()
        .or_else(|| zed_thread.first_user_prompt.clone());
}

fn normalized_path_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.trim_end_matches('/').to_owned())
}

fn zed_public_agent_id_from_thread_id(thread_id: &str) -> Option<&str> {
    let remainder = thread_id.strip_prefix("zed:")?;
    remainder.split(':').next()
}

fn zed_public_agent_title(agent_id: &str) -> String {
    match agent_id {
        "codex" => "Codex".to_owned(),
        "codex-direct" => "Codex Direct".to_owned(),
        "looper" => "Looper".to_owned(),
        _ => agent_id.to_owned(),
    }
}

fn latest_millis(values: impl IntoIterator<Item = Option<i64>>) -> Option<i64> {
    values.into_iter().flatten().max()
}

fn attach_goals_to_desktop_threads(threads: &mut [DesktopThread], goals: &[GoalSummary]) {
    for thread in threads {
        thread.goal = goal_for_thread(goals, &thread.thread_id);
    }
}

fn devin_desktop_connection(
    installation: &DevinInstallationStatus,
    status: &DevinDesktopStatus,
) -> ManagedConnection {
    ManagedConnection {
        id: installation.id.clone(),
        kind: DEVIN_CONNECTION_KIND.to_owned(),
        label: installation.label.clone(),
        status: devin_connection_status(installation),
        subtitle: Some(format!("{} channel", installation.channel)),
        detail: Some(devin_connection_detail(installation, status)),
        created_at: None,
        last_used_at: None,
        revoked_at: None,
        can_rename: false,
        can_revoke: false,
        action_hint: Some(DEVIN_CONNECTION_ACTION_HINT.to_owned()),
    }
}

fn devin_connection_status(installation: &DevinInstallationStatus) -> String {
    if installation.running {
        "connected"
    } else if installation.acp_enabled == Some(true) {
        "configured"
    } else if installation.installed {
        "installed"
    } else {
        "missing"
    }
    .to_owned()
}

fn devin_installation_should_render(installation: &DevinInstallationStatus) -> bool {
    installation.running || installation.installed || installation.settings_exists
}

fn codex_server_owner_label(owner: &CodexServerOwner) -> String {
    match owner {
        CodexServerOwner::CodexApp => "Codex app",
        CodexServerOwner::CodexCli => "Codex CLI",
        CodexServerOwner::Cursor => "Cursor Codex",
        CodexServerOwner::DevinDesktop => "Devin Desktop",
        CodexServerOwner::Superconductor => "Superconductor Codex",
        CodexServerOwner::Unknown => "Codex server",
    }
    .to_owned()
}

fn hook_owner_label(owner: &HookOwner) -> String {
    match owner {
        HookOwner::LooperRust => "looper Rust",
        HookOwner::Unknown => "Unknown owner",
        HookOwner::None => "Not registered",
    }
    .to_owned()
}

fn known_thread_ids(threads: &[ThreadRecord]) -> BTreeSet<String> {
    threads
        .iter()
        .map(|thread| thread.thread_id.clone())
        .collect()
}

fn thin_desktop_threads_for_bounded_snapshot(threads: &mut [DesktopThread]) {
    for thread in threads {
        thread.capabilities.tools.clear();
        thread.capabilities.mcp_tools.clear();
        thread.capabilities.app_tools.clear();
        thread.capabilities.automation_tools.clear();
        thread.capabilities.spawn.children.clear();
        thread.capabilities.diff.paths.clear();
    }
}

fn codex_threads_for_snapshot(
    all_threads: &[ThreadRecord],
    thread_limit: Option<usize>,
) -> Vec<ThreadRecord> {
    let mut threads = all_threads.to_vec();
    if let Some(limit) = thread_limit {
        threads.sort_by(|left, right| {
            right
                .updated_at_ms
                .unwrap_or_default()
                .cmp(&left.updated_at_ms.unwrap_or_default())
                .then_with(|| left.thread_id.cmp(&right.thread_id))
        });
        threads.truncate(limit);
    }
    threads
}

fn limited_items<T>(items: &[T], limit: Option<usize>) -> impl Iterator<Item = &T> {
    items.iter().take(limit.unwrap_or(items.len()))
}

fn empty_devin_session_discovery() -> DevinSessionDiscovery {
    DevinSessionDiscovery::default()
}

fn stale_grok_build_status() -> GrokBuildStatus {
    GrokBuildStatus {
        hooks: GrokHookStatus {
            registered_events: Vec::new(),
            active_command: None,
            owner: GrokHookOwner::None,
            health: BOUNDED_SNAPSHOT_STALE_HEALTH.to_owned(),
            hooks_path: None,
        },
        session_count: 0,
        active_session_count: 0,
    }
}

fn desktop_thread_to_thread_record(thread: &DesktopThread) -> ThreadRecord {
    ThreadRecord {
        thread_id: thread.thread_id.clone(),
        title: thread.title.clone(),
        cwd: thread.cwd.clone(),
        transcript_path: thread.transcript_path.clone(),
        source: thread.source.clone(),
        originator: thread.originator.clone(),
        model: thread.model.clone(),
        reasoning_effort: thread.reasoning_effort.clone(),
        git_sha: thread.git_sha.clone(),
        git_branch: thread.git_branch.clone(),
        cli_version: thread.cli_version.clone(),
        agent_nickname: thread.agent_nickname.clone(),
        agent_role: thread.agent_role.clone(),
        agent_path: thread.agent_path.clone(),
        created_at_ms: thread.created_at_ms,
        updated_at_ms: thread.updated_at_ms,
        archived: thread.archived,
    }
}

fn metadata_modified_at_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    let millis = duration.as_millis();
    i64::try_from(millis).ok()
}

fn transcript_source_signature(threads: &[ThreadRevisionRecord]) -> String {
    let mut parts = threads
        .iter()
        .filter_map(|thread| {
            let path = thread.transcript_path.as_deref()?;
            let metadata = fs::metadata(Path::new(path)).ok();
            let modified_at_ms = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
                .unwrap_or_default();
            let len = metadata.as_ref().map(fs::Metadata::len).unwrap_or_default();
            Some(format!(
                "{}:{}:{}:{}",
                thread.thread_id, path, modified_at_ms, len
            ))
        })
        .collect::<Vec<_>>();
    parts.sort();
    parts.join("|")
}

fn mobile_session_state_revision(state: &MobileSessionState) -> String {
    let session_overrides = state
        .sessions
        .iter()
        .map(|(thread_id, override_state)| {
            json!({
                "threadId": thread_id,
                "preset": override_state.preset,
                "archived": override_state.archived,
                "muted": override_state.muted,
                "deleted": override_state.deleted,
                "deletedAt": override_state.deleted_at,
                "notificationIds": override_state.notification_ids,
                "completionCheckId": override_state.completion_check_id,
                "completionCheckWaitForReply": override_state.completion_check_wait_for_reply,
            })
        })
        .collect::<Vec<_>>();
    let lifecycle = state
        .lifecycle
        .iter()
        .map(|(thread_id, lifecycle)| {
            json!({
                "threadId": thread_id,
                "status": lifecycle.status,
                "updatedAt": lifecycle.updated_at,
            })
        })
        .collect::<Vec<_>>();
    let notifications = state
        .notifications
        .iter()
        .map(|notification| {
            json!({
                "id": notification.id,
                "label": notification.label,
                "channel": notification.channel,
                "hasWebhookUrl": notification.webhook_url.is_some(),
                "hasChatId": notification.chat_id.is_some(),
                "hasBotToken": notification.bot_token.is_some(),
                "hasBotUrl": notification.bot_url.is_some(),
                "chatUsername": notification.chat_username,
                "chatDisplayName": notification.chat_display_name,
            })
        })
        .collect::<Vec<_>>();
    let completion_checks = state
        .completion_checks
        .iter()
        .map(|check| {
            json!({
                "id": check.id,
                "label": check.label,
                "commandsHash": revision_hash(&check.commands.join("\n")),
            })
        })
        .collect::<Vec<_>>();
    let fingerprint = json!({
        "defaultPromptHash": revision_hash(&state.default_prompt),
        "scope": state.scope,
        "globalPreset": state.global_preset,
        "globalNotificationId": state.global_notification_id,
        "defaultNotificationTargetIds": state.default_notification_target_ids,
        "globalCompletionCheckId": state.global_completion_check_id,
        "globalCompletionCheckWaitForReply": state.global_completion_check_wait_for_reply,
        "assistantSurface": state.assistant_surface,
        "siriDefaultThreadId": state.siri_default_thread_id,
        "siriDefaultAssistantSurface": state.siri_default_assistant_surface,
        "siriCurrentThreadId": state.siri_current_thread_id,
        "siriCurrentAssistantSurface": state.siri_current_assistant_surface,
        "siriCurrentUpdatedAtMs": state.siri_current_updated_at_ms,
        "notifications": notifications,
        "completionChecks": completion_checks,
        "sessions": session_overrides,
        "lifecycle": lifecycle,
    });
    revision_hash(&fingerprint.to_string())
}

fn loaded_desktop_snapshot_revision(
    threads: &[DesktopThread],
    goals: &[GoalSummary],
    automations: &[AutomationSummary],
    session_state: &MobileSessionState,
    active_thread_count: usize,
    archived_thread_count: usize,
) -> String {
    let mut thread_signature = threads
        .iter()
        .map(|thread| {
            json!({
                "id": thread.thread_id,
                "source": thread.source,
                "updatedAt": thread.updated_at_ms,
                "latestMessageAt": thread.latest_message_at_ms,
                "archived": thread.archived,
                "runtimeStatus": thread.runtime_status,
                "assistantPreviewHash": thread
                    .assistant_preview
                    .as_deref()
                    .map(revision_hash),
                "firstUserPromptHash": thread
                    .first_user_prompt
                    .as_deref()
                    .map(revision_hash),
                "goal": thread.goal,
            })
            .to_string()
        })
        .collect::<Vec<_>>();
    thread_signature.sort();

    let mut goal_signature = goals
        .iter()
        .map(|goal| {
            json!({
                "id": goal.id,
                "targetThreadId": goal.target_thread_id,
                "running": goal.running,
                "updatedAt": goal.updated_at_ms,
                "contentHash": goal.content_hash,
            })
            .to_string()
        })
        .collect::<Vec<_>>();
    goal_signature.sort();

    let mut automation_signature = automations
        .iter()
        .map(|automation| {
            json!({
                "id": automation.id,
                "targetThreadId": automation.target_thread_id,
                "status": automation.status,
                "rrule": automation.rrule,
                "covered": automation.control_plane_covered,
            })
            .to_string()
        })
        .collect::<Vec<_>>();
    automation_signature.sort();

    let queued_prompt_count = session_state
        .sessions
        .values()
        .filter(|session| !session.deleted)
        .count();
    let fingerprint = json!({
        "threads": thread_signature,
        "goals": goal_signature,
        "automations": automation_signature,
        "activeThreadCount": active_thread_count,
        "archivedThreadCount": archived_thread_count,
        "queuedPromptCount": queued_prompt_count,
        "assistantSurface": session_state.assistant_surface,
        "mobileState": mobile_session_state_revision(session_state),
    });
    revision_hash(&fingerprint.to_string())
}

fn revision_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::{AssistantRuntime, AssistantRuntimeKind};
    use serde_json::json;

    const STORED_THREAD_UPDATED_AT_MS: i64 = 1;
    const TRANSCRIPT_MESSAGE_AT_MS: i64 = 1_781_568_060_000;

    fn thread_record(thread_id: &str, updated_at_ms: i64) -> ThreadRecord {
        ThreadRecord {
            thread_id: thread_id.to_owned(),
            title: None,
            cwd: None,
            transcript_path: None,
            source: None,
            originator: None,
            model: None,
            reasoning_effort: None,
            git_sha: None,
            git_branch: None,
            cli_version: None,
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
            created_at_ms: None,
            updated_at_ms: Some(updated_at_ms),
            archived: false,
        }
    }

    fn grok_build_adapter(runtimes: Vec<AssistantRuntime>) -> AssistantAdapterCapability {
        AssistantAdapterCapability {
            assistant_kind: AssistantKind::GrokBuild,
            live_sessions: true,
            tool_inventory: false,
            spawn_graph: false,
            diff_summary: false,
            auth_capabilities: false,
            runtimes,
            detail: "Grok hooks and sessions".to_owned(),
        }
    }

    fn codex_capabilities(thread_id: &str) -> ThreadCapabilities {
        ThreadCapabilities {
            thread_id: thread_id.to_owned(),
            assistant_kind: AssistantKind::Codex,
            tools: Vec::new(),
            mcp_tools: Vec::new(),
            app_tools: Vec::new(),
            automation_tools: Vec::new(),
            spawn: SpawnGraph {
                parent_thread_id: None,
                root_thread_id: thread_id.to_owned(),
                children: Vec::new(),
                launch_kind: LaunchKind::Main,
            },
            diff: DiffSummary {
                git_branch: None,
                git_sha: None,
                produced_file_changes: false,
                paths: Vec::new(),
            },
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
        }
    }

    fn transcript_record(role: &str, content_key: &str, text: &str) -> String {
        json!({
            "type": "response_item",
            "timestamp": "2026-06-16T00:01:00Z",
            "payload": {
                "type": "message",
                "role": role,
                "content": [
                    {
                        "type": content_key,
                        "text": text
                    }
                ]
            }
        })
        .to_string()
    }

    #[test]
    fn limited_snapshot_threads_use_most_recent_codex_threads() {
        let threads = vec![
            thread_record("old", 1),
            thread_record("new", 3),
            thread_record("middle", 2),
        ];

        let limited = codex_threads_for_snapshot(&threads, Some(2));

        assert_eq!(
            limited
                .iter()
                .map(|thread| thread.thread_id.as_str())
                .collect::<Vec<_>>(),
            vec!["new", "middle"]
        );
    }

    #[test]
    fn mobile_snapshot_mode_uses_bounded_cached_recovery_contract() {
        assert!(SnapshotInspectionMode::Mobile.discovers_live_external_sessions());
        assert!(!SnapshotInspectionMode::CachedMenu.discovers_live_external_sessions());
        assert!(SnapshotInspectionMode::Live.discovers_live_external_sessions());
        assert!(SnapshotInspectionMode::Mobile.includes_transcript_previews());
        assert!(!SnapshotInspectionMode::CachedMenu.includes_transcript_previews());
        assert!(SnapshotInspectionMode::Live.includes_transcript_previews());

        let threads = (0..(DESKTOP_SNAPSHOT_THREAD_LIMIT + 1))
            .map(|index| {
                thread_record(
                    &format!("thread-{index:03}"),
                    i64::try_from(index).expect("thread index fits i64"),
                )
            })
            .collect::<Vec<_>>();

        let bounded = codex_threads_for_snapshot(&threads, Some(DESKTOP_SNAPSHOT_THREAD_LIMIT));

        assert_eq!(bounded.len(), DESKTOP_SNAPSHOT_THREAD_LIMIT);
        assert_eq!(
            bounded.first().map(|thread| thread.thread_id.as_str()),
            Some("thread-250")
        );
        assert_eq!(
            bounded.last().map(|thread| thread.thread_id.as_str()),
            Some("thread-001")
        );
    }

    #[test]
    fn bounded_snapshot_status_skips_live_process_inventory() {
        let fixture_dir = tempfile::tempdir().expect("tempdir");
        let control_plane = ControlPlane::new(ControlPlaneConfig {
            codex_home: fixture_dir.path().join(".codex"),
            codex_executable: None,
            claude_executable: Some("/usr/bin/false".to_owned()),
            store_path: fixture_dir.path().join("store.sqlite"),
            hook_command: None,
            host_environment: HostEnvironment::hermetic(fixture_dir.path().to_path_buf()),
        });

        let status = control_plane.snapshot_status(SnapshotInspectionMode::Mobile);

        assert!(status.app_server.is_none());
        assert!(status.codex_servers.is_empty());
        assert_eq!(status.source.health, "degraded");
    }

    #[test]
    fn pruned_codex_desktop_thread_skips_transcript_details() {
        let fixture_dir = tempfile::tempdir().expect("tempdir");
        let transcript_path = fixture_dir.path().join("transcript.jsonl");
        std::fs::write(
            &transcript_path,
            format!(
                "{}\n{}",
                transcript_record("assistant", "output_text", "live assistant"),
                transcript_record("user", "input_text", "first prompt")
            ),
        )
        .expect("write transcript");
        let mut thread = thread_record("thread-1", STORED_THREAD_UPDATED_AT_MS);
        thread.transcript_path = Some(transcript_path.display().to_string());
        let capabilities = codex_capabilities(&thread.thread_id);

        let live_thread = codex_thread_to_desktop_thread(&thread, capabilities.clone(), true);
        assert_eq!(
            live_thread.assistant_preview.as_deref(),
            Some("live assistant")
        );
        assert_eq!(
            live_thread.latest_message_at_ms,
            Some(TRANSCRIPT_MESSAGE_AT_MS)
        );
        assert_ne!(live_thread.updated_at_ms, Some(STORED_THREAD_UPDATED_AT_MS));

        let pruned_thread = codex_thread_to_desktop_thread(&thread, capabilities, false);
        assert_eq!(pruned_thread.assistant_preview, None);
        assert_eq!(pruned_thread.first_user_prompt, None);
        assert_eq!(pruned_thread.latest_message_at_ms, None);
        assert_eq!(
            pruned_thread.updated_at_ms,
            Some(STORED_THREAD_UPDATED_AT_MS)
        );
    }

    #[test]
    fn transcript_source_signature_tracks_transcript_file_metadata() {
        let fixture_dir = tempfile::tempdir().expect("tempdir");
        let transcript_path = fixture_dir.path().join("thread.jsonl");
        std::fs::write(&transcript_path, "one").expect("write transcript");
        let records = vec![ThreadRevisionRecord {
            thread_id: "thread-1".to_owned(),
            transcript_path: Some(transcript_path.display().to_string()),
            updated_at_ms: Some(1),
            archived: false,
        }];

        let first = transcript_source_signature(&records);
        std::fs::write(&transcript_path, "one\ntwo").expect("update transcript");
        let second = transcript_source_signature(&records);

        assert_ne!(first, second);
        assert!(second.contains(":7"));
    }

    #[test]
    fn grok_build_connections_surface_installed_cli() {
        let connections = grok_build_cli_connections_from_adapters(&[grok_build_adapter(vec![
            AssistantRuntime {
                kind: AssistantRuntimeKind::Cli,
                running: false,
                installed: true,
                label: GROK_BUILD_CLI_RUNTIME_LABEL.to_owned(),
                bundle_id: None,
                executable: Some("/Users/test/.grok/bin/grok".to_owned()),
                command: None,
            },
        ])]);

        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].kind, GROK_BUILD_CONNECTION_KIND);
        assert_eq!(connections[0].status, "installed");
        assert_eq!(
            connections[0].subtitle.as_deref(),
            Some("/Users/test/.grok/bin/grok")
        );
    }

    #[test]
    fn grok_build_connections_surface_running_cli() {
        let connections = grok_build_cli_connections_from_adapters(&[grok_build_adapter(vec![
            AssistantRuntime {
                kind: AssistantRuntimeKind::Cli,
                running: true,
                installed: true,
                label: GROK_BUILD_CLI_RUNTIME_LABEL.to_owned(),
                bundle_id: None,
                executable: Some("/Users/test/.grok/bin/grok".to_owned()),
                command: None,
            },
        ])]);

        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].status, "connected");
        assert_eq!(connections[0].subtitle.as_deref(), Some("CLI running"));
    }

    #[test]
    fn grok_build_connections_surface_active_session() {
        let connections = grok_build_cli_connections_from_adapters(&[grok_build_adapter(vec![
            AssistantRuntime {
                kind: AssistantRuntimeKind::Cli,
                running: true,
                installed: true,
                label: GROK_BUILD_SESSION_RUNTIME_LABEL.to_owned(),
                bundle_id: None,
                executable: None,
                command: Some("grok agent stdio".to_owned()),
            },
        ])]);

        assert_eq!(connections.len(), 1);
        assert_eq!(connections[0].status, "connected");
        assert_eq!(connections[0].subtitle.as_deref(), Some("session active"));
    }

    #[test]
    fn grok_build_connections_hidden_when_missing() {
        let connections = grok_build_cli_connections_from_adapters(&[grok_build_adapter(vec![
            AssistantRuntime {
                kind: AssistantRuntimeKind::Cli,
                running: false,
                installed: false,
                label: GROK_BUILD_CLI_RUNTIME_LABEL.to_owned(),
                bundle_id: None,
                executable: None,
                command: None,
            },
        ])]);

        assert!(connections.is_empty());
    }

    #[test]
    fn managed_connections_always_include_assistant_hook_rows() {
        let fixture_dir = tempfile::tempdir().expect("tempdir");
        let control_plane = ControlPlane::new(ControlPlaneConfig {
            codex_home: fixture_dir.path().join(".codex"),
            codex_executable: None,
            claude_executable: Some("/usr/bin/false".to_owned()),
            store_path: fixture_dir.path().join("store.sqlite"),
            hook_command: None,
            host_environment: HostEnvironment::hermetic(fixture_dir.path().to_path_buf()),
        });

        let connections = control_plane
            .managed_connections_response()
            .expect("connections")
            .connections;
        assert!(
            connections
                .iter()
                .any(|connection| connection.id == GROK_BUILD_HOOKS_CONNECTION_ID)
        );
        assert!(
            connections
                .iter()
                .any(|connection| connection.id == DEVIN_HOOKS_CONNECTION_ID)
        );
        assert!(
            connections
                .iter()
                .any(|connection| connection.id == CLAUDE_CODE_HOOKS_CONNECTION_ID)
        );
    }

    #[test]
    fn timed_response_cache_reuses_fresh_response() {
        let cache = TimedResponseCache::new();
        let mut refresh_count = 0;

        let first = cache
            .get_or_refresh(Duration::from_secs(60), || {
                refresh_count += 1;
                Ok("first".to_owned())
            })
            .expect("first refresh");
        let second = cache
            .get_or_refresh(Duration::from_secs(60), || {
                refresh_count += 1;
                Ok("second".to_owned())
            })
            .expect("cached refresh");

        assert_eq!(refresh_count, 1);
        assert_eq!(first, "first");
        assert_eq!(second, "first");
    }

    #[test]
    fn session_mini_reconciler_waits_for_source_change_after_success() {
        let reconciler = Arc::new(SessionMiniProjectionReconciler::new());
        let signature = SessionMiniProjectionSourceSignature {
            state_db_path: PathBuf::from("/tmp/state_1.sqlite"),
            modified_at_ms: 1,
            len: 10,
            transcript_signature: String::new(),
            acp_runtime_signature: String::new(),
            claude_transcript_signature: String::new(),
        };
        let changed_signature = SessionMiniProjectionSourceSignature {
            len: 11,
            ..signature.clone()
        };

        let permit = reconciler
            .try_acquire_for_source_change(Instant::now(), Duration::ZERO, &signature)
            .expect("first source change acquire");
        drop(permit);
        reconciler.mark_source_signature(signature.clone());

        assert!(
            reconciler
                .try_acquire_for_source_change(Instant::now(), Duration::ZERO, &signature)
                .is_none()
        );
        assert!(
            reconciler
                .try_acquire_for_source_change(Instant::now(), Duration::ZERO, &changed_signature)
                .is_some()
        );
    }

    #[test]
    fn session_mini_reconciler_retries_source_until_marked_successful() {
        let reconciler = Arc::new(SessionMiniProjectionReconciler::new());
        let signature = SessionMiniProjectionSourceSignature {
            state_db_path: PathBuf::from("/tmp/state_1.sqlite"),
            modified_at_ms: 1,
            len: 10,
            transcript_signature: String::new(),
            acp_runtime_signature: String::new(),
            claude_transcript_signature: String::new(),
        };

        let permit = reconciler
            .try_acquire_for_source_change(Instant::now(), Duration::ZERO, &signature)
            .expect("first source change acquire");
        drop(permit);

        assert!(
            reconciler
                .try_acquire_for_source_change(Instant::now(), Duration::ZERO, &signature)
                .is_some()
        );
    }

    #[test]
    fn timed_response_cache_invalidates_response() {
        let cache = TimedResponseCache::new();
        let mut refresh_count = 0;

        let first = cache
            .get_or_refresh(Duration::from_secs(60), || {
                refresh_count += 1;
                Ok("first".to_owned())
            })
            .expect("first refresh");
        cache.invalidate();
        let second = cache
            .get_or_refresh(Duration::from_secs(60), || {
                refresh_count += 1;
                Ok("second".to_owned())
            })
            .expect("second refresh");

        assert_eq!(refresh_count, 2);
        assert_eq!(first, "first");
        assert_eq!(second, "second");
    }

    #[test]
    fn timed_response_cache_reuses_infallible_response() {
        let cache = TimedResponseCache::new();
        let mut refresh_count = 0;

        let first = cache.get_or_refresh_infallible(Duration::from_secs(60), || {
            refresh_count += 1;
            "first".to_owned()
        });
        let second = cache.get_or_refresh_infallible(Duration::from_secs(60), || {
            refresh_count += 1;
            "second".to_owned()
        });

        assert_eq!(refresh_count, 1);
        assert_eq!(first, "first");
        assert_eq!(second, "first");
    }
}

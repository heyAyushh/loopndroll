use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::assistant::{AssistantAdapterCapability, AssistantKind, adapter_capabilities};
use crate::automations::{AutomationSummary, read_automations};
use crate::codex::{
    CodexServerOwner, CodexServerProcess, ControlPlaneStatus, HookOwner, StateData,
    ThreadCapabilities, ThreadRecord, capabilities_for_state_thread, inspect_control_plane,
    read_state,
};
use crate::compaction::{CompactionEvent, read_compaction_events, read_recent_compaction_events};
use crate::devin::{
    DevinAcpBridgeAttach, DevinAcpBridgeProbe, DevinAcpBridgeStatus, DevinDesktopStatus,
    DevinInstallationStatus, build_acp_bridge_attach, build_acp_bridge_probe,
    devin_connection_detail, devin_session_capabilities, devin_session_to_desktop_thread,
    devin_session_to_thread_record, discover_devin_sessions, inspect_devin_desktop_for_home,
};
use crate::events::{AutomationRunRecord, EventStore};
use crate::goals::{GoalSummary, read_goals};
use crate::grok_build::{
    GrokHookOwner, GrokHookStatus, discover_grok_sessions, grok_session_to_desktop_thread,
    inspect_grok_hooks, register_owned_grok_hooks, unregister_owned_grok_hooks,
};
use crate::hook_registration::{register_owned_hooks, unregister_owned_hooks};
use crate::mobile_auth::MobileAuthService;
use crate::mobile_events::{
    MobileEventHub, MobileEventInput, MobileEventKind, build_mobile_event, mobile_event_now,
};
use crate::mobile_push::MobilePushService;
use crate::mobile_session::MobileSessionService;
use crate::sync_manifest::SyncManifest;
use crate::telegram::TelegramService;
use crate::transcript_preview::latest_assistant_message_for_path;

const DESKTOP_COMPACTION_LIMIT: usize = 50;
const DESKTOP_COMPACTION_FILE_SCAN_LIMIT: usize = 250;
const DESKTOP_MENU_COMPACTION_LIMIT: usize = 10;
const DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT: usize = 50;
const DESKTOP_MENU_THREAD_LIMIT: usize = 12;
const CODEX_HOOKS_CONNECTION_ID: &str = "codex-hooks";
const CODEX_HOOKS_CONNECTION_LABEL: &str = "Codex hooks";
const CODEX_CONNECTION_KIND: &str = "codex";
const DEVIN_CONNECTION_KIND: &str = "devin";
const GROK_BUILD_CONNECTION_KIND: &str = "grok-build";
const GROK_BUILD_CONNECTION_ID: &str = "grok-build-cli";
const GROK_BUILD_CONNECTION_LABEL: &str = "Grok Build CLI";
const GROK_BUILD_CLI_RUNTIME_LABEL: &str = "Grok Build CLI";
const GROK_BUILD_SESSION_RUNTIME_LABEL: &str = "Grok Build session";
const MOBILE_CONNECTION_KIND: &str = "mobile";
const READ_ONLY_CONNECTION_ACTION_HINT: &str = "Detected from local Codex state.";
const DEVIN_CONNECTION_ACTION_HINT: &str =
    "Devin ACP status with explicit probe support; lifecycle control is not automatic.";
const GROK_BUILD_CONNECTION_ACTION_HINT: &str =
    "Grok Build hooks at ~/.grok/hooks/looper.json; sessions read from ~/.grok/sessions/.";
const GROK_BUILD_HOOKS_CONNECTION_ID: &str = "grok-build-hooks";
const GROK_BUILD_HOOKS_CONNECTION_LABEL: &str = "Grok Build hooks";

#[derive(Clone, Debug)]
pub struct ControlPlaneConfig {
    pub codex_home: PathBuf,
    pub grok_home: PathBuf,
    pub store_path: PathBuf,
    pub hook_command: Option<String>,
    pub home_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DevinAcpAttachSession {
    pub agent_id: String,
    pub agent_name: Option<String>,
    pub attached_at: String,
    pub status: String,
    pub detail: String,
}

#[derive(Clone)]
pub struct ControlPlane {
    config: ControlPlaneConfig,
    store: EventStore,
    mobile_events: MobileEventHub,
    devin_acp_attach: Arc<Mutex<Option<DevinAcpAttachSession>>>,
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
pub struct DevinDesktopResponse {
    pub status: DevinDesktopStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeResponse {
    pub bridge: DevinAcpBridgeStatus,
    pub attach_session: Option<DevinAcpAttachSession>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeProbeResponse {
    pub probe: DevinAcpBridgeProbe,
    pub bridge: DevinAcpBridgeStatus,
    pub attach_session: Option<DevinAcpAttachSession>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpBridgeAttachResponse {
    pub attach: DevinAcpBridgeAttach,
    pub bridge: DevinAcpBridgeStatus,
    pub attach_session: Option<DevinAcpAttachSession>,
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
    pub devin_desktop: DevinDesktopStatus,
    pub grok_build: GrokBuildStatus,
    pub compactions: Vec<CompactionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrokBuildStatus {
    pub hooks: GrokHookStatus,
    pub session_count: usize,
    pub active_session_count: usize,
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
    pub assistant_preview: Option<String>,
    pub archived: bool,
    pub capabilities: ThreadCapabilities,
}

impl ControlPlane {
    pub fn new(config: ControlPlaneConfig) -> Self {
        let store = EventStore::new(config.store_path.clone());
        Self {
            config,
            store,
            mobile_events: MobileEventHub::new(),
            devin_acp_attach: Arc::new(Mutex::new(None)),
        }
    }

    pub fn mobile_event_hub(&self) -> &MobileEventHub {
        &self.mobile_events
    }

    pub fn emit_mobile_event(&self, input: MobileEventInput) {
        let event = build_mobile_event(input);
        if let Err(error) = self.store.record_mobile_event(&event) {
            eprintln!("mobile event persistence failed: {error}");
        }
        self.mobile_events.publish(event);
    }

    pub fn devin_acp_attach_session(&self) -> Option<DevinAcpAttachSession> {
        self.devin_acp_attach
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
    }

    pub fn set_devin_acp_attach_session(&self, session: Option<DevinAcpAttachSession>) {
        if let Ok(mut guard) = self.devin_acp_attach.lock() {
            *guard = session;
        }
    }

    pub fn mobile_snapshot_revision(&self) -> Result<String> {
        let state = read_state(&self.config.codex_home)?;
        let session_state = self.mobile_session_service().state()?;
        let mut thread_signature =
            codex_threads_for_snapshot(&state.threads, Some(DESKTOP_MENU_THREAD_LIMIT))
                .iter()
                .map(|thread| {
                    format!(
                        "{}:{}:{}",
                        thread.thread_id,
                        thread.updated_at_ms.unwrap_or_default(),
                        thread.archived
                    )
                })
                .collect::<Vec<_>>();
        thread_signature.sort();
        let grok_sessions = discover_grok_sessions(&self.config.grok_home).unwrap_or_default();
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
        let devin_sessions = discover_devin_sessions(&self.config.home_path).unwrap_or_default();
        let mut devin_signature = devin_sessions
            .iter()
            .take(DESKTOP_MENU_THREAD_LIMIT)
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
        devin_signature.sort();
        let codex_active_thread_count = state
            .threads
            .iter()
            .filter(|thread| !thread.archived)
            .count();
        let codex_archived_thread_count = state
            .threads
            .len()
            .saturating_sub(codex_active_thread_count);
        let grok_active_thread_count = grok_sessions
            .iter()
            .filter(|session| session.running)
            .count();
        let devin_active_thread_count = devin_sessions
            .iter()
            .filter(|session| session.is_active())
            .count();
        let devin_archived_thread_count = devin_sessions
            .iter()
            .filter(|session| session.archived)
            .count();
        let queued_prompt_count = session_state
            .sessions
            .values()
            .filter(|session| !session.deleted)
            .count();
        Ok(format!(
            "threads={}:grok={}:devin={}:active={}:archived={}:overrides={}:surface={}",
            thread_signature.join("|"),
            grok_signature.join("|"),
            devin_signature.join("|"),
            codex_active_thread_count + grok_active_thread_count + devin_active_thread_count,
            codex_archived_thread_count + devin_archived_thread_count,
            queued_prompt_count,
            session_state.assistant_surface
        ))
    }

    pub fn codex_home(&self) -> &PathBuf {
        &self.config.codex_home
    }

    pub fn grok_home(&self) -> &PathBuf {
        &self.config.grok_home
    }

    pub fn store(&self) -> &EventStore {
        &self.store
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
        let status = inspect_devin_desktop_for_home(&self.config.home_path);
        status
            .installations
            .iter()
            .filter(|installation| devin_installation_should_render(installation))
            .map(|installation| devin_desktop_connection(installation, &status))
            .collect()
    }

    fn grok_build_connections(&self) -> Vec<ManagedConnection> {
        let hook_status = inspect_grok_hooks(&self.config.grok_home);
        let mut connections = grok_build_cli_connections_from_adapters(&adapter_capabilities());
        connections.push(grok_hook_connection(&hook_status));
        connections
    }

    pub fn status(&self) -> ControlPlaneStatus {
        inspect_control_plane(&self.config.codex_home)
    }

    pub fn codex_servers_response(&self) -> CodexServersResponse {
        CodexServersResponse {
            servers: self.status().codex_servers,
        }
    }

    pub fn managed_connections_response(&self) -> Result<ManagedConnectionsResponse> {
        let status = self.status();
        let mut connections = self.mobile_connections()?;
        connections.extend(self.devin_desktop_connections());
        connections.extend(self.grok_build_connections());
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
        self.managed_connections_response()
    }

    pub fn revoke_mobile_connection(
        &self,
        connection_id: &str,
    ) -> Result<ManagedConnectionsResponse> {
        self.mobile_auth_service()
            .revoke_mobile_connection(connection_id)?;
        self.managed_connections_response()
    }

    pub fn threads(&self) -> Result<Vec<ThreadRecord>> {
        let state = read_state(&self.config.codex_home)?;
        let mut threads = state.threads;
        threads.extend(
            discover_devin_sessions(&self.config.home_path)
                .unwrap_or_default()
                .iter()
                .map(devin_session_to_thread_record),
        );
        threads.extend(
            discover_grok_sessions(&self.config.grok_home)
                .unwrap_or_default()
                .iter()
                .map(grok_session_to_desktop_thread)
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
            adapters: adapter_capabilities(),
        }
    }

    pub fn devin_desktop_response(&self) -> DevinDesktopResponse {
        DevinDesktopResponse {
            status: inspect_devin_desktop_for_home(&self.config.home_path),
        }
    }

    pub fn devin_acp_bridge_response(&self) -> DevinAcpBridgeResponse {
        DevinAcpBridgeResponse {
            bridge: inspect_devin_desktop_for_home(&self.config.home_path).acp_bridge,
            attach_session: self.devin_acp_attach_session(),
        }
    }

    pub fn devin_acp_bridge_probe_response(
        &self,
        agent_id: Option<&str>,
    ) -> DevinAcpBridgeProbeResponse {
        let status = inspect_devin_desktop_for_home(&self.config.home_path);
        DevinAcpBridgeProbeResponse {
            probe: build_acp_bridge_probe(&status.installations, &status.acp_registry, agent_id),
            bridge: status.acp_bridge,
            attach_session: self.devin_acp_attach_session(),
        }
    }

    pub fn devin_acp_bridge_attach_response(
        &self,
        agent_id: Option<&str>,
    ) -> DevinAcpBridgeAttachResponse {
        let status = inspect_devin_desktop_for_home(&self.config.home_path);
        let attach = build_acp_bridge_attach(&status.installations, &status.acp_registry, agent_id);
        let attach_session = if attach.ok {
            let session = DevinAcpAttachSession {
                agent_id: attach.agent_id.clone().unwrap_or_default(),
                agent_name: attach.name.clone(),
                attached_at: mobile_event_now(),
                status: "attached-experimental".to_owned(),
                detail: attach.detail.clone(),
            };
            self.set_devin_acp_attach_session(Some(session.clone()));
            self.emit_mobile_event(MobileEventInput {
                kind: MobileEventKind::LifecycleChanged,
                thread_id: None,
                prompt_id: None,
                detail: Some(format!("devin-acp-attach:{}", session.agent_id)),
            });
            Some(session)
        } else {
            self.set_devin_acp_attach_session(None);
            None
        };
        DevinAcpBridgeAttachResponse {
            attach,
            bridge: status.acp_bridge,
            attach_session,
        }
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
            + unregister_owned_grok_hooks(&self.config.grok_home)?;
        let settings = self.store.set_hooks_auto_registration(false)?;
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
        let grok_change = register_owned_grok_hooks(&self.config.grok_home, hook_command)?;
        let settings = self.store.set_hooks_auto_registration(true)?;
        Ok(HookMutationResponse {
            action: "register-hooks".to_owned(),
            removed_handlers: codex_change.removed_handlers + grok_change.removed_handlers,
            installed_handlers: codex_change.installed_handlers + grok_change.installed_handlers,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn unregister_live_hooks(&self) -> Result<HookMutationResponse> {
        let removed_handlers = unregister_owned_hooks(&self.config.codex_home)?
            + unregister_owned_grok_hooks(&self.config.grok_home)?;
        let settings = self.store.service_settings()?;
        Ok(HookMutationResponse {
            action: "unregister-live-hooks".to_owned(),
            removed_handlers,
            installed_handlers: 0,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
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

        if let Some(session) = discover_devin_sessions(&self.config.home_path)
            .unwrap_or_default()
            .into_iter()
            .find(|session| session.thread_id == thread_id || session.session_id == thread_id)
        {
            return Ok(devin_session_capabilities(&session));
        }

        if let Some(session) = discover_grok_sessions(&self.config.grok_home)
            .unwrap_or_default()
            .into_iter()
            .find(|session| session.session_id == thread_id)
        {
            return Ok(grok_session_to_desktop_thread(&session).capabilities);
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
        let known_thread_ids = known_thread_ids(&threads);
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
            None,
            DESKTOP_COMPACTION_LIMIT,
            DESKTOP_COMPACTION_FILE_SCAN_LIMIT,
        )
    }

    pub fn desktop_menu_snapshot(&self) -> Result<DesktopSnapshot> {
        self.desktop_snapshot_with_limits(
            Some(DESKTOP_MENU_THREAD_LIMIT),
            DESKTOP_MENU_COMPACTION_LIMIT,
            DESKTOP_MENU_COMPACTION_FILE_SCAN_LIMIT,
        )
    }

    fn desktop_snapshot_with_limits(
        &self,
        thread_limit: Option<usize>,
        compaction_limit: usize,
        compaction_file_scan_limit: usize,
    ) -> Result<DesktopSnapshot> {
        let state = read_state(&self.config.codex_home)?;
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
                Ok(DesktopThread {
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
                    assistant_preview: thread
                        .transcript_path
                        .as_deref()
                        .and_then(|path| latest_assistant_message_for_path(Path::new(path))),
                    archived: thread.archived,
                    capabilities,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let grok_sessions = discover_grok_sessions(&self.config.grok_home).unwrap_or_default();
        let grok_build = GrokBuildStatus {
            hooks: inspect_grok_hooks(&self.config.grok_home),
            session_count: grok_sessions.len(),
            active_session_count: grok_sessions
                .iter()
                .filter(|session| session.running)
                .count(),
        };
        desktop_threads.extend(
            limited_items(&grok_sessions, thread_limit).map(grok_session_to_desktop_thread),
        );
        let devin_sessions = discover_devin_sessions(&self.config.home_path).unwrap_or_default();
        desktop_threads.extend(
            limited_items(&devin_sessions, thread_limit).map(devin_session_to_desktop_thread),
        );
        desktop_threads.sort_by(|left, right| {
            right
                .updated_at_ms
                .unwrap_or_default()
                .cmp(&left.updated_at_ms.unwrap_or_default())
        });
        let visible_codex_threads = snapshot_codex_threads;

        let compactions = read_recent_compaction_events(
            &self.config.codex_home,
            compaction_limit,
            compaction_file_scan_limit,
        )?;
        let mut known_thread_ids = known_thread_ids(&all_threads);
        known_thread_ids.extend(
            grok_sessions
                .iter()
                .map(|session| session.session_id.clone()),
        );
        known_thread_ids.extend(
            devin_sessions
                .iter()
                .map(|session| session.thread_id.clone()),
        );
        let goals = read_goals(&self.config.codex_home, &known_thread_ids)?;
        let automations = read_automations(&self.config.codex_home)?
            .into_iter()
            .map(|automation| automation.to_summary(&known_thread_ids))
            .collect::<Vec<_>>();
        let codex_active_thread_count =
            all_threads.iter().filter(|thread| !thread.archived).count();
        let codex_archived_thread_count =
            all_threads.len().saturating_sub(codex_active_thread_count);
        let grok_active_thread_count = grok_build.active_session_count;
        let devin_active_thread_count = devin_sessions
            .iter()
            .filter(|session| session.is_active())
            .count();
        let devin_archived_thread_count = devin_sessions
            .iter()
            .filter(|session| session.archived)
            .count();
        let active_thread_count =
            codex_active_thread_count + grok_active_thread_count + devin_active_thread_count;
        let archived_thread_count = codex_archived_thread_count + devin_archived_thread_count;
        let sync_manifest = SyncManifest::metadata_only(
            &self.status(),
            &goals,
            &automations,
            &visible_codex_threads,
            &capabilities,
        )?;

        Ok(DesktopSnapshot {
            control_plane: self.status(),
            thread_count: all_threads.len() + grok_build.session_count + devin_sessions.len(),
            active_thread_count,
            archived_thread_count,
            threads: desktop_threads,
            automations,
            automation_runs: self.store.automation_runs()?,
            goals,
            sync_manifest,
            assistant_adapters: adapter_capabilities(),
            devin_desktop: inspect_devin_desktop_for_home(&self.config.home_path),
            grok_build,
            compactions,
        })
    }

    pub fn record_automation_fire(
        &self,
        automation_id: &str,
        target_thread_id: Option<&str>,
        scheduled_at_ms: i64,
        fired_at_ms: i64,
        detail: Option<&str>,
    ) -> Result<Option<AutomationRunRecord>> {
        self.store.record_automation_run(
            automation_id,
            target_thread_id,
            scheduled_at_ms,
            fired_at_ms,
            detail,
        )
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

    fn known_desktop_thread_ids(&self) -> Result<BTreeSet<String>> {
        let state = read_state(&self.config.codex_home)?;
        let mut thread_ids = known_thread_ids(&state.threads);
        thread_ids.extend(
            discover_devin_sessions(&self.config.home_path)
                .unwrap_or_default()
                .into_iter()
                .map(|session| session.thread_id),
        );
        thread_ids.extend(
            discover_grok_sessions(&self.config.grok_home)
                .unwrap_or_default()
                .into_iter()
                .map(|session| session.session_id),
        );
        Ok(thread_ids)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assistant::{AssistantRuntime, AssistantRuntimeKind};

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
    fn grok_build_managed_connections_always_include_hooks_row() {
        let fixture_dir = tempfile::tempdir().expect("tempdir");
        let control_plane = ControlPlane::new(ControlPlaneConfig {
            codex_home: fixture_dir.path().join(".codex"),
            grok_home: fixture_dir.path().join(".grok"),
            store_path: fixture_dir.path().join("store.sqlite"),
            hook_command: None,
            home_path: fixture_dir.path().to_path_buf(),
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
    }
}

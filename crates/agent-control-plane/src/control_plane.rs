use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::assistant::{AssistantAdapterCapability, adapter_capabilities};
use crate::automations::{AutomationSummary, read_automations};
use crate::codex::{
    CodexServerProcess, ControlPlaneStatus, StateData, ThreadCapabilities, ThreadRecord,
    capabilities_for_state_thread, capabilities_for_thread, inspect_control_plane, read_state,
};
use crate::compaction::{CompactionEvent, read_compaction_events, read_recent_compaction_events};
use crate::events::{AutomationRunRecord, EventStore};
use crate::goals::{GoalSummary, read_goals};
use crate::hook_registration::{register_owned_hooks, unregister_owned_hooks};
use crate::sync_manifest::SyncManifest;

const DESKTOP_COMPACTION_LIMIT: usize = 50;
const DESKTOP_COMPACTION_FILE_SCAN_LIMIT: usize = 250;

#[derive(Clone, Debug)]
pub struct ControlPlaneConfig {
    pub codex_home: PathBuf,
    pub store_path: PathBuf,
    pub hook_command: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ControlPlane {
    config: ControlPlaneConfig,
    store: EventStore,
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
pub struct CompactionsResponse {
    pub events: Vec<CompactionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexServersResponse {
    pub servers: Vec<CodexServerProcess>,
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
    pub compactions: Vec<CompactionEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesktopThread {
    pub thread_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub source: Option<String>,
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
    pub archived: bool,
    pub capabilities: ThreadCapabilities,
}

impl ControlPlane {
    pub fn new(config: ControlPlaneConfig) -> Self {
        let store = EventStore::new(config.store_path.clone());
        Self { config, store }
    }

    pub fn codex_home(&self) -> &PathBuf {
        &self.config.codex_home
    }

    pub fn store(&self) -> &EventStore {
        &self.store
    }

    pub fn status(&self) -> ControlPlaneStatus {
        inspect_control_plane(&self.config.codex_home)
    }

    pub fn codex_servers_response(&self) -> CodexServersResponse {
        CodexServersResponse {
            servers: self.status().codex_servers,
        }
    }

    pub fn threads(&self) -> Result<Vec<ThreadRecord>> {
        Ok(read_state(&self.config.codex_home)?.threads)
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

    pub fn compactions(&self) -> Result<Vec<CompactionEvent>> {
        read_compaction_events(&self.config.codex_home)
    }

    pub fn compactions_response(&self) -> Result<CompactionsResponse> {
        Ok(CompactionsResponse {
            events: self.compactions()?,
        })
    }

    pub fn unregister_hooks(&self) -> Result<HookMutationResponse> {
        let removed_handlers = unregister_owned_hooks(&self.config.codex_home)?;
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
        let change = register_owned_hooks(&self.config.codex_home, hook_command)?;
        let settings = self.store.set_hooks_auto_registration(true)?;
        Ok(HookMutationResponse {
            action: "register-hooks".to_owned(),
            removed_handlers: change.removed_handlers,
            installed_handlers: change.installed_handlers,
            hooks_auto_registration: settings.hooks_auto_registration,
            status: self.status(),
        })
    }

    pub fn unregister_live_hooks(&self) -> Result<HookMutationResponse> {
        let removed_handlers = unregister_owned_hooks(&self.config.codex_home)?;
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
        capabilities_for_thread(&self.config.codex_home, thread_id)
    }

    pub fn automations(&self) -> Result<Vec<AutomationSummary>> {
        let known_thread_ids = self
            .threads()?
            .into_iter()
            .map(|thread| thread.thread_id)
            .collect::<BTreeSet<_>>();
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
        let known_thread_ids = self
            .threads()?
            .into_iter()
            .map(|thread| thread.thread_id)
            .collect::<BTreeSet<_>>();
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

    fn desktop_snapshot_with_limits(
        &self,
        thread_limit: Option<usize>,
        compaction_limit: usize,
        compaction_file_scan_limit: usize,
    ) -> Result<DesktopSnapshot> {
        let state = read_state(&self.config.codex_home)?;
        let all_threads = state.threads.clone();
        let visible_threads = all_threads
            .iter()
            .take(thread_limit.unwrap_or(usize::MAX))
            .cloned()
            .collect::<Vec<_>>();
        let capabilities = self.capabilities_by_threads(&state, &visible_threads);
        let desktop_threads = visible_threads
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
                    source: thread.source.clone(),
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
                    capabilities,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let compactions = read_recent_compaction_events(
            &self.config.codex_home,
            compaction_limit,
            compaction_file_scan_limit,
        )?;
        let known_thread_ids = known_thread_ids(&all_threads);
        let goals = read_goals(&self.config.codex_home, &known_thread_ids)?;
        let automations = read_automations(&self.config.codex_home)?
            .into_iter()
            .map(|automation| automation.to_summary(&known_thread_ids))
            .collect::<Vec<_>>();
        let active_thread_count = all_threads.iter().filter(|thread| !thread.archived).count();
        let archived_thread_count = all_threads.len().saturating_sub(active_thread_count);
        let sync_manifest = SyncManifest::metadata_only(
            &self.status(),
            &goals,
            &automations,
            &visible_threads,
            &capabilities,
        )?;

        Ok(DesktopSnapshot {
            control_plane: self.status(),
            thread_count: all_threads.len(),
            active_thread_count,
            archived_thread_count,
            threads: desktop_threads,
            automations,
            automation_runs: self.store.automation_runs()?,
            goals,
            sync_manifest,
            assistant_adapters: adapter_capabilities(),
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
}

fn known_thread_ids(threads: &[ThreadRecord]) -> BTreeSet<String> {
    threads
        .iter()
        .map(|thread| thread.thread_id.clone())
        .collect()
}

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::automations::{AutomationKind, AutomationStatus, AutomationSummary};
use crate::codex::{ControlPlaneStatus, HookOwner, LaunchKind, ThreadCapabilities, ThreadRecord};
use crate::goals::{GoalStatus, GoalSummary};

const SYNC_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncManifest {
    pub schema_version: u32,
    pub manifest_id: String,
    pub generated_at_ms: i64,
    pub privacy: SyncPrivacy,
    pub goals: Vec<SyncGoal>,
    pub automations: Vec<SyncAutomation>,
    pub threads: Vec<SyncThread>,
    pub hooks: SyncHookSummary,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncPrivacy {
    pub profile: String,
    pub raw_goal_bodies: bool,
    pub raw_automation_prompts: bool,
    pub raw_thread_logs: bool,
    pub credentials: bool,
    pub source_paths: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncGoal {
    pub id: String,
    pub title: String,
    pub status: GoalStatus,
    pub lifecycle: GoalStatus,
    pub running: bool,
    pub priority: Option<String>,
    pub target_thread_id: Option<String>,
    pub target_known: bool,
    pub updated_at_ms: Option<i64>,
    pub token_budget: Option<i64>,
    pub tokens_used: Option<i64>,
    pub time_used_seconds: Option<i64>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncAutomation {
    pub id: String,
    pub kind: AutomationKind,
    pub name: String,
    pub status: AutomationStatus,
    pub rrule: String,
    pub target_thread_id: Option<String>,
    pub target_known: bool,
    pub control_plane_covered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncThread {
    pub thread_id: String,
    pub updated_at_ms: Option<i64>,
    pub archived: bool,
    pub launch_kind: LaunchKind,
    pub parent_thread_id: Option<String>,
    pub child_count: usize,
    pub mcp_tool_count: usize,
    pub app_tool_count: usize,
    pub automation_tool_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncHookSummary {
    pub enabled: bool,
    pub owner: HookOwner,
    pub health: String,
    pub registered_events: Vec<String>,
}

impl SyncManifest {
    pub fn metadata_only(
        status: &ControlPlaneStatus,
        goals: &[GoalSummary],
        automations: &[AutomationSummary],
        threads: &[ThreadRecord],
        capabilities: &BTreeMap<String, ThreadCapabilities>,
    ) -> Result<Self> {
        let goals = goals.iter().map(SyncGoal::from).collect::<Vec<_>>();
        let automations = automations
            .iter()
            .map(SyncAutomation::from)
            .collect::<Vec<_>>();
        let threads = threads
            .iter()
            .map(|thread| SyncThread::from_thread(thread, capabilities.get(&thread.thread_id)))
            .collect::<Vec<_>>();
        let hooks = SyncHookSummary::from_status(status);
        let manifest_id = manifest_id(&goals, &automations, &threads, &hooks)?;

        Ok(Self {
            schema_version: SYNC_SCHEMA_VERSION,
            manifest_id,
            generated_at_ms: now_ms(),
            privacy: SyncPrivacy::metadata_only(),
            goals,
            automations,
            threads,
            hooks,
        })
    }
}

impl SyncPrivacy {
    pub fn metadata_only() -> Self {
        Self {
            profile: "metadata-only".to_owned(),
            raw_goal_bodies: false,
            raw_automation_prompts: false,
            raw_thread_logs: false,
            credentials: false,
            source_paths: false,
        }
    }
}

impl From<&GoalSummary> for SyncGoal {
    fn from(goal: &GoalSummary) -> Self {
        Self {
            id: goal.id.clone(),
            title: goal.title.clone(),
            status: goal.status.clone(),
            lifecycle: goal.lifecycle.clone(),
            running: goal.running,
            priority: goal.priority.clone(),
            target_thread_id: goal.target_thread_id.clone(),
            target_known: goal.target_known,
            updated_at_ms: goal.updated_at_ms,
            token_budget: goal.token_budget,
            tokens_used: goal.tokens_used,
            time_used_seconds: goal.time_used_seconds,
            content_hash: goal.content_hash.clone(),
        }
    }
}

impl From<&AutomationSummary> for SyncAutomation {
    fn from(automation: &AutomationSummary) -> Self {
        Self {
            id: automation.id.clone(),
            kind: automation.kind.clone(),
            name: automation.name.clone(),
            status: automation.status.clone(),
            rrule: automation.rrule.clone(),
            target_thread_id: automation.target_thread_id.clone(),
            target_known: automation.target_known,
            control_plane_covered: automation.control_plane_covered,
        }
    }
}

impl SyncThread {
    fn from_thread(thread: &ThreadRecord, capabilities: Option<&ThreadCapabilities>) -> Self {
        Self {
            thread_id: thread.thread_id.clone(),
            updated_at_ms: thread.updated_at_ms,
            archived: thread.archived,
            launch_kind: capabilities
                .map(|capabilities| capabilities.spawn.launch_kind.clone())
                .unwrap_or(LaunchKind::Main),
            parent_thread_id: capabilities
                .and_then(|capabilities| capabilities.spawn.parent_thread_id.clone()),
            child_count: capabilities
                .map(|capabilities| capabilities.spawn.children.len())
                .unwrap_or_default(),
            mcp_tool_count: capabilities
                .map(|capabilities| capabilities.mcp_tools.len())
                .unwrap_or_default(),
            app_tool_count: capabilities
                .map(|capabilities| capabilities.app_tools.len())
                .unwrap_or_default(),
            automation_tool_count: capabilities
                .map(|capabilities| capabilities.automation_tools.len())
                .unwrap_or_default(),
        }
    }
}

impl SyncHookSummary {
    fn from_status(status: &ControlPlaneStatus) -> Self {
        Self {
            enabled: status.hooks.enabled,
            owner: status.hooks.owner.clone(),
            health: status.hooks.health.clone(),
            registered_events: status.hooks.registered_events.clone(),
        }
    }
}

fn manifest_id(
    goals: &[SyncGoal],
    automations: &[SyncAutomation],
    threads: &[SyncThread],
    hooks: &SyncHookSummary,
) -> Result<String> {
    let fingerprint = serde_json::to_vec(&json!({
        "schema_version": SYNC_SCHEMA_VERSION,
        "goals": goals,
        "automations": automations,
        "threads": threads,
        "hooks": hooks,
    }))?;
    Ok(format!("sync-{}", sha256_hex(&fingerprint)))
}

fn now_ms() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn sha256_hex(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

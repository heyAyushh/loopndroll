// allow: SIZE_OK — Codex session inventory boundary coordinates state DB, transcript, rollout, and spawn metadata truth sources.
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{Date, Month, PrimitiveDateTime, Time};

use crate::assistant::{
    AssistantKind, assistant_kind_from_client, infer_assistant_client_from_paths,
};
use crate::hook_registration::{LOOPER_HOOK_MARKER, owned_hook_state_keys_for_hooks_path};
use crate::privacy::redact_command_for_display;

const MAX_PROCESS_ANCESTOR_DEPTH: usize = 8;
const DEVIN_DESKTOP_PROCESS_NEEDLES: &[&str] = &[
    "/applications/devin.app/",
    "/applications/devin - next.app/",
    ".devin-next",
    "devin - next helper",
    "devin-desktop",
    "devin desktop",
];
const SUPERCONDUCTOR_PROCESS_NEEDLES: &[&str] = &["superconductor", ".superconductor"];
const CURSOR_PROCESS_NEEDLES: &[&str] = &["/cursor.app/", ".cursor/extensions", "cursor --type"];
const CODEX_APP_PROCESS_NEEDLES: &[&str] = &[
    "/applications/codex.app/",
    "codex.app/contents/",
    "com.openai.codex",
];
const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const ROLLOUT_FILENAME_TIMESTAMP_LENGTH: usize = 19;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;
const BOUNDED_ROLLOUT_REFRESH_MULTIPLIER: usize = 4;
const MIN_BOUNDED_ROLLOUT_REFRESH_CANDIDATES: usize = 64;
const ROLLOUT_SESSION_META_SCAN_LINE_LIMIT: usize = 2_000;
const BYTES_PER_KIBIBYTE: u64 = 1_024;
const ROLLOUT_SESSION_META_SCAN_BYTE_LIMIT: u64 = 256 * BYTES_PER_KIBIBYTE;
const SESSION_INDEX_FILENAME: &str = "session_index.jsonl";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlPlaneStatus {
    pub hooks: HookStatus,
    pub app_server: Option<ProcessSummary>,
    pub codex_servers: Vec<CodexServerProcess>,
    pub source: SourceStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceStatus {
    pub codex_home: String,
    pub state_db: Option<String>,
    pub logs_db: Option<String>,
    pub sessions_root: String,
    pub health: String,
    pub degraded_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookStatus {
    pub enabled: bool,
    pub registered_events: Vec<String>,
    pub active_command: Option<String>,
    pub owner: HookOwner,
    pub health: String,
    pub issues: Vec<String>,
    pub recent_failures_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HookOwner {
    LooperRust,
    Unknown,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessSummary {
    pub pid: i32,
    pub parent_pid: Option<i32>,
    pub tty: Option<String>,
    pub executable: String,
    pub command: String,
    pub parent_processes: Vec<ProcessAncestor>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessAncestor {
    pub pid: i32,
    pub parent_pid: Option<i32>,
    pub executable: String,
    pub command: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodexServerProcess {
    pub pid: i32,
    pub parent_pid: Option<i32>,
    pub tty: Option<String>,
    pub executable: String,
    pub command: String,
    pub owner: CodexServerOwner,
    pub parent_processes: Vec<ProcessAncestor>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CodexServerOwner {
    CodexApp,
    CodexCli,
    Cursor,
    DevinDesktop,
    Superconductor,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadRecord {
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
    pub archived: bool,
}

#[derive(Debug, Deserialize)]
struct RolloutRecord {
    #[serde(rename = "type")]
    record_type: Option<String>,
    payload: Option<RolloutPayload>,
}

#[derive(Debug, Deserialize)]
struct RolloutPayload {
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SessionIndexRecord {
    id: Option<String>,
    thread_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpawnGraph {
    pub parent_thread_id: Option<String>,
    pub root_thread_id: String,
    pub children: Vec<String>,
    pub launch_kind: LaunchKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LaunchKind {
    Main,
    Subagent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DynamicTool {
    pub name: String,
    pub namespace: Option<String>,
    pub description: Option<String>,
    pub defer_loading: bool,
    pub classification: ToolClassification,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolClassification {
    BuiltInLocal,
    Mcp,
    App,
    Automation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffSummary {
    pub git_branch: Option<String>,
    pub git_sha: Option<String>,
    pub produced_file_changes: bool,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadCapabilities {
    pub thread_id: String,
    pub assistant_kind: AssistantKind,
    pub tools: Vec<DynamicTool>,
    pub mcp_tools: Vec<String>,
    pub app_tools: Vec<String>,
    pub automation_tools: Vec<String>,
    pub spawn: SpawnGraph,
    pub diff: DiffSummary,
    pub agent_nickname: Option<String>,
    pub agent_role: Option<String>,
    pub agent_path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CodexSources {
    pub codex_home: PathBuf,
    pub state_db: Option<PathBuf>,
    pub logs_db: Option<PathBuf>,
    pub sessions_root: PathBuf,
    pub session_index: PathBuf,
}

#[derive(Clone, Debug)]
pub struct StateData {
    pub threads: Vec<ThreadRecord>,
    pub dynamic_tools_by_thread: BTreeMap<String, Vec<DynamicTool>>,
    pub spawn_edges: Vec<SpawnEdge>,
    pub known_thread_ids: BTreeSet<String>,
    pub total_thread_count: usize,
    pub active_thread_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadRevisionState {
    pub threads: Vec<ThreadRevisionRecord>,
    pub total_thread_count: usize,
    pub active_thread_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadRevisionRecord {
    pub thread_id: String,
    pub transcript_path: Option<String>,
    pub updated_at_ms: Option<i64>,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnEdge {
    pub parent_thread_id: String,
    pub child_thread_id: String,
    pub status: Option<String>,
}

pub fn discover_sources(codex_home: &Path) -> CodexSources {
    CodexSources {
        codex_home: codex_home.to_path_buf(),
        state_db: latest_matching_file(codex_home, "state_", ".sqlite"),
        logs_db: latest_matching_file(codex_home, "logs_", ".sqlite"),
        sessions_root: codex_home.join("sessions"),
        session_index: codex_home.join(SESSION_INDEX_FILENAME),
    }
}

pub fn source_status(codex_home: &Path) -> SourceStatus {
    let sources = discover_sources(codex_home);
    let degraded_reason = if sources.state_db.is_none() {
        Some("missing Codex state DB".to_owned())
    } else {
        None
    };
    SourceStatus {
        codex_home: sources.codex_home.display().to_string(),
        state_db: sources.state_db.map(|path| path.display().to_string()),
        logs_db: sources.logs_db.map(|path| path.display().to_string()),
        sessions_root: sources.sessions_root.display().to_string(),
        health: if degraded_reason.is_some() {
            "degraded".to_owned()
        } else {
            "healthy".to_owned()
        },
        degraded_reason,
    }
}

pub fn inspect_control_plane(codex_home: &Path) -> ControlPlaneStatus {
    let codex_servers = find_codex_server_processes().unwrap_or_default();
    ControlPlaneStatus {
        hooks: inspect_hooks(codex_home),
        app_server: codex_servers
            .first()
            .map(CodexServerProcess::as_process_summary),
        codex_servers,
        source: source_status(codex_home),
    }
}

pub fn inspect_hooks(codex_home: &Path) -> HookStatus {
    let config_path = codex_home.join("config.toml");
    let enabled = read_hooks_enabled(&config_path).unwrap_or(false);
    let hooks_path = codex_home.join("hooks.json");
    let mut issues = Vec::new();
    let (registered_events, active_command) = match read_hooks_json(&hooks_path) {
        Ok(value) => value,
        Err(error) => {
            issues.push(error.to_string());
            (Vec::new(), None)
        }
    };
    match read_disabled_owned_hook_states(&config_path, &hooks_path) {
        Ok(disabled_states) => {
            issues.extend(
                disabled_states
                    .into_iter()
                    .map(|state| format!("Codex disabled Looper hook state {state}")),
            );
        }
        Err(error) => issues.push(error.to_string()),
    }
    let owner = classify_hook_owner(active_command.as_deref());
    let health = if enabled
        && issues.is_empty()
        && !registered_events.is_empty()
        && owner != HookOwner::Unknown
    {
        "healthy"
    } else if enabled {
        "degraded"
    } else {
        "disabled"
    }
    .to_owned();

    HookStatus {
        enabled,
        registered_events,
        active_command,
        owner,
        health,
        issues,
        recent_failures_count: 0,
    }
}

pub fn read_state(codex_home: &Path) -> Result<StateData> {
    read_state_with_thread_limit(codex_home, None)
}

pub fn read_snapshot_state_with_thread_limit(
    codex_home: &Path,
    thread_limit: Option<usize>,
) -> Result<StateData> {
    read_state_with_options(codex_home, thread_limit, false)
}

pub fn read_thread_revision_state(
    codex_home: &Path,
    thread_limit: usize,
) -> Result<ThreadRevisionState> {
    let sources = discover_sources(codex_home);
    let Some(state_db) = sources.state_db else {
        return Ok(ThreadRevisionState {
            threads: Vec::new(),
            total_thread_count: 0,
            active_thread_count: 0,
        });
    };
    let connection = Connection::open(&state_db)
        .with_context(|| format!("open Codex state DB {}", state_db.display()))?;
    let threads = read_thread_revision_records(&connection, thread_limit)?;
    let (total_thread_count, active_thread_count) = read_thread_counts(&connection)?;
    Ok(ThreadRevisionState {
        threads,
        total_thread_count,
        active_thread_count,
    })
}

pub fn read_state_with_thread_limit(
    codex_home: &Path,
    thread_limit: Option<usize>,
) -> Result<StateData> {
    read_state_with_options(codex_home, thread_limit, true)
}

fn read_state_with_options(
    codex_home: &Path,
    thread_limit: Option<usize>,
    refresh_rollout_paths_on_request: bool,
) -> Result<StateData> {
    let sources = discover_sources(codex_home);
    let Some(state_db) = sources.state_db else {
        return Ok(StateData {
            threads: Vec::new(),
            dynamic_tools_by_thread: BTreeMap::new(),
            spawn_edges: Vec::new(),
            known_thread_ids: BTreeSet::new(),
            total_thread_count: 0,
            active_thread_count: 0,
        });
    };
    let connection = Connection::open(&state_db)
        .with_context(|| format!("open Codex state DB {}", state_db.display()))?;
    let mut threads = if refresh_rollout_paths_on_request {
        let mut threads = read_threads(&connection, None)?;
        refresh_thread_rollout_paths(
            &mut threads,
            &sources.sessions_root,
            thread_limit.map(bounded_rollout_refresh_candidate_limit),
        );
        if let Some(limit) = thread_limit {
            threads = latest_thread_records(threads, limit);
        }
        threads
    } else {
        read_threads(&connection, thread_limit)?
    };
    apply_session_index_titles(&mut threads, &sources.session_index);
    let selected_thread_ids = threads
        .iter()
        .map(|thread| thread.thread_id.clone())
        .collect::<BTreeSet<_>>();
    let dynamic_tools_by_thread = match thread_limit {
        Some(_) => read_dynamic_tools_for_threads(&connection, &selected_thread_ids)?,
        None => read_dynamic_tools(&connection)?,
    };
    let spawn_edges = match thread_limit {
        Some(_) => read_spawn_edges_for_threads(&connection, &selected_thread_ids)?,
        None => read_spawn_edges(&connection)?,
    };
    let known_thread_ids = read_thread_ids(&connection)?;
    let (total_thread_count, active_thread_count) = read_thread_counts(&connection)?;
    Ok(StateData {
        threads,
        dynamic_tools_by_thread,
        spawn_edges,
        known_thread_ids,
        total_thread_count,
        active_thread_count,
    })
}

pub fn capabilities_for_thread(codex_home: &Path, thread_id: &str) -> Result<ThreadCapabilities> {
    let state = read_state(codex_home)?;
    Ok(capabilities_for_state_thread(&state, thread_id))
}

pub fn capabilities_for_state_thread(state: &StateData, thread_id: &str) -> ThreadCapabilities {
    let thread = state
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id);
    let tools = state
        .dynamic_tools_by_thread
        .get(thread_id)
        .cloned()
        .unwrap_or_default();
    let spawn = build_spawn_graph(thread_id, &state.spawn_edges);
    let mcp_tools = tool_names(&tools, ToolClassification::Mcp);
    let app_tools = tool_names(&tools, ToolClassification::App);
    let automation_tools = tool_names(&tools, ToolClassification::Automation);
    let assistant_kind = thread
        .map(|thread| {
            assistant_kind_from_client(infer_assistant_client_from_paths(
                thread.transcript_path.as_deref(),
                thread.cwd.as_deref(),
                thread.source.as_deref(),
                thread.originator.as_deref(),
                thread.agent_path.as_deref(),
            ))
        })
        .unwrap_or(AssistantKind::Codex);

    ThreadCapabilities {
        thread_id: thread_id.to_owned(),
        assistant_kind,
        mcp_tools,
        app_tools,
        automation_tools,
        tools,
        spawn,
        diff: DiffSummary {
            git_branch: state
                .threads
                .iter()
                .find(|thread| thread.thread_id == thread_id)
                .and_then(|thread| thread.git_branch.clone()),
            git_sha: state
                .threads
                .iter()
                .find(|thread| thread.thread_id == thread_id)
                .and_then(|thread| thread.git_sha.clone()),
            produced_file_changes: false,
            paths: Vec::new(),
        },
        agent_nickname: state
            .threads
            .iter()
            .find(|thread| thread.thread_id == thread_id)
            .and_then(|thread| thread.agent_nickname.clone()),
        agent_role: state
            .threads
            .iter()
            .find(|thread| thread.thread_id == thread_id)
            .and_then(|thread| thread.agent_role.clone()),
        agent_path: state
            .threads
            .iter()
            .find(|thread| thread.thread_id == thread_id)
            .and_then(|thread| thread.agent_path.clone()),
    }
}

pub fn build_spawn_graph(thread_id: &str, edges: &[SpawnEdge]) -> SpawnGraph {
    let parent_thread_id = edges
        .iter()
        .find(|edge| edge.child_thread_id == thread_id)
        .map(|edge| edge.parent_thread_id.clone());
    let children = edges
        .iter()
        .filter(|edge| edge.parent_thread_id == thread_id)
        .map(|edge| edge.child_thread_id.clone())
        .collect::<Vec<_>>();
    let root_thread_id = root_thread_id_for_spawn_graph(thread_id, edges);
    SpawnGraph {
        parent_thread_id,
        root_thread_id,
        children,
        launch_kind: if edges.iter().any(|edge| edge.child_thread_id == thread_id) {
            LaunchKind::Subagent
        } else {
            LaunchKind::Main
        },
    }
}

fn root_thread_id_for_spawn_graph(thread_id: &str, edges: &[SpawnEdge]) -> String {
    let mut root_thread_id = thread_id.to_owned();
    let mut seen = BTreeSet::from([root_thread_id.clone()]);
    while let Some(parent_thread_id) = edges
        .iter()
        .find(|edge| edge.child_thread_id == root_thread_id)
        .map(|edge| edge.parent_thread_id.clone())
    {
        if !seen.insert(parent_thread_id.clone()) {
            break;
        }
        root_thread_id = parent_thread_id;
    }
    root_thread_id
}

fn read_threads(connection: &Connection, limit: Option<usize>) -> Result<Vec<ThreadRecord>> {
    if !table_exists(connection, "threads")? {
        return Ok(Vec::new());
    }
    let columns = table_columns(connection, "threads")?;
    let id_column = preferred_column(&columns, &["thread_id", "id"]).unwrap_or("id");
    let created_column = preferred_column(&columns, &["created_at_ms", "created_at"]);
    let updated_column = preferred_column(&columns, &["updated_at_ms", "updated_at"]);
    let order_column = updated_column.unwrap_or(id_column);
    let sql = format!(
        "select
            {id} as thread_id,
            {title} as title,
            {cwd} as cwd,
            {transcript_path} as transcript_path,
            {source} as source,
            {model} as model,
            {reasoning} as reasoning_effort,
            {git_sha} as git_sha,
            {git_branch} as git_branch,
            {cli_version} as cli_version,
            {agent_nickname} as agent_nickname,
            {agent_role} as agent_role,
            {agent_path} as agent_path,
            {created} as created_at_ms,
            {updated} as updated_at_ms,
            {archived} as archived
         from threads
         order by {order} desc{limit_clause}",
        id = quoted_identifier(id_column),
        title = nullable_column(&columns, "title"),
        cwd = nullable_column(&columns, "cwd"),
        transcript_path = nullable_column(&columns, "rollout_path"),
        source = nullable_column(&columns, "source"),
        model = nullable_column(&columns, "model"),
        reasoning = nullable_column(&columns, "reasoning_effort"),
        git_sha = nullable_column(&columns, "git_sha"),
        git_branch = nullable_column(&columns, "git_branch"),
        cli_version = nullable_column(&columns, "cli_version"),
        agent_nickname = nullable_column(&columns, "agent_nickname"),
        agent_role = nullable_column(&columns, "agent_role"),
        agent_path = nullable_column(&columns, "agent_path"),
        created = created_column
            .map(quoted_identifier)
            .unwrap_or_else(|| "null".to_owned()),
        updated = updated_column
            .map(quoted_identifier)
            .unwrap_or_else(|| "null".to_owned()),
        archived = nullable_column(&columns, "archived"),
        order = quoted_identifier(order_column),
        limit_clause = limit
            .map(|limit| format!(" limit {limit}"))
            .unwrap_or_default(),
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        let archived: Option<i64> = row.get(15)?;
        let transcript_path: Option<String> = row.get(3)?;
        let originator = transcript_path
            .as_deref()
            .and_then(transcript_originator_for_path);
        Ok(ThreadRecord {
            thread_id: row.get(0)?,
            title: row.get(1)?,
            cwd: row.get(2)?,
            transcript_path,
            source: row.get(4)?,
            originator,
            model: row.get(5)?,
            reasoning_effort: row.get(6)?,
            git_sha: row.get(7)?,
            git_branch: row.get(8)?,
            cli_version: row.get(9)?,
            agent_nickname: row.get(10)?,
            agent_role: row.get(11)?,
            agent_path: row.get(12)?,
            created_at_ms: row.get(13)?,
            updated_at_ms: row.get(14)?,
            archived: archived.unwrap_or(0) != 0,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_thread_revision_records(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<ThreadRevisionRecord>> {
    if !table_exists(connection, "threads")? {
        return Ok(Vec::new());
    }
    let columns = table_columns(connection, "threads")?;
    let id_column = preferred_column(&columns, &["thread_id", "id"]).unwrap_or("id");
    let updated_column = preferred_column(&columns, &["updated_at_ms", "updated_at"]);
    let order_column = updated_column.unwrap_or(id_column);
    let sql = format!(
        "select
            {id} as thread_id,
            {transcript_path} as transcript_path,
            {updated} as updated_at_ms,
            {archived} as archived
         from threads
         order by {order} desc
         limit {limit}",
        id = quoted_identifier(id_column),
        transcript_path = nullable_column(&columns, "rollout_path"),
        updated = updated_column
            .map(quoted_identifier)
            .unwrap_or_else(|| "null".to_owned()),
        archived = nullable_column(&columns, "archived"),
        order = quoted_identifier(order_column),
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| {
        let archived: Option<i64> = row.get(3)?;
        Ok(ThreadRevisionRecord {
            thread_id: row.get(0)?,
            transcript_path: row.get(1)?,
            updated_at_ms: row.get(2)?,
            archived: archived.unwrap_or(0) != 0,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn bounded_rollout_refresh_candidate_limit(thread_limit: usize) -> usize {
    thread_limit
        .saturating_mul(BOUNDED_ROLLOUT_REFRESH_MULTIPLIER)
        .max(MIN_BOUNDED_ROLLOUT_REFRESH_CANDIDATES)
}

fn refresh_thread_rollout_paths(
    threads: &mut [ThreadRecord],
    sessions_root: &Path,
    candidate_limit: Option<usize>,
) {
    if threads.is_empty() || !sessions_root.is_dir() {
        return;
    }

    let selected_thread_ids = threads
        .iter()
        .map(|thread| thread.thread_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut latest_rollouts_by_thread = BTreeMap::<String, RolloutPathCandidate>::new();

    let candidates = rollout_paths_by_freshness(sessions_root);
    let candidate_limit = candidate_limit.unwrap_or(candidates.len());
    for candidate in candidates.into_iter().take(candidate_limit) {
        for session_id in rollout_session_ids(&candidate.path) {
            if !selected_thread_ids.contains(session_id.as_str()) {
                continue;
            }
            latest_rollouts_by_thread
                .entry(session_id)
                .or_insert_with(|| candidate.clone());
        }
        if latest_rollouts_by_thread.len() == selected_thread_ids.len() {
            break;
        }
    }

    for thread in threads {
        let Some(candidate) = latest_rollouts_by_thread.get(&thread.thread_id) else {
            continue;
        };
        let current_freshness_at_ms = thread
            .transcript_path
            .as_deref()
            .and_then(|path| rollout_path_freshness_at_ms(Path::new(path)));
        if current_freshness_at_ms
            .map(|current| current >= candidate.freshness_at_ms)
            .unwrap_or(false)
        {
            continue;
        }

        thread.transcript_path = Some(candidate.path.display().to_string());
        thread.updated_at_ms = Some(
            thread
                .updated_at_ms
                .unwrap_or_default()
                .max(candidate.freshness_at_ms),
        );
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RolloutPathCandidate {
    path: PathBuf,
    freshness_at_ms: i64,
}

impl RolloutPathCandidate {
    fn for_path(path: PathBuf) -> Option<Self> {
        rollout_path_freshness_at_ms(&path).map(|freshness_at_ms| Self {
            path,
            freshness_at_ms,
        })
    }
}

fn rollout_paths_by_freshness(sessions_root: &Path) -> Vec<RolloutPathCandidate> {
    let mut candidates = Vec::<RolloutPathCandidate>::new();
    collect_rollout_paths(sessions_root, &mut candidates);
    candidates.sort_by(|left, right| {
        right
            .freshness_at_ms
            .cmp(&left.freshness_at_ms)
            .then_with(|| right.path.cmp(&left.path))
    });
    candidates
}

fn collect_rollout_paths(directory: &Path, candidates: &mut Vec<RolloutPathCandidate>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            collect_rollout_paths(&path, candidates);
            continue;
        }
        if !is_rollout_jsonl_path(&path) {
            continue;
        }
        if let Some(candidate) = RolloutPathCandidate::for_path(path) {
            candidates.push(candidate);
        }
    }
}

fn is_rollout_jsonl_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
        .unwrap_or(false)
}

fn rollout_session_ids(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let reader = BufReader::new(file.take(ROLLOUT_SESSION_META_SCAN_BYTE_LIMIT));
    let mut session_ids = Vec::new();
    for line in reader
        .lines()
        .map_while(Result::ok)
        .take(ROLLOUT_SESSION_META_SCAN_LINE_LIMIT)
    {
        let Ok(record) = serde_json::from_str::<RolloutRecord>(&line) else {
            continue;
        };
        if record.record_type.as_deref() != Some("session_meta") {
            continue;
        }
        if let Some(session_id) = record.payload.and_then(|payload| payload.id)
            && !session_ids.contains(&session_id)
        {
            session_ids.push(session_id);
        }
    }
    if session_ids.is_empty()
        && let Some(session_id) = rollout_session_id_from_filename(path)
    {
        session_ids.push(session_id);
    }
    session_ids
}

fn rollout_session_id_from_filename(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_prefix("rollout-")?.strip_suffix(".jsonl")?;
    let uuid_start = stem.len().checked_sub(36)?;
    Some(stem[uuid_start..].to_owned())
}

fn rollout_timestamp_from_filename(path: &Path) -> Option<i64> {
    let name = path.file_name()?.to_str()?;
    let timestamp = name
        .strip_prefix("rollout-")?
        .get(..ROLLOUT_FILENAME_TIMESTAMP_LENGTH)?;
    let (date, time) = timestamp.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = Month::try_from(date_parts.next()?.parse::<u8>().ok()?).ok()?;
    let day = date_parts.next()?.parse::<u8>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let mut time_parts = time.split('-');
    let hour = time_parts.next()?.parse::<u8>().ok()?;
    let minute = time_parts.next()?.parse::<u8>().ok()?;
    let second = time_parts.next()?.parse::<u8>().ok()?;
    if time_parts.next().is_some() {
        return None;
    }

    let datetime = PrimitiveDateTime::new(
        Date::from_calendar_date(year, month, day).ok()?,
        Time::from_hms(hour, minute, second).ok()?,
    );
    i64::try_from(datetime.assume_utc().unix_timestamp_nanos() / NANOSECONDS_PER_MILLISECOND).ok()
}

fn file_modified_at_ms(path: &Path) -> Option<i64> {
    let modified_at = std::fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified_at.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn rollout_path_freshness_at_ms(path: &Path) -> Option<i64> {
    rollout_timestamp_from_filename(path).or_else(|| file_modified_at_ms(path))
}

fn latest_thread_records(mut threads: Vec<ThreadRecord>, limit: usize) -> Vec<ThreadRecord> {
    threads.sort_by(|left, right| {
        right
            .updated_at_ms
            .unwrap_or_default()
            .cmp(&left.updated_at_ms.unwrap_or_default())
            .then_with(|| left.thread_id.cmp(&right.thread_id))
    });
    threads.truncate(limit);
    threads
}

fn read_thread_ids(connection: &Connection) -> Result<BTreeSet<String>> {
    if !table_exists(connection, "threads")? {
        return Ok(BTreeSet::new());
    }
    let columns = table_columns(connection, "threads")?;
    let id_column = preferred_column(&columns, &["thread_id", "id"]).unwrap_or("id");
    let sql = format!("select {} from threads", quoted_identifier(id_column));
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<BTreeSet<_>, _>>().map_err(Into::into)
}

fn read_thread_counts(connection: &Connection) -> Result<(usize, usize)> {
    if !table_exists(connection, "threads")? {
        return Ok((0, 0));
    }
    let columns = table_columns(connection, "threads")?;
    let active_expression = if columns.contains("archived") {
        "sum(case when coalesce(archived, 0) = 0 then 1 else 0 end)".to_owned()
    } else {
        "count(*)".to_owned()
    };
    let sql = format!("select count(*), coalesce({active_expression}, 0) from threads");
    connection
        .query_row(&sql, [], |row| {
            let total: i64 = row.get(0)?;
            let active: i64 = row.get(1)?;
            Ok((total.max(0) as usize, active.max(0) as usize))
        })
        .map_err(Into::into)
}

fn transcript_originator_for_path(path: &str) -> Option<String> {
    let first_line = BufReader::new(File::open(path).ok()?)
        .lines()
        .next()?
        .ok()?;
    let value = serde_json::from_str::<Value>(&first_line).ok()?;
    value
        .get("payload")
        .and_then(|payload| payload.get("originator"))
        .and_then(Value::as_str)
        .filter(|originator| !originator.trim().is_empty())
        .map(str::to_owned)
}

fn apply_session_index_titles(threads: &mut [ThreadRecord], session_index_path: &Path) {
    if threads.is_empty() {
        return;
    }
    let titles = read_session_index_titles(session_index_path);
    if titles.is_empty() {
        return;
    }

    for thread in threads {
        if let Some(title) = titles.get(&thread.thread_id) {
            thread.title = Some(title.clone());
        }
    }
}

fn read_session_index_titles(session_index_path: &Path) -> BTreeMap<String, String> {
    let Ok(file) = File::open(session_index_path) else {
        return BTreeMap::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<SessionIndexRecord>(&line).ok())
        .filter_map(|record| {
            let id = non_empty_trimmed_string(record.id)?;
            let title = non_empty_trimmed_string(record.thread_name)?;
            Some((id, title))
        })
        .collect()
}

fn non_empty_trimmed_string(value: Option<String>) -> Option<String> {
    let value = value?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn read_dynamic_tools(connection: &Connection) -> Result<BTreeMap<String, Vec<DynamicTool>>> {
    if !table_exists(connection, "thread_dynamic_tools")? {
        return Ok(BTreeMap::new());
    }
    let mut statement = connection.prepare(
        "select thread_id, name, namespace, description, defer_loading
         from thread_dynamic_tools
         order by thread_id, position",
    )?;
    let rows = statement.query_map([], dynamic_tool_from_row)?;
    let mut tools_by_thread: BTreeMap<String, Vec<DynamicTool>> = BTreeMap::new();
    for row in rows {
        let (thread_id, tool) = row?;
        tools_by_thread.entry(thread_id).or_default().push(tool);
    }
    Ok(tools_by_thread)
}

fn read_dynamic_tools_for_threads(
    connection: &Connection,
    thread_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, Vec<DynamicTool>>> {
    if thread_ids.is_empty() || !table_exists(connection, "thread_dynamic_tools")? {
        return Ok(BTreeMap::new());
    }
    let placeholders = sql_placeholders(thread_ids.len());
    let sql = format!(
        "select thread_id, name, namespace, description, defer_loading
         from thread_dynamic_tools
         where thread_id in ({placeholders})
         order by thread_id, position"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(thread_ids.iter()), dynamic_tool_from_row)?;
    let mut tools_by_thread: BTreeMap<String, Vec<DynamicTool>> = BTreeMap::new();
    for row in rows {
        let (thread_id, tool) = row?;
        tools_by_thread.entry(thread_id).or_default().push(tool);
    }
    Ok(tools_by_thread)
}

fn read_spawn_edges(connection: &Connection) -> Result<Vec<SpawnEdge>> {
    if !table_exists(connection, "thread_spawn_edges")? {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare(
        "select parent_thread_id, child_thread_id, status
         from thread_spawn_edges
         order by parent_thread_id, child_thread_id",
    )?;
    let rows = statement.query_map([], spawn_edge_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn read_spawn_edges_for_threads(
    connection: &Connection,
    thread_ids: &BTreeSet<String>,
) -> Result<Vec<SpawnEdge>> {
    if thread_ids.is_empty() || !table_exists(connection, "thread_spawn_edges")? {
        return Ok(Vec::new());
    }
    let mut edges_by_key = BTreeMap::<(String, String), SpawnEdge>::new();
    let mut visited_children = BTreeSet::new();
    let mut frontier = thread_ids.clone();
    while !frontier.is_empty() {
        let next_frontier =
            read_spawn_edges_matching_column(connection, "child_thread_id", &frontier)?
                .into_iter()
                .filter_map(|edge| {
                    let parent_thread_id = edge.parent_thread_id.clone();
                    edges_by_key.insert(
                        (edge.parent_thread_id.clone(), edge.child_thread_id.clone()),
                        edge,
                    );
                    (!visited_children.contains(&parent_thread_id)).then_some(parent_thread_id)
                })
                .collect::<BTreeSet<_>>();
        visited_children.extend(frontier);
        frontier = next_frontier
            .difference(&visited_children)
            .cloned()
            .collect::<BTreeSet<_>>();
    }

    for edge in read_spawn_edges_matching_column(connection, "parent_thread_id", thread_ids)? {
        edges_by_key.insert(
            (edge.parent_thread_id.clone(), edge.child_thread_id.clone()),
            edge,
        );
    }

    Ok(edges_by_key.into_values().collect())
}

fn read_spawn_edges_matching_column(
    connection: &Connection,
    column: &str,
    thread_ids: &BTreeSet<String>,
) -> Result<Vec<SpawnEdge>> {
    if thread_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = sql_placeholders(thread_ids.len());
    let sql = format!(
        "select parent_thread_id, child_thread_id, status
         from thread_spawn_edges
         where {} in ({placeholders})
         order by parent_thread_id, child_thread_id",
        quoted_identifier(column)
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params_from_iter(thread_ids.iter()), spawn_edge_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn dynamic_tool_from_row(row: &Row<'_>) -> rusqlite::Result<(String, DynamicTool)> {
    let name: String = row.get(1)?;
    let namespace: Option<String> = row.get(2)?;
    let description: Option<String> = row.get(3)?;
    let defer_loading: Option<i64> = row.get(4)?;
    Ok((
        row.get::<_, String>(0)?,
        DynamicTool {
            classification: classify_tool(&name, namespace.as_deref()),
            name,
            namespace,
            description,
            defer_loading: defer_loading.unwrap_or(0) != 0,
        },
    ))
}

fn spawn_edge_from_row(row: &Row<'_>) -> rusqlite::Result<SpawnEdge> {
    Ok(SpawnEdge {
        parent_thread_id: row.get(0)?,
        child_thread_id: row.get(1)?,
        status: row.get(2)?,
    })
}

fn sql_placeholders(count: usize) -> String {
    std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(", ")
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool> {
    connection
        .query_row(
            "select name from sqlite_master where type = 'table' and name = ?1",
            [table_name],
            |_row| Ok(()),
        )
        .optional()
        .map(|row| row.is_some())
        .map_err(Into::into)
}

fn table_columns(connection: &Connection, table_name: &str) -> Result<BTreeSet<String>> {
    let mut statement = connection.prepare(&format!(
        "pragma table_info({})",
        quoted_identifier(table_name)
    ))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    rows.collect::<Result<BTreeSet<_>, _>>().map_err(Into::into)
}

fn preferred_column<'a>(columns: &BTreeSet<String>, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .find(|candidate| columns.contains(*candidate))
}

fn nullable_column(columns: &BTreeSet<String>, column_name: &str) -> String {
    if columns.contains(column_name) {
        quoted_identifier(column_name)
    } else {
        "null".to_owned()
    }
}

fn quoted_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn read_hooks_enabled(path: &Path) -> Result<bool> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path)?;
    let value: toml::Value = toml::from_str(&content)?;
    Ok(value
        .get("features")
        .and_then(|features| features.get("hooks"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false))
}

fn read_hooks_json(path: &Path) -> Result<(Vec<String>, Option<String>)> {
    if !path.is_file() {
        return Ok((Vec::new(), None));
    }
    let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let hook_value = value.get("hooks").unwrap_or(&value);
    let Some(object) = hook_value.as_object() else {
        return Ok((Vec::new(), None));
    };
    let mut events = object.keys().cloned().collect::<Vec<_>>();
    events.sort();
    let active_command = events
        .iter()
        .find_map(|event| object.get(event))
        .and_then(first_hook_command);
    Ok((events, active_command))
}

fn read_disabled_owned_hook_states(config_path: &Path, hooks_path: &Path) -> Result<Vec<String>> {
    if !config_path.is_file() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(config_path)?;
    let value: toml::Value = toml::from_str(&content)?;
    let Some(state_table) = value
        .get("hooks")
        .and_then(|hooks| hooks.get("state"))
        .and_then(toml::Value::as_table)
    else {
        return Ok(Vec::new());
    };
    let owned_state_keys = owned_hook_state_keys_for_hooks_path(hooks_path);
    let disabled_states = owned_state_keys
        .into_iter()
        .filter(|key| {
            state_table
                .get(key)
                .and_then(|state| state.get("enabled"))
                .and_then(toml::Value::as_bool)
                == Some(false)
        })
        .collect();
    Ok(disabled_states)
}

fn first_hook_command(value: &Value) -> Option<String> {
    if let Some(command) = value.as_str() {
        return Some(command.to_owned());
    }
    if let Some(command) = value.get("command").and_then(Value::as_str) {
        return Some(command.to_owned());
    }
    if let Some(hooks) = value.get("hooks") {
        return first_hook_command(hooks);
    }
    value.as_array()?.iter().find_map(first_hook_command)
}

fn classify_hook_owner(command: Option<&str>) -> HookOwner {
    let Some(command) = command else {
        return HookOwner::None;
    };
    let normalized = command.to_ascii_lowercase();
    if normalized.contains(LOOPER_HOOK_MARKER) {
        HookOwner::LooperRust
    } else {
        HookOwner::Unknown
    }
}

fn classify_tool(name: &str, namespace: Option<&str>) -> ToolClassification {
    let normalized_name = name.to_ascii_lowercase();
    let normalized_namespace = namespace.unwrap_or_default().to_ascii_lowercase();
    if normalized_name.contains("automation") {
        ToolClassification::Automation
    } else if normalized_namespace.contains("codex_apps") {
        ToolClassification::App
    } else if normalized_namespace.starts_with("mcp__") {
        ToolClassification::Mcp
    } else {
        ToolClassification::BuiltInLocal
    }
}

fn tool_names(tools: &[DynamicTool], classification: ToolClassification) -> Vec<String> {
    tools
        .iter()
        .filter(|tool| tool.classification == classification)
        .map(|tool| tool.name.clone())
        .collect()
}

fn latest_matching_file(directory: &Path, prefix: &str, suffix: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    let mut candidates = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with(prefix) && name.ends_with(suffix))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| compare_matching_files(left, right, prefix, suffix));
    candidates
        .into_iter()
        .rev()
        .find(|path| is_valid_sqlite_file(path))
}

fn compare_matching_files(left: &Path, right: &Path, prefix: &str, suffix: &str) -> Ordering {
    let left_suffix = matching_file_numeric_suffix(left, prefix, suffix);
    let right_suffix = matching_file_numeric_suffix(right, prefix, suffix);
    match (left_suffix, right_suffix) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => file_name_string(left).cmp(&file_name_string(right)),
    }
}

fn matching_file_numeric_suffix(path: &Path, prefix: &str, suffix: &str) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse::<u64>()
        .ok()
}

fn file_name_string(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn is_valid_sqlite_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if metadata.len() < SQLITE_HEADER.len() as u64 {
        return false;
    }

    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0_u8; SQLITE_HEADER.len()];
    file.read_exact(&mut header)
        .map(|_| &header == SQLITE_HEADER)
        .unwrap_or(false)
}

pub fn inspect_codex_servers_from_process_lines(lines: &[String]) -> Vec<CodexServerProcess> {
    let processes = lines
        .iter()
        .filter_map(|line| parse_process_line(line))
        .map(|process| (process.pid, process))
        .collect::<BTreeMap<_, _>>();
    codex_server_processes_from_map(&processes)
}

fn find_codex_server_processes() -> Result<Vec<CodexServerProcess>> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,ppid=,tty=,command="])
        .output()
        .with_context(|| "inspect process table")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(inspect_codex_servers_from_process_lines(
        &stdout.lines().map(str::to_owned).collect::<Vec<_>>(),
    ))
}

fn codex_server_processes_from_map(
    processes: &BTreeMap<i32, ProcessSummary>,
) -> Vec<CodexServerProcess> {
    let mut servers = processes
        .values()
        .filter(|process| is_codex_app_server(process))
        .map(|process| {
            let parent_processes = process_ancestry(processes, process.parent_pid);
            CodexServerProcess::from_process(
                process,
                classify_codex_server_owner(process, &parent_processes),
                parent_processes,
            )
        })
        .collect::<Vec<_>>();
    servers.sort_by_key(|server| server.pid);
    servers
}

fn parse_process_line(line: &str) -> Option<ProcessSummary> {
    let mut parts = line.splitn(4, char::is_whitespace);
    let pid = parts.next()?.trim().parse().ok()?;
    let parent_pid = parts.next()?.trim().parse().ok();
    let tty = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let command = parts.next()?.trim().to_owned();
    if command.is_empty() {
        return None;
    }
    let executable = first_executable_from_command(&command).unwrap_or_default();
    Some(ProcessSummary {
        pid,
        parent_pid,
        tty: tty.map(str::to_owned),
        executable,
        command,
        parent_processes: Vec::new(),
    })
}

impl CodexServerProcess {
    fn from_process(
        process: &ProcessSummary,
        owner: CodexServerOwner,
        parent_processes: Vec<ProcessAncestor>,
    ) -> Self {
        Self {
            pid: process.pid,
            parent_pid: process.parent_pid,
            tty: process.tty.clone(),
            executable: process.executable.clone(),
            command: redact_command_for_display(&process.command),
            owner,
            parent_processes: parent_processes
                .into_iter()
                .map(|ancestor| ProcessAncestor {
                    command: redact_command_for_display(&ancestor.command),
                    ..ancestor
                })
                .collect(),
        }
    }

    fn as_process_summary(&self) -> ProcessSummary {
        ProcessSummary {
            pid: self.pid,
            parent_pid: self.parent_pid,
            tty: self.tty.clone(),
            executable: self.executable.clone(),
            command: self.command.clone(),
            parent_processes: self.parent_processes.clone(),
        }
    }
}

fn is_codex_app_server(process: &ProcessSummary) -> bool {
    let normalized = process.command.to_ascii_lowercase();
    normalized.contains("app-server") && normalized.contains("codex")
}

fn classify_codex_server_owner(
    process: &ProcessSummary,
    parent_processes: &[ProcessAncestor],
) -> CodexServerOwner {
    let mut haystack = format!("{} {}", process.executable, process.command).to_ascii_lowercase();
    for ancestor in parent_processes {
        haystack.push(' ');
        haystack.push_str(&ancestor.executable.to_ascii_lowercase());
        haystack.push(' ');
        haystack.push_str(&ancestor.command.to_ascii_lowercase());
    }

    if contains_any(&haystack, SUPERCONDUCTOR_PROCESS_NEEDLES) {
        CodexServerOwner::Superconductor
    } else if contains_any(&haystack, CURSOR_PROCESS_NEEDLES) {
        CodexServerOwner::Cursor
    } else if contains_any(&haystack, DEVIN_DESKTOP_PROCESS_NEEDLES) {
        CodexServerOwner::DevinDesktop
    } else if contains_any(&haystack, CODEX_APP_PROCESS_NEEDLES) {
        CodexServerOwner::CodexApp
    } else if executable_name(&process.executable)
        .map(|name| name == "codex")
        .unwrap_or(false)
    {
        CodexServerOwner::CodexCli
    } else {
        CodexServerOwner::Unknown
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn executable_name(executable: &str) -> Option<String> {
    std::path::Path::new(executable)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
}

fn first_executable_from_command(command: &str) -> Option<String> {
    command.split_whitespace().next().map(str::to_owned)
}

fn process_ancestry(
    processes: &BTreeMap<i32, ProcessSummary>,
    parent_pid: Option<i32>,
) -> Vec<ProcessAncestor> {
    let mut ancestors = Vec::new();
    let mut next_parent_pid = parent_pid;
    for _ in 0..MAX_PROCESS_ANCESTOR_DEPTH {
        let Some(pid) = next_parent_pid else {
            break;
        };
        let Some(process) = processes.get(&pid) else {
            break;
        };
        ancestors.push(ProcessAncestor {
            pid: process.pid,
            parent_pid: process.parent_pid,
            executable: process.executable.clone(),
            command: process.command.clone(),
        });
        next_parent_pid = process.parent_pid;
    }
    ancestors
}

#[cfg(test)]
mod tests {
    use super::{
        CodexServerOwner, LaunchKind, SpawnEdge, ThreadRecord, build_spawn_graph,
        inspect_codex_servers_from_process_lines, latest_matching_file,
        read_snapshot_state_with_thread_limit, read_state_with_thread_limit,
        refresh_thread_rollout_paths, rollout_session_ids,
    };
    use rusqlite::Connection;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn codex_server_inventory_tracks_all_local_servers_with_owners() {
        let servers = inspect_codex_servers_from_process_lines(&[
            "100 1 ?? /Applications/Codex.app/Contents/Resources/codex app-server --analytics-default-enabled".to_owned(),
            "200 1 ?? /Applications/Cursor.app/Contents/MacOS/Cursor --type=renderer".to_owned(),
            "201 200 ?? /Users/test/.cursor/extensions/openai.chatgpt/bin/macos-aarch64/codex app-server --mcp-url=http://localhost:1234/mcp?token=cursor-secret&worktree=/Users/test/private".to_owned(),
            "300 1 ?? /bin/bash /Users/test/.superconductor/bin/codex app-server".to_owned(),
            "301 300 ?? /opt/homebrew/bin/codex -c mcp_servers.superconductor.url=http://localhost:31418/mcp?sc_token=super-secret&terminal_id=terminal-secret app-server".to_owned(),
            "400 1 ttys001 /opt/homebrew/bin/codex app-server --api-key=cli-secret".to_owned(),
            "500 1 ?? /Applications/Devin - Next.app/Contents/MacOS/Devin - Next".to_owned(),
            "501 500 ?? /Applications/Devin - Next.app/Contents/Frameworks/Devin - Next Helper (Plugin).app/Contents/MacOS/Devin - Next Helper (Plugin) --type=utility".to_owned(),
            "502 501 ?? /Applications/Codex.app/Contents/Resources/codex app-server --listen stdio://".to_owned(),
        ]);

        assert_eq!(servers.len(), 6);
        assert!(
            servers
                .iter()
                .any(|server| server.owner == CodexServerOwner::CodexApp)
        );
        assert!(
            servers
                .iter()
                .any(|server| server.owner == CodexServerOwner::Cursor)
        );
        assert!(
            servers
                .iter()
                .any(|server| server.owner == CodexServerOwner::Superconductor)
        );
        assert!(
            servers
                .iter()
                .any(|server| server.owner == CodexServerOwner::CodexCli)
        );
        assert!(
            servers
                .iter()
                .any(|server| server.owner == CodexServerOwner::DevinDesktop)
        );
    }

    #[test]
    fn codex_server_inventory_attributes_devin_parent_chain() {
        let servers = inspect_codex_servers_from_process_lines(&[
            "600 1 ?? /Applications/Devin - Next.app/Contents/MacOS/Devin - Next".to_owned(),
            "601 600 ?? npm exec @agentclientprotocol/codex-acp".to_owned(),
            "602 601 ?? /Users/test/.nvm/bin/node /Users/test/.npm/_npx/openai/node_modules/@openai/codex/bin/codex.js app-server".to_owned(),
        ]);

        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].owner, CodexServerOwner::DevinDesktop);
    }

    #[test]
    fn refresh_thread_rollout_paths_uses_session_meta_for_resumed_threads() {
        let tempdir = tempdir().expect("tempdir");
        let sessions_root = tempdir
            .path()
            .join("sessions")
            .join("2026")
            .join("06")
            .join("16");
        fs::create_dir_all(&sessions_root).expect("sessions root");
        let stale_rollout = sessions_root
            .join("rollout-2026-06-16T00-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl");
        let resumed_rollout = sessions_root
            .join("rollout-2026-06-16T13-05-58-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.jsonl");
        write_session_meta_rollout(&stale_rollout, "thread-1");
        write_session_meta_rollout(&resumed_rollout, "fork-thread");
        append_session_meta_rollout(&resumed_rollout, "thread-1");
        let mut threads = vec![test_thread_record(
            "thread-1",
            stale_rollout.display().to_string(),
        )];

        refresh_thread_rollout_paths(&mut threads, tempdir.path(), None);

        assert_eq!(
            threads[0].transcript_path.as_deref(),
            Some(resumed_rollout.to_str().expect("utf8 path"))
        );
        assert!(threads[0].updated_at_ms.unwrap_or_default() > 1);
    }

    #[test]
    fn refresh_thread_rollout_paths_scans_past_early_session_meta() {
        let tempdir = tempdir().expect("tempdir");
        let sessions_root = create_test_sessions_root(tempdir.path());
        let stale_rollout = sessions_root
            .join("rollout-2026-06-16T00-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl");
        let resumed_rollout = sessions_root
            .join("rollout-2026-06-16T13-05-58-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.jsonl");
        write_session_meta_rollout(&stale_rollout, "thread-1");
        write_session_meta_rollout(&resumed_rollout, "fork-thread");
        append_filler_records(&resumed_rollout, 160);
        append_session_meta_rollout(&resumed_rollout, "thread-1");
        let mut threads = vec![test_thread_record(
            "thread-1",
            stale_rollout.display().to_string(),
        )];

        refresh_thread_rollout_paths(&mut threads, tempdir.path(), None);

        assert_eq!(
            threads[0].transcript_path.as_deref(),
            Some(resumed_rollout.to_str().expect("utf8 path"))
        );
    }

    #[test]
    fn refresh_thread_rollout_paths_prefers_newer_rollout_name_over_touched_stale_file() {
        let tempdir = tempdir().expect("tempdir");
        let sessions_root = create_test_sessions_root(tempdir.path());
        let stale_rollout = sessions_root
            .join("rollout-2026-06-16T00-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl");
        let resumed_rollout = sessions_root
            .join("rollout-2026-06-16T13-05-58-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.jsonl");
        write_session_meta_rollout(&resumed_rollout, "thread-1");
        write_session_meta_rollout(&stale_rollout, "thread-1");
        append_session_meta_rollout(&stale_rollout, "thread-1");
        let mut threads = vec![test_thread_record(
            "thread-1",
            stale_rollout.display().to_string(),
        )];

        refresh_thread_rollout_paths(&mut threads, tempdir.path(), None);

        assert_eq!(
            threads[0].transcript_path.as_deref(),
            Some(resumed_rollout.to_str().expect("utf8 path"))
        );
    }

    #[test]
    fn rollout_session_ids_falls_back_to_filename_when_meta_is_after_byte_limit() {
        let tempdir = tempdir().expect("tempdir");
        let sessions_root = create_test_sessions_root(tempdir.path());
        let filename_session_id = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        let rollout = sessions_root.join(format!(
            "rollout-2026-06-16T13-05-58-{filename_session_id}.jsonl"
        ));
        let oversized_record = "x".repeat(super::ROLLOUT_SESSION_META_SCAN_BYTE_LIMIT as usize + 1);
        fs::write(&rollout, format!("{oversized_record}\n")).expect("write oversized rollout");
        append_session_meta_rollout(&rollout, "thread-after-byte-limit");

        assert_eq!(
            rollout_session_ids(&rollout),
            vec![filename_session_id.to_owned()]
        );
    }

    #[test]
    fn bounded_state_refreshes_rollouts_before_applying_thread_limit() {
        let tempdir = tempdir().expect("tempdir");
        let state_db = tempdir.path().join("state_1.sqlite");
        let connection = Connection::open(&state_db).expect("open state db");
        connection
            .execute(
                "create table threads (
                    id text primary key,
                    rollout_path text,
                    updated_at_ms integer,
                    archived integer
                )",
                [],
            )
            .expect("create threads");
        for index in 0..12 {
            connection
                .execute(
                    "insert into threads (id, updated_at_ms, archived) values (?1, ?2, 0)",
                    (format!("newer-thread-{index}"), 10_000_i64 - index),
                )
                .expect("insert newer thread");
        }

        let sessions_root = create_test_sessions_root(tempdir.path());
        let stale_rollout = sessions_root
            .join("rollout-2026-06-16T00-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl");
        let resumed_rollout = sessions_root
            .join("rollout-2026-06-16T13-05-58-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.jsonl");
        write_session_meta_rollout(&stale_rollout, "thread-13");
        write_session_meta_rollout(&resumed_rollout, "thread-13");
        connection
            .execute(
                "insert into threads (id, rollout_path, updated_at_ms, archived)
                 values (?1, ?2, ?3, 0)",
                ("thread-13", stale_rollout.display().to_string(), 1_i64),
            )
            .expect("insert stale thread");

        let state = read_state_with_thread_limit(tempdir.path(), Some(12)).expect("read state");

        assert_eq!(state.threads.len(), 12);
        assert_eq!(state.threads[0].thread_id, "thread-13");
        assert_eq!(
            state.threads[0].transcript_path.as_deref(),
            Some(resumed_rollout.to_str().expect("utf8 path"))
        );
    }

    #[test]
    fn snapshot_state_applies_thread_limit_without_rollout_scan() {
        let tempdir = tempdir().expect("tempdir");
        let state_db = tempdir.path().join("state_1.sqlite");
        let connection = Connection::open(&state_db).expect("open state db");
        connection
            .execute(
                "create table threads (
                    id text primary key,
                    rollout_path text,
                    updated_at_ms integer,
                    archived integer
                )",
                [],
            )
            .expect("create threads");
        for index in 0..12 {
            connection
                .execute(
                    "insert into threads (id, updated_at_ms, archived) values (?1, ?2, 0)",
                    (format!("newer-thread-{index}"), 10_000_i64 - index),
                )
                .expect("insert newer thread");
        }

        let sessions_root = create_test_sessions_root(tempdir.path());
        let stale_rollout = sessions_root
            .join("rollout-2026-06-16T00-00-00-aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl");
        let resumed_rollout = sessions_root
            .join("rollout-2026-06-16T13-05-58-bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb.jsonl");
        write_session_meta_rollout(&stale_rollout, "thread-13");
        write_session_meta_rollout(&resumed_rollout, "thread-13");
        connection
            .execute(
                "insert into threads (id, rollout_path, updated_at_ms, archived)
                 values (?1, ?2, ?3, 0)",
                ("thread-13", stale_rollout.display().to_string(), 1_i64),
            )
            .expect("insert stale thread");

        let state =
            read_snapshot_state_with_thread_limit(tempdir.path(), Some(12)).expect("read state");

        assert_eq!(state.threads.len(), 12);
        assert!(
            state
                .threads
                .iter()
                .all(|thread| thread.thread_id != "thread-13")
        );
        assert!(state.threads.iter().all(|thread| {
            thread
                .transcript_path
                .as_deref()
                .map(|path| path != resumed_rollout.to_str().expect("utf8 path"))
                .unwrap_or(true)
        }));
    }

    #[test]
    fn read_state_prefers_codex_session_index_thread_name_over_state_title() {
        let tempdir = tempdir().expect("tempdir");
        let state_db = tempdir.path().join("state_1.sqlite");
        let connection = Connection::open(&state_db).expect("open state db");
        connection
            .execute(
                "create table threads (
                    id text primary key,
                    title text,
                    updated_at_ms integer,
                    archived integer
                )",
                [],
            )
            .expect("create threads");
        connection
            .execute(
                "insert into threads (id, title, updated_at_ms, archived)
                 values (?1, ?2, ?3, 0)",
                ("thread-1", "first prompt copied into state title", 10_i64),
            )
            .expect("insert thread");
        fs::write(
            tempdir.path().join(super::SESSION_INDEX_FILENAME),
            serde_json::json!({
                "id": "thread-1",
                "thread_name": "Generated Codex Desktop title"
            })
            .to_string()
                + "\n",
        )
        .expect("write session index");

        let state = read_state_with_thread_limit(tempdir.path(), None).expect("read state");

        assert_eq!(state.threads.len(), 1);
        assert_eq!(
            state.threads[0].title.as_deref(),
            Some("Generated Codex Desktop title")
        );
    }

    #[test]
    fn session_index_titles_ignore_blank_and_malformed_rows() {
        let tempdir = tempdir().expect("tempdir");
        let state_db = tempdir.path().join("state_1.sqlite");
        let connection = Connection::open(&state_db).expect("open state db");
        connection
            .execute(
                "create table threads (
                    id text primary key,
                    title text,
                    updated_at_ms integer,
                    archived integer
                )",
                [],
            )
            .expect("create threads");
        connection
            .execute(
                "insert into threads (id, title, updated_at_ms, archived)
                 values (?1, ?2, ?3, 0)",
                ("thread-1", "state title", 10_i64),
            )
            .expect("insert thread");
        connection
            .execute(
                "insert into threads (id, title, updated_at_ms, archived)
                 values (?1, ?2, ?3, 0)",
                ("thread-2", "second state title", 9_i64),
            )
            .expect("insert second thread");
        fs::write(
            tempdir.path().join(super::SESSION_INDEX_FILENAME),
            [
                "{\"id\":\"thread-1\",\"thread_name\":\"   \"}",
                "not-json",
                "{\"id\":\"thread-2\",\"thread_name\":\"  Indexed title  \"}",
            ]
            .join("\n"),
        )
        .expect("write session index");

        let state = read_state_with_thread_limit(tempdir.path(), None).expect("read state");

        assert_eq!(state.threads.len(), 2);
        assert_eq!(state.threads[0].title.as_deref(), Some("state title"));
        assert_eq!(state.threads[1].title.as_deref(), Some("Indexed title"));
    }

    #[test]
    fn codex_server_inventory_redacts_commands_and_ancestors() {
        let servers = inspect_codex_servers_from_process_lines(&[
            "500 1 ?? /bin/bash /Users/test/.superconductor/bin/codex app-server --token=parent-secret".to_owned(),
            "501 500 ?? /opt/homebrew/bin/codex -c mcp_servers.superconductor.url=http://localhost:31418/mcp?sc_token=child-secret&terminal_id=terminal-secret app-server --api-key=cli-secret".to_owned(),
        ]);

        let json = serde_json::to_string(&servers).expect("serialize servers");
        assert!(!json.contains("parent-secret"));
        assert!(!json.contains("child-secret"));
        assert!(!json.contains("terminal-secret"));
        assert!(!json.contains("cli-secret"));
        assert!(json.contains("<redacted>"));
    }

    #[test]
    fn latest_matching_file_skips_empty_sqlite_placeholders() {
        let tempdir = tempdir().expect("tempdir");
        let valid_path = tempdir.path().join("state_5.sqlite");
        let empty_path = tempdir.path().join("state_9.sqlite");
        fs::write(&valid_path, super::SQLITE_HEADER).expect("write valid sqlite header");
        fs::write(&empty_path, []).expect("write empty placeholder");

        let selected = latest_matching_file(tempdir.path(), "state_", ".sqlite");

        assert_eq!(selected.as_deref(), Some(valid_path.as_path()));
    }

    #[test]
    fn latest_matching_file_orders_numeric_suffixes_numerically() {
        let tempdir = tempdir().expect("tempdir");
        let state_9 = tempdir.path().join("state_9.sqlite");
        let state_10 = tempdir.path().join("state_10.sqlite");
        let state_11_empty = tempdir.path().join("state_11.sqlite");
        fs::write(&state_9, super::SQLITE_HEADER).expect("write state 9");
        fs::write(&state_10, super::SQLITE_HEADER).expect("write state 10");
        fs::write(&state_11_empty, []).expect("write empty state 11");

        let selected = latest_matching_file(tempdir.path(), "state_", ".sqlite");

        assert_eq!(selected.as_deref(), Some(state_10.as_path()));
    }

    #[test]
    fn spawn_graph_resolves_nested_root_with_cycle_guard() {
        let edges = vec![
            SpawnEdge {
                parent_thread_id: "root".to_owned(),
                child_thread_id: "child".to_owned(),
                status: None,
            },
            SpawnEdge {
                parent_thread_id: "child".to_owned(),
                child_thread_id: "grandchild".to_owned(),
                status: None,
            },
        ];

        let graph = build_spawn_graph("grandchild", &edges);

        assert_eq!(graph.parent_thread_id.as_deref(), Some("child"));
        assert_eq!(graph.root_thread_id, "root");
        assert_eq!(graph.launch_kind, LaunchKind::Subagent);

        let cyclic_edges = vec![
            SpawnEdge {
                parent_thread_id: "left".to_owned(),
                child_thread_id: "right".to_owned(),
                status: None,
            },
            SpawnEdge {
                parent_thread_id: "right".to_owned(),
                child_thread_id: "left".to_owned(),
                status: None,
            },
        ];
        let cyclic_graph = build_spawn_graph("left", &cyclic_edges);
        assert_eq!(cyclic_graph.parent_thread_id.as_deref(), Some("right"));
        assert_eq!(cyclic_graph.root_thread_id, "right");
    }

    fn write_session_meta_rollout(path: &Path, session_id: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("rollout parent");
        }
        let record = serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": session_id
            }
        })
        .to_string();
        fs::write(path, format!("{record}\n")).expect("write rollout");
    }

    fn create_test_sessions_root(root: &Path) -> std::path::PathBuf {
        let sessions_root = root.join("sessions").join("2026").join("06").join("16");
        fs::create_dir_all(&sessions_root).expect("sessions root");
        sessions_root
    }

    fn append_session_meta_rollout(path: &Path, session_id: &str) {
        use std::io::Write;

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open rollout");
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session_meta",
                "payload": {
                    "id": session_id
                }
            })
        )
        .expect("append rollout");
    }

    fn append_filler_records(path: &Path, count: usize) {
        use std::io::Write;

        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("open rollout");
        for index in 0..count {
            writeln!(
                file,
                "{}",
                serde_json::json!({
                    "type": "event_msg",
                    "payload": {
                        "type": "progress",
                        "message": format!("filler {index}")
                    }
                })
            )
            .expect("append filler");
        }
    }

    fn test_thread_record(thread_id: &str, transcript_path: String) -> ThreadRecord {
        ThreadRecord {
            thread_id: thread_id.to_owned(),
            title: None,
            cwd: None,
            transcript_path: Some(transcript_path),
            source: Some("vscode".to_owned()),
            originator: Some("Codex Desktop".to_owned()),
            model: None,
            reasoning_effort: None,
            git_sha: None,
            git_branch: None,
            cli_version: None,
            agent_nickname: None,
            agent_role: None,
            agent_path: None,
            created_at_ms: None,
            updated_at_ms: Some(1),
            archived: false,
        }
    }
}

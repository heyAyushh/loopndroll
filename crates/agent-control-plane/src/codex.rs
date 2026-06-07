use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
}

#[derive(Clone, Debug)]
pub struct StateData {
    pub threads: Vec<ThreadRecord>,
    pub dynamic_tools_by_thread: BTreeMap<String, Vec<DynamicTool>>,
    pub spawn_edges: Vec<SpawnEdge>,
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
    let sources = discover_sources(codex_home);
    let Some(state_db) = sources.state_db else {
        return Ok(StateData {
            threads: Vec::new(),
            dynamic_tools_by_thread: BTreeMap::new(),
            spawn_edges: Vec::new(),
        });
    };
    let connection = Connection::open(&state_db)
        .with_context(|| format!("open Codex state DB {}", state_db.display()))?;
    Ok(StateData {
        threads: read_threads(&connection)?,
        dynamic_tools_by_thread: read_dynamic_tools(&connection)?,
        spawn_edges: read_spawn_edges(&connection)?,
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
    let root_thread_id = parent_thread_id
        .clone()
        .unwrap_or_else(|| thread_id.to_owned());
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

fn read_threads(connection: &Connection) -> Result<Vec<ThreadRecord>> {
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
         order by {order} desc",
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

fn read_dynamic_tools(connection: &Connection) -> Result<BTreeMap<String, Vec<DynamicTool>>> {
    if !table_exists(connection, "thread_dynamic_tools")? {
        return Ok(BTreeMap::new());
    }
    let mut statement = connection.prepare(
        "select thread_id, name, namespace, description, defer_loading
         from thread_dynamic_tools
         order by thread_id, position",
    )?;
    let rows = statement.query_map([], |row| {
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
    })?;
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
    let rows = statement.query_map([], |row| {
        Ok(SpawnEdge {
            parent_thread_id: row.get(0)?,
            child_thread_id: row.get(1)?,
            status: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
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
    candidates.sort();
    candidates
        .into_iter()
        .rev()
        .find(|path| is_valid_sqlite_file(path))
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
    use super::{CodexServerOwner, inspect_codex_servers_from_process_lines, latest_matching_file};
    use std::fs;
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
}

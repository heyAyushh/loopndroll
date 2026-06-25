use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOALS_DIRECTORY_NAME: &str = "goals";
const ROOT_GOALS_FILE_NAME: &str = "GOALS.md";
const DIRECTORY_GOAL_FILES: [&str; 4] = ["goal.toml", "goal.json", "goal.md", "README.md"];
const GOALS_DATABASE_PREFIX: &str = "goals_";
const SQLITE_DATABASE_SUFFIX: &str = ".sqlite";
const THREAD_GOALS_TABLE_NAME: &str = "thread_goals";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GoalStatus {
    Pursuing,
    Paused,
    Blocked,
    Achieved,
    Unmet,
    UsageLimited,
    BudgetLimited,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GoalSourceKind {
    Toml,
    Json,
    Markdown,
    Sqlite,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalDefinition {
    pub id: String,
    pub title: String,
    pub status: GoalStatus,
    pub priority: Option<String>,
    pub target_thread_id: Option<String>,
    pub source_kind: GoalSourceKind,
    pub source_path: PathBuf,
    pub updated_at_ms: Option<i64>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalSummary {
    pub id: String,
    pub title: String,
    pub status: GoalStatus,
    pub lifecycle: GoalStatus,
    pub running: bool,
    pub priority: Option<String>,
    pub target_thread_id: Option<String>,
    pub target_known: bool,
    pub source_kind: GoalSourceKind,
    pub source_path: String,
    pub updated_at_ms: Option<i64>,
    pub token_budget: Option<i64>,
    pub tokens_used: Option<i64>,
    pub time_used_seconds: Option<i64>,
    pub content_hash: String,
    pub sync_safe: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadGoalSummary {
    pub id: String,
    pub title: String,
    pub status: GoalStatus,
    pub lifecycle: GoalStatus,
    pub running: bool,
    pub token_budget: Option<i64>,
    pub tokens_used: Option<i64>,
    pub time_used_seconds: Option<i64>,
    pub updated_at_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct GoalToml {
    id: Option<String>,
    title: Option<String>,
    name: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    target_thread_id: Option<String>,
}

impl GoalDefinition {
    pub fn to_summary(&self, known_thread_ids: &BTreeSet<String>) -> GoalSummary {
        GoalSummary {
            id: self.id.clone(),
            title: self.title.clone(),
            status: self.status.clone(),
            lifecycle: self.status.clone(),
            running: goal_status_is_running(&self.status),
            priority: self.priority.clone(),
            target_thread_id: self.target_thread_id.clone(),
            target_known: self
                .target_thread_id
                .as_ref()
                .map(|thread_id| known_thread_ids.contains(thread_id))
                .unwrap_or(false),
            source_kind: self.source_kind.clone(),
            source_path: self.source_path.display().to_string(),
            updated_at_ms: self.updated_at_ms,
            token_budget: None,
            tokens_used: None,
            time_used_seconds: None,
            content_hash: self.content_hash.clone(),
            sync_safe: true,
        }
    }
}

pub fn read_goals(
    codex_home: &Path,
    known_thread_ids: &BTreeSet<String>,
) -> Result<Vec<GoalSummary>> {
    let mut goals = Vec::new();
    for path in goal_candidate_files(codex_home)? {
        let goal = read_goal_file(&path)?;
        goals.push(goal.to_summary(known_thread_ids));
    }
    goals.extend(read_thread_goals_database(codex_home, known_thread_ids)?);
    goals.sort_by(|left, right| left.id.cmp(&right.id));
    goals.dedup_by(|left, right| {
        left.id == right.id && left.target_thread_id == right.target_thread_id
    });
    Ok(goals)
}

pub fn goal_for_thread(goals: &[GoalSummary], thread_id: &str) -> Option<ThreadGoalSummary> {
    goals
        .iter()
        .filter(|goal| goal.target_thread_id.as_deref() == Some(thread_id))
        .max_by(|left, right| {
            left.running
                .cmp(&right.running)
                .then_with(|| {
                    left.updated_at_ms
                        .unwrap_or_default()
                        .cmp(&right.updated_at_ms.unwrap_or_default())
                })
                .then_with(|| left.id.cmp(&right.id))
        })
        .map(ThreadGoalSummary::from)
}

impl From<&GoalSummary> for ThreadGoalSummary {
    fn from(goal: &GoalSummary) -> Self {
        Self {
            id: goal.id.clone(),
            title: goal.title.clone(),
            status: goal.status.clone(),
            lifecycle: goal.lifecycle.clone(),
            running: goal.running,
            token_budget: goal.token_budget,
            tokens_used: goal.tokens_used,
            time_used_seconds: goal.time_used_seconds,
            updated_at_ms: goal.updated_at_ms,
        }
    }
}

fn goal_candidate_files(codex_home: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    let goals_path = codex_home.join(GOALS_DIRECTORY_NAME);
    if goals_path.exists() {
        let mut entries = std::fs::read_dir(&goals_path)
            .with_context(|| format!("read {}", goals_path.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                if let Some(path) = DIRECTORY_GOAL_FILES
                    .iter()
                    .map(|file_name| path.join(file_name))
                    .find(|candidate| candidate.is_file())
                {
                    paths.push(path);
                }
            } else if supported_goal_file(&path) {
                paths.push(path);
            }
        }
    }

    let root_goals = codex_home.join(ROOT_GOALS_FILE_NAME);
    if root_goals.is_file() {
        paths.push(root_goals);
    }
    Ok(paths)
}

fn supported_goal_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| matches!(extension, "toml" | "json" | "md"))
        .unwrap_or(false)
}

fn read_goal_file(path: &Path) -> Result<GoalDefinition> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("read goal {}", path.display()))?;
    let source_kind = source_kind(path);
    let default_id = id_from_path(path);
    let content_hash = sha256_hex(content.as_bytes());
    let updated_at_ms = updated_at_ms(path);
    match source_kind {
        GoalSourceKind::Toml => {
            read_toml_goal(path, &content, default_id, content_hash, updated_at_ms)
        }
        GoalSourceKind::Json => {
            read_json_goal(path, &content, default_id, content_hash, updated_at_ms)
        }
        GoalSourceKind::Markdown => Ok(GoalDefinition {
            id: default_id,
            title: first_markdown_heading(&content).unwrap_or_else(|| "Untitled goal".to_owned()),
            status: parse_status(markdown_metadata(&content, "status").as_deref()),
            priority: markdown_metadata(&content, "priority"),
            target_thread_id: markdown_metadata(&content, "target_thread_id"),
            source_kind,
            source_path: path.to_path_buf(),
            updated_at_ms,
            content_hash,
        }),
        GoalSourceKind::Sqlite => unreachable!("SQLite goals are read from the goals database"),
    }
}

fn read_toml_goal(
    path: &Path,
    content: &str,
    default_id: String,
    content_hash: String,
    updated_at_ms: Option<i64>,
) -> Result<GoalDefinition> {
    let parsed: GoalToml =
        toml::from_str(content).with_context(|| format!("parse goal {}", path.display()))?;
    Ok(GoalDefinition {
        id: parsed.id.unwrap_or(default_id),
        title: parsed
            .title
            .or(parsed.name)
            .unwrap_or_else(|| "Untitled goal".to_owned()),
        status: parse_status(parsed.status.as_deref()),
        priority: parsed.priority,
        target_thread_id: parsed.target_thread_id,
        source_kind: GoalSourceKind::Toml,
        source_path: path.to_path_buf(),
        updated_at_ms,
        content_hash,
    })
}

fn read_json_goal(
    path: &Path,
    content: &str,
    default_id: String,
    content_hash: String,
    updated_at_ms: Option<i64>,
) -> Result<GoalDefinition> {
    let value: Value =
        serde_json::from_str(content).with_context(|| format!("parse goal {}", path.display()))?;
    Ok(GoalDefinition {
        id: value_string(&value, "id").unwrap_or(default_id),
        title: value_string(&value, "title")
            .or_else(|| value_string(&value, "name"))
            .unwrap_or_else(|| "Untitled goal".to_owned()),
        status: parse_status(value_string(&value, "status").as_deref()),
        priority: value_string(&value, "priority"),
        target_thread_id: value_string(&value, "target_thread_id"),
        source_kind: GoalSourceKind::Json,
        source_path: path.to_path_buf(),
        updated_at_ms,
        content_hash,
    })
}

fn read_thread_goals_database(
    codex_home: &Path,
    known_thread_ids: &BTreeSet<String>,
) -> Result<Vec<GoalSummary>> {
    let Some(database_path) = latest_goals_database(codex_home) else {
        return Ok(Vec::new());
    };
    let connection = Connection::open(&database_path)
        .with_context(|| format!("open Codex goals DB {}", database_path.display()))?;
    if !table_exists(&connection, THREAD_GOALS_TABLE_NAME)? {
        return Ok(Vec::new());
    }

    let mut statement = connection.prepare(
        "select
            thread_id,
            goal_id,
            objective,
            status,
            token_budget,
            tokens_used,
            time_used_seconds,
            created_at_ms,
            updated_at_ms
         from thread_goals
         order by updated_at_ms desc, thread_id",
    )?;
    let rows = statement.query_map([], |row| {
        let thread_id: String = row.get(0)?;
        let goal_id: String = row.get(1)?;
        let objective: String = row.get(2)?;
        let status_text: String = row.get(3)?;
        let token_budget: Option<i64> = row.get(4)?;
        let tokens_used: i64 = row.get(5)?;
        let time_used_seconds: i64 = row.get(6)?;
        let created_at_ms: i64 = row.get(7)?;
        let updated_at_ms: i64 = row.get(8)?;
        let status = parse_status(Some(&status_text));
        let content_hash = sha256_hex(
            format!(
                "{thread_id}\n{goal_id}\n{objective}\n{status_text}\n{token_budget:?}\n{tokens_used}\n{time_used_seconds}\n{created_at_ms}\n{updated_at_ms}"
            )
            .as_bytes(),
        );
        Ok(GoalSummary {
            id: goal_id,
            title: objective,
            status: status.clone(),
            lifecycle: status.clone(),
            running: goal_status_is_running(&status),
            priority: None,
            target_thread_id: Some(thread_id.clone()),
            target_known: known_thread_ids.contains(&thread_id),
            source_kind: GoalSourceKind::Sqlite,
            source_path: database_path.display().to_string(),
            updated_at_ms: Some(updated_at_ms),
            token_budget,
            tokens_used: Some(tokens_used),
            time_used_seconds: Some(time_used_seconds),
            content_hash,
            sync_safe: true,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn latest_goals_database(codex_home: &Path) -> Option<PathBuf> {
    let mut candidates = std::fs::read_dir(codex_home)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.starts_with(GOALS_DATABASE_PREFIX)
                        && name.ends_with(SQLITE_DATABASE_SUFFIX)
                })
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
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

fn source_kind(path: &Path) -> GoalSourceKind {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("toml") => GoalSourceKind::Toml,
        Some("json") => GoalSourceKind::Json,
        _ => GoalSourceKind::Markdown,
    }
}

fn id_from_path(path: &Path) -> String {
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| DIRECTORY_GOAL_FILES.contains(&name))
        .unwrap_or(false)
    {
        return path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "goal".to_owned());
    }
    path.file_stem()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "goal".to_owned())
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn first_markdown_heading(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn markdown_metadata(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    content.lines().take(20).find_map(|line| {
        line.strip_prefix(&prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn parse_status(value: Option<&str>) -> GoalStatus {
    match value.unwrap_or("active").to_ascii_lowercase().as_str() {
        "pursuing" | "active" | "open" | "todo" | "in_progress" | "in-progress" => {
            GoalStatus::Pursuing
        }
        "paused" => GoalStatus::Paused,
        "blocked" => GoalStatus::Blocked,
        "achieved" | "done" | "complete" | "completed" | "closed" => GoalStatus::Achieved,
        "unmet" | "failed" => GoalStatus::Unmet,
        "usage-limited" | "usage_limited" | "usage limited" => GoalStatus::UsageLimited,
        "budget-limited" | "budget_limited" | "budget limited" => GoalStatus::BudgetLimited,
        _ => GoalStatus::Unknown,
    }
}

fn goal_status_is_running(status: &GoalStatus) -> bool {
    matches!(status, GoalStatus::Pursuing)
}

fn updated_at_ms(path: &Path) -> Option<i64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    system_time_ms(modified)
}

fn system_time_ms(time: SystemTime) -> Option<i64> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn sha256_hex(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

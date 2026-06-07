use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::assistant::AssistantKind;
use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities};
use crate::control_plane::DesktopThread;

const SESSIONS_DIR: &str = "sessions";
const ACTIVE_SESSIONS_FILE: &str = "active_sessions.json";
const SUMMARY_FILE: &str = "summary.json";
const UPDATES_FILE: &str = "updates.jsonl";
const CHAT_HISTORY_FILE: &str = "chat_history.jsonl";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokSessionRecord {
    pub session_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub updates_path: PathBuf,
    pub updated_at: String,
    pub assistant_preview: Option<String>,
    pub running: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct GrokActiveSession {
    session_id: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GrokSessionSummary {
    info: GrokSessionInfo,
    #[serde(default)]
    session_summary: Option<String>,
    #[serde(default)]
    generated_title: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    last_active_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GrokSessionInfo {
    id: String,
    #[serde(default)]
    cwd: Option<String>,
}

pub fn discover_grok_sessions(grok_home: &Path) -> Result<Vec<GrokSessionRecord>> {
    let sessions_root = grok_home.join(SESSIONS_DIR);
    if !sessions_root.is_dir() {
        return Ok(Vec::new());
    }

    let active_sessions = read_active_sessions(grok_home)?;
    let active_ids = active_sessions
        .iter()
        .map(|session| session.session_id.as_str())
        .collect::<BTreeSet<_>>();
    let active_cwd_by_id = active_sessions
        .iter()
        .map(|session| (session.session_id.clone(), session.cwd.clone()))
        .collect::<BTreeMap<_, _>>();

    let mut records = Vec::new();
    for workspace_dir in fs::read_dir(&sessions_root)
        .with_context(|| format!("read grok sessions root {}", sessions_root.display()))?
    {
        let workspace_dir = workspace_dir?.path();
        if !workspace_dir.is_dir() {
            continue;
        }
        for session_dir in fs::read_dir(&workspace_dir)
            .with_context(|| format!("read grok workspace sessions {}", workspace_dir.display()))?
        {
            let session_dir = session_dir?.path();
            if !session_dir.is_dir() {
                continue;
            }
            let Some(record) = grok_session_record_from_dir(
                &session_dir,
                active_ids.contains(
                    session_dir
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                ),
                active_cwd_by_id
                    .get(
                        session_dir
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or_default(),
                    )
                    .and_then(|cwd| cwd.clone()),
            )?
            else {
                continue;
            };
            records.push(record);
        }
    }

    records.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(records)
}

pub fn grok_session_to_desktop_thread(session: &GrokSessionRecord) -> DesktopThread {
    DesktopThread {
        thread_id: session.session_id.clone(),
        title: session.title.clone(),
        cwd: session.cwd.clone(),
        transcript_path: Some(session.updates_path.display().to_string()),
        source: Some("grok-build".to_owned()),
        originator: Some("Grok Build".to_owned()),
        model: None,
        reasoning_effort: None,
        git_sha: None,
        git_branch: None,
        cli_version: None,
        agent_nickname: None,
        agent_role: None,
        agent_path: None,
        created_at_ms: None,
        updated_at_ms: parse_timestamp_ms(&session.updated_at),
        assistant_preview: session.assistant_preview.clone(),
        archived: false,
        capabilities: ThreadCapabilities {
            thread_id: session.session_id.clone(),
            assistant_kind: AssistantKind::GrokBuild,
            tools: Vec::new(),
            mcp_tools: Vec::new(),
            app_tools: Vec::new(),
            automation_tools: Vec::new(),
            spawn: SpawnGraph {
                parent_thread_id: None,
                root_thread_id: session.session_id.clone(),
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
        },
    }
}

fn grok_session_record_from_dir(
    session_dir: &Path,
    running: bool,
    active_cwd: Option<String>,
) -> Result<Option<GrokSessionRecord>> {
    let summary_path = session_dir.join(SUMMARY_FILE);
    let updates_path = session_dir.join(UPDATES_FILE);
    if !summary_path.is_file() {
        return Ok(None);
    }

    let summary: GrokSessionSummary = serde_json::from_slice(
        &fs::read(&summary_path).with_context(|| format!("read {}", summary_path.display()))?,
    )
    .with_context(|| format!("parse {}", summary_path.display()))?;
    let title = summary
        .generated_title
        .or(summary.session_summary.clone())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let assistant_preview = summary
        .session_summary
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            latest_assistant_message_from_chat_history(&session_dir.join(CHAT_HISTORY_FILE))
        });
    let updated_at = summary
        .last_active_at
        .or(summary.updated_at)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default()
        });

    Ok(Some(GrokSessionRecord {
        session_id: summary.info.id,
        title,
        cwd: active_cwd.or(summary.info.cwd),
        updates_path,
        updated_at,
        assistant_preview,
        running,
    }))
}

fn read_active_sessions(grok_home: &Path) -> Result<Vec<GrokActiveSession>> {
    let active_sessions_path = grok_home.join(ACTIVE_SESSIONS_FILE);
    if !active_sessions_path.is_file() {
        return Ok(Vec::new());
    }
    serde_json::from_slice(&fs::read(active_sessions_path)?).context("parse active_sessions.json")
}

fn latest_assistant_message_from_chat_history(chat_history_path: &Path) -> Option<String> {
    let file = fs::File::open(chat_history_path).ok()?;
    let reader = BufReader::new(file);
    let mut latest_message = None;
    for line in reader.lines().map_while(Result::ok) {
        let value = serde_json::from_str::<Value>(&line).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let content = value
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned);
        if let Some(content) = content {
            latest_message = Some(content);
        }
    }
    latest_message
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|timestamp| timestamp.unix_timestamp() * 1_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_grok_sessions_from_summary_json() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let grok_home = temp_dir.path();
        let session_dir = grok_home
            .join(SESSIONS_DIR)
            .join("%2Ftmp%2Fproject")
            .join("session-1");
        fs::create_dir_all(&session_dir).expect("create session dir");
        fs::write(
            session_dir.join(SUMMARY_FILE),
            serde_json::json!({
                "info": {
                    "id": "session-1",
                    "cwd": "/tmp/project"
                },
                "session_summary": "Ship Grok Build hooks",
                "generated_title": "Grok Build hooks",
                "updated_at": "2026-06-06T12:00:00Z"
            })
            .to_string(),
        )
        .expect("write summary");
        fs::write(session_dir.join(UPDATES_FILE), "{}\n").expect("write updates");
        fs::write(
            grok_home.join(ACTIVE_SESSIONS_FILE),
            serde_json::json!([{
                "session_id": "session-1",
                "cwd": "/tmp/project"
            }])
            .to_string(),
        )
        .expect("write active sessions");

        let sessions = discover_grok_sessions(grok_home).expect("discover sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-1");
        assert!(sessions[0].running);
        assert_eq!(sessions[0].title.as_deref(), Some("Grok Build hooks"));

        let desktop_thread = grok_session_to_desktop_thread(&sessions[0]);
        assert_eq!(desktop_thread.source.as_deref(), Some("grok-build"));
        assert_eq!(
            desktop_thread.capabilities.assistant_kind,
            AssistantKind::GrokBuild
        );
    }
}

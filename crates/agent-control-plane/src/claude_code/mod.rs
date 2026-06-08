use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::assistant::AssistantKind;
use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities};
use crate::control_plane::DesktopThread;
use crate::mobile_session::{MOBILE_SESSION_STATUS_ACTIVE, MOBILE_SESSION_STATUS_STOPPED};

const CLAUDE_PROJECTS_DIR: &str = "projects";
const CLAUDE_THREAD_PREFIX: &str = "claude:";
const CLAUDE_SOURCE: &str = "claude-code";
const CLAUDE_ORIGINATOR: &str = "Claude Code";
const JSONL_EXTENSION: &str = "jsonl";
const TEXT_CONTENT_TYPE: &str = "text";
const USER_MESSAGE_TYPE: &str = "user";
const ASSISTANT_MESSAGE_TYPE: &str = "assistant";
const MAX_SUMMARY_CHARACTERS: usize = 160;
const NANOS_PER_MILLISECOND: i128 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeSessionRecord {
    pub session_id: String,
    pub thread_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub transcript_path: PathBuf,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub assistant_preview: Option<String>,
    pub running: bool,
}

#[derive(Default)]
struct ClaudeSessionDraft {
    session_id: Option<String>,
    cwd: Option<String>,
    created_at_ms: Option<i64>,
    updated_at_ms: Option<i64>,
    first_user_message: Option<String>,
    latest_assistant_message: Option<String>,
}

pub fn default_claude_home(home: &Path) -> PathBuf {
    home.join(".claude")
}

pub fn discover_claude_sessions(claude_home: &Path) -> Result<Vec<ClaudeSessionRecord>> {
    discover_claude_sessions_with_processes(claude_home, &current_process_commands())
}

pub fn discover_claude_sessions_with_processes(
    claude_home: &Path,
    process_commands: &[String],
) -> Result<Vec<ClaudeSessionRecord>> {
    let projects_root = claude_home.join(CLAUDE_PROJECTS_DIR);
    if !projects_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    for project_dir in fs::read_dir(&projects_root)
        .with_context(|| format!("read Claude projects root {}", projects_root.display()))?
    {
        let project_dir = project_dir?.path();
        if !project_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&project_dir)
            .with_context(|| format!("read Claude project {}", project_dir.display()))?
        {
            let path = entry?.path();
            if !is_jsonl_file(&path) {
                continue;
            }
            if let Some(record) = read_claude_session_file(&path, process_commands)? {
                records.push(record);
            }
        }
    }

    records.sort_by(|left, right| {
        right
            .updated_at_ms
            .unwrap_or_default()
            .cmp(&left.updated_at_ms.unwrap_or_default())
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    Ok(records)
}

pub fn claude_session_to_desktop_thread(session: &ClaudeSessionRecord) -> DesktopThread {
    let runtime_status = if session.running {
        MOBILE_SESSION_STATUS_ACTIVE
    } else {
        MOBILE_SESSION_STATUS_STOPPED
    };

    DesktopThread {
        thread_id: session.thread_id.clone(),
        title: session.title.clone(),
        cwd: session.cwd.clone(),
        transcript_path: Some(session.transcript_path.display().to_string()),
        source: Some(CLAUDE_SOURCE.to_owned()),
        originator: Some(CLAUDE_ORIGINATOR.to_owned()),
        model: None,
        reasoning_effort: None,
        git_sha: None,
        git_branch: None,
        cli_version: None,
        agent_nickname: Some(CLAUDE_ORIGINATOR.to_owned()),
        agent_role: Some(CLAUDE_SOURCE.to_owned()),
        agent_path: None,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        assistant_preview: session.assistant_preview.clone(),
        runtime_status: Some(runtime_status.to_owned()),
        archived: false,
        goal: None,
        capabilities: ThreadCapabilities {
            thread_id: session.thread_id.clone(),
            assistant_kind: AssistantKind::ClaudeCode,
            tools: Vec::new(),
            mcp_tools: Vec::new(),
            app_tools: Vec::new(),
            automation_tools: Vec::new(),
            spawn: SpawnGraph {
                parent_thread_id: None,
                root_thread_id: session.thread_id.clone(),
                children: Vec::new(),
                launch_kind: LaunchKind::Main,
            },
            diff: DiffSummary {
                git_branch: None,
                git_sha: None,
                produced_file_changes: false,
                paths: Vec::new(),
            },
            agent_nickname: Some(CLAUDE_ORIGINATOR.to_owned()),
            agent_role: Some(CLAUDE_SOURCE.to_owned()),
            agent_path: None,
        },
    }
}

fn read_claude_session_file(
    path: &Path,
    process_commands: &[String],
) -> Result<Option<ClaudeSessionRecord>> {
    let file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let mut draft = ClaudeSessionDraft::default();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        update_claude_session_draft(&mut draft, &value);
    }

    let Some(session_id) = draft.session_id else {
        return Ok(None);
    };
    let updated_at_ms = draft.updated_at_ms.or_else(|| file_modified_at_ms(path));
    let thread_id = format!("{CLAUDE_THREAD_PREFIX}{session_id}");
    let running = claude_session_is_running(&session_id, path, process_commands);
    Ok(Some(ClaudeSessionRecord {
        session_id,
        thread_id,
        title: draft.first_user_message.map(compact_summary),
        cwd: draft.cwd,
        transcript_path: path.to_path_buf(),
        created_at_ms: draft.created_at_ms,
        updated_at_ms,
        assistant_preview: draft.latest_assistant_message.map(compact_summary),
        running,
    }))
}

fn update_claude_session_draft(draft: &mut ClaudeSessionDraft, value: &Value) {
    if draft.session_id.is_none() {
        draft.session_id = string_field(value, "sessionId");
    }
    if draft.cwd.is_none() {
        draft.cwd = string_field(value, "cwd");
    }
    if let Some(timestamp_ms) =
        string_field(value, "timestamp").and_then(|value| parse_rfc3339_ms(&value))
    {
        draft.created_at_ms = draft.created_at_ms.or(Some(timestamp_ms));
        draft.updated_at_ms = Some(timestamp_ms);
    }

    match value.get("type").and_then(Value::as_str) {
        Some(USER_MESSAGE_TYPE) => {
            if draft.first_user_message.is_none() {
                draft.first_user_message = message_text(value);
            }
        }
        Some(ASSISTANT_MESSAGE_TYPE) => {
            if let Some(text) = message_text(value) {
                draft.latest_assistant_message = Some(text);
            }
        }
        _ => {}
    }
}

fn message_text(value: &Value) -> Option<String> {
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))?;
    if let Some(text) = content.as_str() {
        return clean_text(text);
    }
    content.as_array()?.iter().find_map(|item| {
        if item.get("type").and_then(Value::as_str) != Some(TEXT_CONTENT_TYPE) {
            return None;
        }
        item.get("text")
            .and_then(Value::as_str)
            .and_then(clean_text)
    })
}

fn clean_text(value: &str) -> Option<String> {
    let text = value.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

fn compact_summary(value: String) -> String {
    let mut chars = value.chars();
    let summary = chars
        .by_ref()
        .take(MAX_SUMMARY_CHARACTERS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{summary}...")
    } else {
        summary
    }
}

fn claude_session_is_running(
    session_id: &str,
    transcript_path: &Path,
    process_commands: &[String],
) -> bool {
    let session_id = session_id.to_ascii_lowercase();
    let transcript_path = transcript_path.display().to_string().to_ascii_lowercase();
    process_commands.iter().any(|command| {
        let command = command.to_ascii_lowercase();
        command.contains(&session_id) || command.contains(&transcript_path)
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn is_jsonl_file(path: &Path) -> bool {
    path.is_file()
        && path.extension().and_then(|extension| extension.to_str()) == Some(JSONL_EXTENSION)
}

fn file_modified_at_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(duration.as_millis()).ok()
}

fn parse_rfc3339_ms(value: &str) -> Option<i64> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339).ok()?;
    i64::try_from(timestamp.unix_timestamp_nanos() / NANOS_PER_MILLISECOND).ok()
}

fn current_process_commands() -> Vec<String> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "command="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn discovers_project_jsonl_sessions_without_leaking_raw_shape() {
        let temp_dir = tempdir().expect("tempdir");
        let claude_home = temp_dir.path().join(".claude");
        let project_dir = claude_home.join("projects").join("-tmp-project");
        fs::create_dir_all(&project_dir).expect("project dir");
        let transcript_path = project_dir.join("session-1.jsonl");
        fs::write(
            &transcript_path,
            r#"{"type":"user","sessionId":"session-1","cwd":"/tmp/project","timestamp":"2026-06-08T10:00:00.000Z","message":{"role":"user","content":"Build Claude support"}}
{"type":"assistant","sessionId":"session-1","cwd":"/tmp/project","timestamp":"2026-06-08T10:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Claude support is visible."}]}}
"#,
        )
        .expect("write transcript");

        let sessions = discover_claude_sessions_with_processes(
            &claude_home,
            &[format!("claude --resume session-1")],
        )
        .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].thread_id, "claude:session-1");
        assert_eq!(sessions[0].title.as_deref(), Some("Build Claude support"));
        assert_eq!(
            sessions[0].assistant_preview.as_deref(),
            Some("Claude support is visible.")
        );
        assert_eq!(sessions[0].cwd.as_deref(), Some("/tmp/project"));
        assert!(sessions[0].running);

        let thread = claude_session_to_desktop_thread(&sessions[0]);
        assert_eq!(thread.source.as_deref(), Some("claude-code"));
        assert_eq!(thread.originator.as_deref(), Some("Claude Code"));
        assert_eq!(
            thread.capabilities.assistant_kind,
            AssistantKind::ClaudeCode
        );
    }
}

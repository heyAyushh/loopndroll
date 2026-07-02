// allow: SIZE_OK — Claude session discovery boundary keeps filesystem scan, transcript parsing, and bounded previews consistent.
use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::assistant::AssistantKind;
use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities};
use crate::control_plane::DesktopThread;
use crate::mobile::session::{MOBILE_SESSION_STATUS_ACTIVE, MOBILE_SESSION_STATUS_STOPPED};

mod hooks;

pub use hooks::{
    ClaudeHookOwner, ClaudeHookRegistrationChange, ClaudeHookStatus, inspect_claude_hooks,
    is_claude_hook_invocation, parse_claude_hook_payload, register_owned_claude_hooks,
    unregister_owned_claude_hooks,
};

const CLAUDE_PROJECTS_DIR: &str = "projects";
const CLAUDE_THREAD_PREFIX: &str = "claude:";
const CLAUDE_SOURCE: &str = "claude-code";
const CLAUDE_ORIGINATOR: &str = "Claude Code";
const JSONL_EXTENSION: &str = "jsonl";
const TEXT_CONTENT_TYPE: &str = "text";
const USER_MESSAGE_TYPE: &str = "user";
const ASSISTANT_MESSAGE_TYPE: &str = "assistant";
const MAX_SUMMARY_CHARACTERS: usize = 160;
const FAST_FRONT_LINE_LIMIT: usize = 256;
const FAST_TAIL_BYTE_LIMIT: u64 = 128 * 1024;
const NANOS_PER_MILLISECOND: i128 = 1_000_000;
const CLAUDE_TRANSCRIPT_PATH_MARKER: &str = "/.claude/";
const JSONL_PATH_SUFFIX: &str = ".jsonl";
const TITLE_FIELD_NAMES: &[&str] = &["title", "summary", "sessionTitle", "session_title"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeSessionRecord {
    pub session_id: String,
    pub thread_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub transcript_path: PathBuf,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub latest_message_at_ms: Option<i64>,
    pub assistant_preview: Option<String>,
    pub first_user_prompt: Option<String>,
    pub running: bool,
}

#[derive(Default)]
struct ClaudeSessionDraft {
    session_id: Option<String>,
    cwd: Option<String>,
    created_at_ms: Option<i64>,
    updated_at_ms: Option<i64>,
    title: Option<String>,
    first_user_message: Option<String>,
    latest_assistant_message: Option<String>,
}

#[derive(Clone, Debug)]
struct ClaudeSessionCandidate {
    path: PathBuf,
    modified_at_ms: Option<i64>,
    mentioned_by_process: bool,
}

pub fn default_claude_home(home: &Path) -> PathBuf {
    home.join(".claude")
}

pub fn discover_claude_sessions(claude_home: &Path) -> Result<Vec<ClaudeSessionRecord>> {
    discover_claude_sessions_with_limit_and_processes(
        claude_home,
        None,
        &current_process_commands(),
    )
}

pub fn discover_recent_claude_sessions(
    claude_home: &Path,
    session_limit: usize,
) -> Result<Vec<ClaudeSessionRecord>> {
    discover_claude_sessions_with_limit_and_processes(
        claude_home,
        Some(session_limit),
        &current_process_commands(),
    )
}

pub fn discover_recent_claude_sessions_with_processes(
    claude_home: &Path,
    session_limit: usize,
    process_commands: &[String],
) -> Result<Vec<ClaudeSessionRecord>> {
    discover_claude_sessions_with_limit_and_processes(
        claude_home,
        Some(session_limit),
        process_commands,
    )
}

pub fn discover_claude_sessions_with_processes(
    claude_home: &Path,
    process_commands: &[String],
) -> Result<Vec<ClaudeSessionRecord>> {
    discover_claude_sessions_with_limit_and_processes(claude_home, None, process_commands)
}

fn discover_claude_sessions_with_limit_and_processes(
    claude_home: &Path,
    session_limit: Option<usize>,
    process_commands: &[String],
) -> Result<Vec<ClaudeSessionRecord>> {
    let projects_root = claude_home.join(CLAUDE_PROJECTS_DIR);
    if !projects_root.is_dir() {
        return Ok(Vec::new());
    }

    let process_transcript_paths = process_mentioned_transcript_paths(process_commands);
    let mut candidates =
        collect_claude_session_candidates(&projects_root, &process_transcript_paths)?;
    if let Some(limit) = session_limit {
        candidates = select_recent_claude_session_candidates(candidates, limit);
    }

    let mut records = Vec::new();
    for candidate in candidates {
        let record = if session_limit.is_some() {
            read_claude_session_file_fast(&candidate.path, process_commands)?
        } else {
            read_claude_session_file(&candidate.path, process_commands)?
        };
        if let Some(record) = record {
            records.push(record);
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

fn collect_claude_session_candidates(
    projects_root: &Path,
    process_transcript_paths: &BTreeSet<String>,
) -> Result<Vec<ClaudeSessionCandidate>> {
    let mut candidates = Vec::new();
    for project_dir in fs::read_dir(projects_root)
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
            candidates.push(ClaudeSessionCandidate {
                mentioned_by_process: process_transcript_paths.contains(&normalized_path(&path)),
                modified_at_ms: file_modified_at_ms(&path),
                path,
            });
        }
    }

    Ok(candidates)
}

fn select_recent_claude_session_candidates(
    mut candidates: Vec<ClaudeSessionCandidate>,
    session_limit: usize,
) -> Vec<ClaudeSessionCandidate> {
    if session_limit == 0 {
        return Vec::new();
    }

    candidates.sort_by(|left, right| {
        right
            .modified_at_ms
            .unwrap_or_default()
            .cmp(&left.modified_at_ms.unwrap_or_default())
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut selected = Vec::new();
    let mut selected_paths = BTreeSet::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.mentioned_by_process)
    {
        if selected_paths.insert(candidate.path.clone()) {
            selected.push(candidate.clone());
        }
    }
    for candidate in candidates.into_iter().take(session_limit) {
        if selected_paths.insert(candidate.path.clone()) {
            selected.push(candidate);
        }
    }
    selected
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
        latest_message_at_ms: session.latest_message_at_ms,
        assistant_preview: session.assistant_preview.clone(),
        first_user_prompt: session.first_user_prompt.clone(),
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
        title: draft.title.map(compact_summary),
        cwd: draft.cwd,
        transcript_path: path.to_path_buf(),
        created_at_ms: draft.created_at_ms,
        updated_at_ms,
        latest_message_at_ms: updated_at_ms,
        assistant_preview: draft.latest_assistant_message.map(compact_summary),
        first_user_prompt: draft.first_user_message.map(compact_summary),
        running,
    }))
}

fn read_claude_session_file_fast(
    path: &Path,
    process_commands: &[String],
) -> Result<Option<ClaudeSessionRecord>> {
    let mut draft = ClaudeSessionDraft::default();
    read_claude_session_front(path, &mut draft)?;
    read_claude_session_tail(path, &mut draft)?;

    let Some(session_id) = draft.session_id.or_else(|| session_id_from_path(path)) else {
        return Ok(None);
    };
    let updated_at_ms = draft.updated_at_ms.or_else(|| file_modified_at_ms(path));
    let thread_id = format!("{CLAUDE_THREAD_PREFIX}{session_id}");
    let running = claude_session_is_running(&session_id, path, process_commands);
    Ok(Some(ClaudeSessionRecord {
        session_id,
        thread_id,
        title: draft.title.map(compact_summary),
        cwd: draft.cwd,
        transcript_path: path.to_path_buf(),
        created_at_ms: draft.created_at_ms,
        updated_at_ms,
        latest_message_at_ms: updated_at_ms,
        assistant_preview: draft.latest_assistant_message.map(compact_summary),
        first_user_prompt: draft.first_user_message.map(compact_summary),
        running,
    }))
}

fn read_claude_session_front(path: &Path, draft: &mut ClaudeSessionDraft) -> Result<()> {
    let file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    for line in BufReader::new(file).lines().take(FAST_FRONT_LINE_LIMIT) {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        update_claude_session_draft(draft, &value);
        if draft.first_user_message.is_some()
            && draft.session_id.is_some()
            && draft.cwd.is_some()
            && draft.title.is_some()
        {
            break;
        }
    }
    Ok(())
}

fn read_claude_session_tail(path: &Path, draft: &mut ClaudeSessionDraft) -> Result<()> {
    let mut file = fs::File::open(path).with_context(|| format!("read {}", path.display()))?;
    let file_size = file.metadata()?.len();
    let start_offset = file_size.saturating_sub(FAST_TAIL_BYTE_LIMIT);
    file.seek(SeekFrom::Start(start_offset))?;

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let lines = if start_offset == 0 {
        text.lines().collect::<Vec<_>>()
    } else {
        // The first slice line may be partial when seeking into the middle of a JSONL file.
        text.lines().skip(1).collect::<Vec<_>>()
    };

    for line in lines.into_iter().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        update_claude_session_metadata(draft, &value);
        if draft.latest_assistant_message.is_none()
            && value.get("type").and_then(Value::as_str) == Some(ASSISTANT_MESSAGE_TYPE)
        {
            draft.latest_assistant_message = message_text(&value);
        }
        if draft.latest_assistant_message.is_some() && draft.session_id.is_some() {
            break;
        }
    }
    Ok(())
}

fn update_claude_session_draft(draft: &mut ClaudeSessionDraft, value: &Value) {
    update_claude_session_metadata(draft, value);

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

fn update_claude_session_metadata(draft: &mut ClaudeSessionDraft, value: &Value) {
    if draft.session_id.is_none() {
        draft.session_id = string_field(value, "sessionId");
    }
    if draft.cwd.is_none() {
        draft.cwd = string_field(value, "cwd");
    }
    if draft.title.is_none() {
        draft.title = session_title_field(value);
    }
    if let Some(timestamp_ms) =
        string_field(value, "timestamp").and_then(|value| parse_rfc3339_ms(&value))
    {
        draft.created_at_ms = draft.created_at_ms.or(Some(timestamp_ms));
        draft.updated_at_ms = draft.updated_at_ms.max(Some(timestamp_ms));
    }
}

fn session_title_field(value: &Value) -> Option<String> {
    TITLE_FIELD_NAMES
        .iter()
        .find_map(|field_name| string_field(value, field_name))
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

fn session_id_from_path(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn parse_rfc3339_ms(value: &str) -> Option<i64> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339).ok()?;
    i64::try_from(timestamp.unix_timestamp_nanos() / NANOS_PER_MILLISECOND).ok()
}

fn process_mentioned_transcript_paths(process_commands: &[String]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for command in process_commands {
        for token in command.split_whitespace() {
            let token = token.trim_matches(shell_token_trim_character);
            let lowercase_token = token.to_ascii_lowercase();
            if !lowercase_token.contains(CLAUDE_TRANSCRIPT_PATH_MARKER) {
                continue;
            }
            let Some(jsonl_suffix_offset) = lowercase_token.find(JSONL_PATH_SUFFIX) else {
                continue;
            };
            let transcript_path = &lowercase_token[..jsonl_suffix_offset + JSONL_PATH_SUFFIX.len()];
            paths.insert(transcript_path.to_owned());
        }
    }
    paths
}

fn normalized_path(path: &Path) -> String {
    path.display().to_string().to_ascii_lowercase()
}

fn shell_token_trim_character(character: char) -> bool {
    matches!(
        character,
        '\'' | '"' | ',' | ';' | ')' | '(' | '[' | ']' | '{' | '}'
    )
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
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;

    const TEST_MTIME_GAP: Duration = Duration::from_millis(20);

    #[test]
    fn discovers_project_jsonl_sessions_without_leaking_raw_shape() {
        let temp_dir = tempdir().expect("tempdir");
        let claude_home = temp_dir.path().join(".claude");
        let project_dir = claude_home.join("projects").join("-tmp-project");
        fs::create_dir_all(&project_dir).expect("project dir");
        write_claude_transcript(
            &project_dir,
            "session-1",
            Some("Native Claude task"),
            "Build Claude support",
            "Claude support is visible.",
        );

        let sessions = discover_claude_sessions_with_processes(
            &claude_home,
            &["claude --resume session-1".to_owned()],
        )
        .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].thread_id, "claude:session-1");
        assert_eq!(sessions[0].title.as_deref(), Some("Native Claude task"));
        assert_eq!(
            sessions[0].first_user_prompt.as_deref(),
            Some("Build Claude support")
        );
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

    #[test]
    fn limited_discovery_reads_newest_claude_sessions() {
        let temp_dir = tempdir().expect("tempdir");
        let claude_home = temp_dir.path().join(".claude");
        let project_dir = claude_home.join("projects").join("-tmp-project");
        fs::create_dir_all(&project_dir).expect("project dir");
        write_claude_transcript(
            &project_dir,
            "older-session",
            Some("Older Claude title"),
            "Older Claude request",
            "Older Claude response",
        );
        thread::sleep(TEST_MTIME_GAP);
        write_claude_transcript(
            &project_dir,
            "newer-session",
            Some("Newer Claude title"),
            "Newer Claude request",
            "Newer Claude response",
        );

        let sessions =
            discover_claude_sessions_with_limit_and_processes(&claude_home, Some(1), &[])
                .expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "newer-session");
        assert_eq!(sessions[0].title.as_deref(), Some("Newer Claude title"));
        assert_eq!(
            sessions[0].first_user_prompt.as_deref(),
            Some("Newer Claude request")
        );
        assert_eq!(
            sessions[0].assistant_preview.as_deref(),
            Some("Newer Claude response")
        );
    }

    #[test]
    fn limited_discovery_keeps_running_claude_session() {
        let temp_dir = tempdir().expect("tempdir");
        let claude_home = temp_dir.path().join(".claude");
        let project_dir = claude_home.join("projects").join("-tmp-project");
        fs::create_dir_all(&project_dir).expect("project dir");
        let active_transcript = write_claude_transcript(
            &project_dir,
            "active-session",
            None,
            "Active Claude request",
            "Active Claude response",
        );
        thread::sleep(TEST_MTIME_GAP);
        write_claude_transcript(
            &project_dir,
            "newer-session",
            None,
            "Newer Claude request",
            "Newer Claude response",
        );

        let sessions = discover_claude_sessions_with_limit_and_processes(
            &claude_home,
            Some(1),
            &[format!("claude --debug {}", active_transcript.display())],
        )
        .expect("sessions");
        let session_ids = sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(session_ids.len(), 2);
        assert!(session_ids.contains(&"newer-session"));
        assert!(session_ids.contains(&"active-session"));
        assert!(
            sessions
                .iter()
                .any(|session| { session.session_id == "active-session" && session.running })
        );
    }

    fn write_claude_transcript(
        project_dir: &Path,
        session_id: &str,
        title: Option<&str>,
        user_message: &str,
        assistant_message: &str,
    ) -> PathBuf {
        let transcript_path = project_dir.join(format!("{session_id}.jsonl"));
        let title_field = title
            .map(|title| format!(r#","title":"{title}""#))
            .unwrap_or_default();
        let content = format!(
            r#"{{"type":"user","sessionId":"{session_id}","cwd":"/tmp/project","timestamp":"2026-06-08T10:00:00.000Z"{title_field},"message":{{"role":"user","content":"{user_message}"}}}}
{{"type":"assistant","sessionId":"{session_id}","cwd":"/tmp/project","timestamp":"2026-06-08T10:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{assistant_message}"}}]}}}}
"#
        );
        fs::write(&transcript_path, content).expect("write transcript");
        transcript_path
    }
}

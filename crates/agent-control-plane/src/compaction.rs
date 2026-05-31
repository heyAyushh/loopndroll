use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::codex::discover_sources;

const CODEX_COMPACTION_PAYLOAD_TYPE: &str = "context_compacted";
pub const LOCAL_COMPACTION_EVENT_TYPE: &str = "codex.context_compacted";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionEvent {
    pub event_id: String,
    pub event_type: String,
    pub thread_id: String,
    pub occurred_at: String,
    pub rollout_path: String,
    pub line_number: usize,
}

pub fn read_compaction_events(codex_home: &Path) -> Result<Vec<CompactionEvent>> {
    read_compaction_events_with_limits(codex_home, None, None)
}

pub fn read_recent_compaction_events(
    codex_home: &Path,
    event_limit: usize,
    file_scan_limit: usize,
) -> Result<Vec<CompactionEvent>> {
    read_compaction_events_with_limits(codex_home, Some(event_limit), Some(file_scan_limit))
}

fn read_compaction_events_with_limits(
    codex_home: &Path,
    event_limit: Option<usize>,
    file_scan_limit: Option<usize>,
) -> Result<Vec<CompactionEvent>> {
    let sources = discover_sources(codex_home);
    let mut rollout_paths = Vec::new();
    collect_session_rollouts(&sources.sessions_root, &mut rollout_paths)?;
    rollout_paths.sort_by(|left, right| {
        rollout_modified_ms(right)
            .cmp(&rollout_modified_ms(left))
            .then_with(|| right.cmp(left))
    });

    let mut events = Vec::new();
    for rollout_path in rollout_paths
        .into_iter()
        .take(file_scan_limit.unwrap_or(usize::MAX))
    {
        events.extend(read_rollout_compactions(&rollout_path)?);
        if let Some(event_limit) = event_limit
            && events.len() >= event_limit
        {
            break;
        }
    }
    events.sort_by(|left, right| {
        right
            .occurred_at
            .cmp(&left.occurred_at)
            .then_with(|| right.event_id.cmp(&left.event_id))
    });
    if let Some(event_limit) = event_limit {
        events.truncate(event_limit);
    }
    Ok(events)
}

fn rollout_modified_ms(path: &Path) -> u128 {
    path.metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn collect_session_rollouts(directory: &Path, rollout_paths: &mut Vec<PathBuf>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(directory).with_context(|| format!("read {}", directory.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_session_rollouts(&path, rollout_paths)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension == "jsonl")
        {
            rollout_paths.push(path);
        }
    }

    Ok(())
}

fn read_rollout_compactions(rollout_path: &Path) -> Result<Vec<CompactionEvent>> {
    let file =
        File::open(rollout_path).with_context(|| format!("open {}", rollout_path.display()))?;
    let reader = BufReader::new(file);
    let mut thread_id = None;
    let mut events = Vec::new();

    for (line_index, line) in reader.lines().enumerate() {
        let line = line?;
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        if is_session_meta(&value) {
            thread_id = session_meta_thread_id(&value).map(str::to_owned);
            continue;
        }

        if is_context_compacted_event(&value) {
            let line_number = line_index + 1;
            let occurred_at = value
                .get("timestamp")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let thread_id = thread_id
                .clone()
                .unwrap_or_else(|| fallback_thread_id(rollout_path));
            events.push(CompactionEvent {
                event_id: stable_event_id(rollout_path, line_number, &occurred_at),
                event_type: LOCAL_COMPACTION_EVENT_TYPE.to_owned(),
                thread_id,
                occurred_at,
                rollout_path: rollout_path.display().to_string(),
                line_number,
            });
        }
    }

    Ok(events)
}

fn is_session_meta(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("session_meta")
}

fn session_meta_thread_id(value: &Value) -> Option<&str> {
    value
        .get("payload")
        .and_then(|payload| payload.get("id"))
        .and_then(Value::as_str)
}

fn is_context_compacted_event(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("event_msg")
        && value
            .get("payload")
            .and_then(|payload| payload.get("type"))
            .and_then(Value::as_str)
            == Some(CODEX_COMPACTION_PAYLOAD_TYPE)
}

fn fallback_thread_id(rollout_path: &Path) -> String {
    rollout_path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-thread")
        .to_owned()
}

fn stable_event_id(rollout_path: &Path, line_number: usize, occurred_at: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(rollout_path.display().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(line_number.to_string().as_bytes());
    hasher.update(b":");
    hasher.update(occurred_at.as_bytes());
    format!("compaction-{:x}", hasher.finalize())
}

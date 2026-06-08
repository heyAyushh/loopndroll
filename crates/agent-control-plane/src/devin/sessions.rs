use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::{DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH, DEVIN_STABLE_APP_SUPPORT_RELATIVE_PATH};
use crate::assistant::AssistantKind;
use crate::codex::{DiffSummary, LaunchKind, SpawnGraph, ThreadCapabilities, ThreadRecord};
use crate::control_plane::DesktopThread;
use crate::mobile_session::{MOBILE_SESSION_STATUS_ACTIVE, MOBILE_SESSION_STATUS_STOPPED};

const DEVIN_NEXT_ORIGINATOR: &str = "Devin - Next";
const DEVIN_STABLE_ORIGINATOR: &str = "Devin";
const DEVIN_DESKTOP_SOURCE: &str = "devin-desktop";
const DEVIN_THREAD_ID_PREFIX: &str = "devin";
const DEVIN_ACP_SESSION_PREFIX: &str = "acp/";
const DEVIN_IDLE_STATUS: &str = "idle";
const DEVIN_END_TURN_STATUS: &str = "end_turn";
const USER_RELATIVE_PATH: &str = "User";
const GLOBAL_STORAGE_RELATIVE_PATH: &str = "globalStorage";
const STATE_DB_FILE: &str = "state.vscdb";
const ACP_EVENTS_RELATIVE_PATH: &str = "acp-events";
const ITEM_TABLE_VALUE_QUERY: &str = "select value from ItemTable where key = ?1";
const METADATA_CACHE_KEY: &str = "windsurf.acp.metadataCache";
const EVENT_LOG_INDEX_KEY: &str = "windsurf.acp.eventLog.index";
const CREATED_AT_META_KEY: &str = "cognition.ai/createdAt";
const IS_ARCHIVED_META_KEY: &str = "cognition.ai/isArchived";
const SESSION_UPDATE_KEY: &str = "sessionUpdate";
const AGENT_MESSAGE_CHUNK_UPDATE: &str = "agent_message_chunk";
const TEXT_CONTENT_TYPE: &str = "text";
const CONTENT_TEXT_KEY: &str = "text";
const CONTENT_TYPE_KEY: &str = "type";
const STREAMING_MESSAGE_ID_META_KEY: &str = "cognition.ai/streamingMessageId";
const MESSAGE_ID_FALLBACK_PREFIX: &str = "message";
const MILLIS_PER_SECOND: i64 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevinSessionRecord {
    pub thread_id: String,
    pub session_id: String,
    pub provider_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub status: String,
    pub transcript_path: Option<PathBuf>,
    pub originator: String,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub assistant_preview: Option<String>,
    pub archived: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevinThreadIdentity {
    pub provider_id: String,
    pub session_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevinPromptTransport {
    CodexAppServer,
}

#[derive(Clone, Copy)]
struct DevinSessionSource {
    app_support_relative_path: &'static str,
    originator: &'static str,
}

#[derive(Debug, Deserialize)]
struct MetadataCache {
    #[serde(default)]
    sessions: Vec<MetadataSession>,
}

#[derive(Debug, Deserialize)]
struct MetadataSession {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "providerId")]
    provider_id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(rename = "updatedAt")]
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(rename = "sortUpdatedAt")]
    #[serde(default)]
    sort_updated_at: Option<String>,
    #[serde(rename = "_meta")]
    #[serde(default)]
    meta: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize, Default)]
struct EventLogEntry {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(rename = "lastUpdated")]
    #[serde(default)]
    last_updated_ms: Option<i64>,
}

type EventLogIndex = BTreeMap<String, EventLogEntry>;

pub fn discover_devin_sessions(home: &Path) -> Result<Vec<DevinSessionRecord>> {
    let mut sessions_by_id = BTreeMap::<String, DevinSessionRecord>::new();
    for source in devin_session_sources() {
        let app_support_path = home.join(source.app_support_relative_path);
        for session in discover_devin_sessions_for_source(&app_support_path, source.originator)? {
            keep_newer_session(&mut sessions_by_id, session);
        }
    }

    let mut sessions = sessions_by_id.into_values().collect::<Vec<_>>();
    sessions.sort_by(|left, right| {
        right
            .updated_at_ms
            .unwrap_or_default()
            .cmp(&left.updated_at_ms.unwrap_or_default())
            .then_with(|| left.session_id.cmp(&right.session_id))
    });
    Ok(sessions)
}

pub fn devin_session_to_desktop_thread(session: &DevinSessionRecord) -> DesktopThread {
    let runtime_status = if session.is_active() {
        MOBILE_SESSION_STATUS_ACTIVE
    } else {
        MOBILE_SESSION_STATUS_STOPPED
    };

    DesktopThread {
        thread_id: session.thread_id.clone(),
        title: session.title.clone(),
        cwd: session.cwd.clone(),
        transcript_path: session
            .transcript_path
            .as_ref()
            .map(|path| path.display().to_string()),
        source: Some(DEVIN_DESKTOP_SOURCE.to_owned()),
        originator: Some(session.originator.clone()),
        model: None,
        reasoning_effort: None,
        git_sha: None,
        git_branch: None,
        cli_version: None,
        agent_nickname: Some(session.originator.clone()),
        agent_role: Some(session.provider_id.clone()),
        agent_path: None,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        assistant_preview: session.assistant_preview.clone(),
        runtime_status: Some(runtime_status.to_owned()),
        archived: session.archived,
        capabilities: devin_session_capabilities(session),
    }
}

pub fn devin_session_to_thread_record(session: &DevinSessionRecord) -> ThreadRecord {
    ThreadRecord {
        thread_id: session.thread_id.clone(),
        title: session.title.clone(),
        cwd: session.cwd.clone(),
        transcript_path: session
            .transcript_path
            .as_ref()
            .map(|path| path.display().to_string()),
        source: Some(DEVIN_DESKTOP_SOURCE.to_owned()),
        originator: Some(session.originator.clone()),
        model: None,
        reasoning_effort: None,
        git_sha: None,
        git_branch: None,
        cli_version: None,
        agent_nickname: Some(session.originator.clone()),
        agent_role: Some(session.provider_id.clone()),
        agent_path: None,
        created_at_ms: session.created_at_ms,
        updated_at_ms: session.updated_at_ms,
        archived: session.archived,
    }
}

pub fn devin_session_capabilities(session: &DevinSessionRecord) -> ThreadCapabilities {
    ThreadCapabilities {
        thread_id: session.thread_id.clone(),
        assistant_kind: AssistantKind::DevinDesktop,
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
        agent_nickname: Some(session.originator.clone()),
        agent_role: Some(session.provider_id.clone()),
        agent_path: None,
    }
}

impl DevinSessionRecord {
    pub fn is_active(&self) -> bool {
        !self.archived && !devin_status_is_resting(&self.status)
    }
}

fn devin_session_sources() -> [DevinSessionSource; 2] {
    [
        DevinSessionSource {
            app_support_relative_path: DEVIN_STABLE_APP_SUPPORT_RELATIVE_PATH,
            originator: DEVIN_STABLE_ORIGINATOR,
        },
        DevinSessionSource {
            app_support_relative_path: DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH,
            originator: DEVIN_NEXT_ORIGINATOR,
        },
    ]
}

fn discover_devin_sessions_for_source(
    app_support_path: &Path,
    originator: &str,
) -> Result<Vec<DevinSessionRecord>> {
    let state_db_path = app_support_path
        .join(USER_RELATIVE_PATH)
        .join(GLOBAL_STORAGE_RELATIVE_PATH)
        .join(STATE_DB_FILE);
    if !state_db_path.is_file() {
        return Ok(Vec::new());
    }

    let connection = Connection::open_with_flags(&state_db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("open Devin state DB {}", state_db_path.display()))?;
    let metadata_cache = read_json_item::<MetadataCache>(&connection, METADATA_CACHE_KEY)?
        .unwrap_or(MetadataCache {
            sessions: Vec::new(),
        });
    let event_log_index =
        read_json_item::<EventLogIndex>(&connection, EVENT_LOG_INDEX_KEY)?.unwrap_or_default();
    let events_path = app_support_path
        .join(USER_RELATIVE_PATH)
        .join(ACP_EVENTS_RELATIVE_PATH);

    Ok(metadata_cache
        .sessions
        .into_iter()
        .map(|session| {
            session_record_from_metadata(session, &event_log_index, &events_path, originator)
        })
        .collect())
}

fn session_record_from_metadata(
    session: MetadataSession,
    event_log_index: &EventLogIndex,
    events_path: &Path,
    originator: &str,
) -> DevinSessionRecord {
    let event_entry = event_log_index.get(&session.session_id);
    let transcript_path = event_entry
        .and_then(|entry| entry.uuid.as_deref())
        .map(|uuid| events_path.join(format!("{uuid}.ndjson")))
        .filter(|path| path.is_file());
    let created_at_ms = session
        .meta
        .get(CREATED_AT_META_KEY)
        .and_then(Value::as_str)
        .and_then(parse_timestamp_ms);
    let updated_at_ms = latest_timestamp_ms([
        session
            .sort_updated_at
            .as_deref()
            .and_then(parse_timestamp_ms),
        session.updated_at.as_deref().and_then(parse_timestamp_ms),
        event_entry.and_then(|entry| entry.last_updated_ms),
        created_at_ms,
    ]);
    let archived = session
        .meta
        .get(IS_ARCHIVED_META_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let assistant_preview = transcript_path
        .as_deref()
        .and_then(latest_assistant_message_from_event_log);

    DevinSessionRecord {
        thread_id: public_thread_id_for_session_id(&session.session_id),
        session_id: session.session_id,
        provider_id: session.provider_id,
        title: non_empty_string(session.title),
        cwd: non_empty_string(session.cwd),
        status: session
            .status
            .filter(|status| !status.trim().is_empty())
            .unwrap_or_else(|| DEVIN_IDLE_STATUS.to_owned()),
        transcript_path,
        originator: originator.to_owned(),
        created_at_ms,
        updated_at_ms,
        assistant_preview,
        archived,
    }
}

fn read_json_item<T>(connection: &Connection, key: &str) -> Result<Option<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let value = connection
        .query_row(ITEM_TABLE_VALUE_QUERY, [key], |row| {
            sqlite_value_bytes(row.get_ref(0)?)
        })
        .optional()?;
    value
        .map(|bytes| serde_json::from_slice(&bytes).with_context(|| format!("parse {key}")))
        .transpose()
}

fn sqlite_value_bytes(value: ValueRef<'_>) -> rusqlite::Result<Vec<u8>> {
    match value {
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => Ok(bytes.to_vec()),
        _ => Ok(Vec::new()),
    }
}

fn latest_assistant_message_from_event_log(event_log_path: &Path) -> Option<String> {
    let file = File::open(event_log_path).ok()?;
    let reader = BufReader::new(file);
    let mut messages_by_id = BTreeMap::<String, String>::new();
    let mut latest_message_id = None;

    for (line_index, line) in reader.lines().map_while(Result::ok).enumerate() {
        let value = serde_json::from_str::<Value>(&line).ok()?;
        let notification = value.get("notification")?;
        if notification.get(SESSION_UPDATE_KEY).and_then(Value::as_str)
            != Some(AGENT_MESSAGE_CHUNK_UPDATE)
        {
            continue;
        }
        let content = notification.get("content")?;
        if content.get(CONTENT_TYPE_KEY).and_then(Value::as_str) != Some(TEXT_CONTENT_TYPE) {
            continue;
        }
        let text = content.get(CONTENT_TEXT_KEY).and_then(Value::as_str)?;
        let message_id = streaming_message_id(notification)
            .unwrap_or_else(|| format!("{MESSAGE_ID_FALLBACK_PREFIX}-{line_index}"));
        messages_by_id
            .entry(message_id.clone())
            .or_default()
            .push_str(text);
        latest_message_id = Some(message_id);
    }

    latest_message_id
        .and_then(|message_id| messages_by_id.remove(&message_id))
        .map(|message| message.trim().to_owned())
        .filter(|message| !message.is_empty())
}

fn streaming_message_id(notification: &Value) -> Option<String> {
    notification
        .get("_meta")
        .and_then(Value::as_object)?
        .get(STREAMING_MESSAGE_ID_META_KEY)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn parse_timestamp_ms(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc3339)
        .ok()
        .map(|timestamp| timestamp.unix_timestamp() * MILLIS_PER_SECOND)
}

fn latest_timestamp_ms(values: [Option<i64>; 4]) -> Option<i64> {
    values.into_iter().flatten().max()
}

fn non_empty_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn devin_status_is_resting(status: &str) -> bool {
    matches!(status, DEVIN_IDLE_STATUS | DEVIN_END_TURN_STATUS)
}

fn keep_newer_session(
    sessions_by_id: &mut BTreeMap<String, DevinSessionRecord>,
    session: DevinSessionRecord,
) {
    let should_replace = sessions_by_id
        .get(&session.session_id)
        .map(|existing| {
            session.updated_at_ms.unwrap_or_default() > existing.updated_at_ms.unwrap_or_default()
        })
        .unwrap_or(true);
    if should_replace {
        sessions_by_id.insert(session.thread_id.clone(), session);
    }
}

fn public_thread_id_for_session_id(session_id: &str) -> String {
    let normalized_session_id = session_id
        .strip_prefix(DEVIN_ACP_SESSION_PREFIX)
        .unwrap_or(session_id)
        .replace('/', ":");
    format!("{DEVIN_THREAD_ID_PREFIX}:{normalized_session_id}")
}

pub fn devin_thread_identity_from_public_thread_id(thread_id: &str) -> Option<DevinThreadIdentity> {
    let remainder = thread_id.strip_prefix("devin:")?;
    let (provider_id, session_id) = remainder.split_once(':')?;
    let provider_id = non_empty_identity_segment(provider_id)?;
    let session_id = non_empty_identity_segment(&session_id.replace(':', "/"))?;
    Some(DevinThreadIdentity {
        provider_id,
        session_id,
    })
}

fn non_empty_identity_segment(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub fn devin_prompt_transport_for_provider(provider_id: &str) -> Option<DevinPromptTransport> {
    match provider_id.trim() {
        "codex" | "codex-acp" => Some(DevinPromptTransport::CodexAppServer),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn discovers_devin_desktop_sessions_for_all_agent_providers() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let home = temp_dir.path();
        let app_support = home.join(DEVIN_NEXT_APP_SUPPORT_RELATIVE_PATH);
        let state_db_path = app_support
            .join(USER_RELATIVE_PATH)
            .join(GLOBAL_STORAGE_RELATIVE_PATH)
            .join(STATE_DB_FILE);
        fs::create_dir_all(state_db_path.parent().expect("state parent")).expect("state parent");
        let events_path = app_support
            .join(USER_RELATIVE_PATH)
            .join(ACP_EVENTS_RELATIVE_PATH);
        fs::create_dir_all(&events_path).expect("events dir");
        fs::write(
            events_path.join("event-1.ndjson"),
            [
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "agent_thought_chunk",
                        "content": { "type": "text", "text": "hidden" },
                        "_meta": { STREAMING_MESSAGE_ID_META_KEY: "thought-1" }
                    }
                })
                .to_string(),
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "Hello " },
                        "_meta": { STREAMING_MESSAGE_ID_META_KEY: "assistant-1" }
                    }
                })
                .to_string(),
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": { "type": "text", "text": "there" },
                        "_meta": { STREAMING_MESSAGE_ID_META_KEY: "assistant-1" }
                    }
                })
                .to_string(),
            ]
            .join("\n"),
        )
        .expect("event log");
        write_state_db(
            &state_db_path,
            serde_json::json!({
                "sessions": [
                    {
                        "sessionId": "acp/devin-cli/brindle-cadet",
                        "providerId": "devin-cli",
                        "title": "hey",
                        "cwd": "/Users/test/project",
                        "status": "end_turn",
                        "updatedAt": "2026-06-07T03:10:03+00:00",
                        "_meta": {
                            CREATED_AT_META_KEY: "2026-06-07T03:09:52.477Z",
                            IS_ARCHIVED_META_KEY: false
                        }
                    },
                    {
                        "sessionId": "acp/codex/thread",
                        "providerId": "codex",
                        "title": "Codex inside Devin"
                    }
                ]
            }),
            serde_json::json!({
                "acp/devin-cli/brindle-cadet": {
                    "uuid": "event-1",
                    "eventCount": 3,
                    "lastUpdated": 1780801814955_i64
                }
            }),
        );

        let sessions = discover_devin_sessions(home).expect("sessions");

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].thread_id, "devin:devin-cli:brindle-cadet");
        assert_eq!(sessions[0].session_id, "acp/devin-cli/brindle-cadet");
        assert_eq!(sessions[0].provider_id, "devin-cli");
        assert_eq!(sessions[0].originator, DEVIN_NEXT_ORIGINATOR);
        assert_eq!(
            sessions[0].assistant_preview.as_deref(),
            Some("Hello there")
        );
        assert!(
            !sessions[0]
                .assistant_preview
                .as_deref()
                .unwrap()
                .contains("hidden")
        );
        assert!(!sessions[0].is_active());

        let codex_session = sessions
            .iter()
            .find(|session| session.provider_id == "codex")
            .expect("codex session hosted by Devin Desktop");
        assert_eq!(codex_session.thread_id, "devin:codex:thread");
        assert_eq!(codex_session.originator, DEVIN_NEXT_ORIGINATOR);
        assert_eq!(codex_session.title.as_deref(), Some("Codex inside Devin"));
    }

    #[test]
    fn discovers_devin_cloud_sessions_without_local_event_logs() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let home = temp_dir.path();
        let app_support = home.join(DEVIN_STABLE_APP_SUPPORT_RELATIVE_PATH);
        let state_db_path = app_support
            .join(USER_RELATIVE_PATH)
            .join(GLOBAL_STORAGE_RELATIVE_PATH)
            .join(STATE_DB_FILE);
        fs::create_dir_all(state_db_path.parent().expect("state parent")).expect("state parent");
        write_state_db(
            &state_db_path,
            serde_json::json!({
                "sessions": [
                    {
                        "sessionId": "acp/devin-cloud/devin-1",
                        "providerId": "devin-cloud",
                        "title": "Cloud task",
                        "status": "idle",
                        "sortUpdatedAt": "2026-05-05T22:55:58.021757+00:00",
                        "_meta": {
                            CREATED_AT_META_KEY: "2026-04-28T19:34:13.225485+00:00",
                            IS_ARCHIVED_META_KEY: true
                        }
                    }
                ]
            }),
            serde_json::json!({}),
        );

        let sessions = discover_devin_sessions(home).expect("sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].provider_id, "devin-cloud");
        assert_eq!(sessions[0].originator, DEVIN_STABLE_ORIGINATOR);
        assert!(sessions[0].transcript_path.is_none());
        assert!(sessions[0].archived);
    }

    #[test]
    fn recovers_native_agent_session_identity_from_public_thread_id() {
        assert_eq!(
            devin_thread_identity_from_public_thread_id("devin:devin-cli:shadow-canidae"),
            Some(DevinThreadIdentity {
                provider_id: "devin-cli".to_owned(),
                session_id: "shadow-canidae".to_owned(),
            })
        );
        assert_eq!(
            devin_thread_identity_from_public_thread_id("devin:codex:019e:path"),
            Some(DevinThreadIdentity {
                provider_id: "codex".to_owned(),
                session_id: "019e/path".to_owned(),
            })
        );
        assert_eq!(
            devin_thread_identity_from_public_thread_id("thread-main"),
            None
        );
    }

    #[test]
    fn prompt_transport_is_explicit_per_devin_provider() {
        assert_eq!(
            devin_prompt_transport_for_provider("codex-acp"),
            Some(DevinPromptTransport::CodexAppServer)
        );
        assert_eq!(devin_prompt_transport_for_provider("devin-cli"), None);
        assert_eq!(devin_prompt_transport_for_provider("claude-acp"), None);
        assert_eq!(devin_prompt_transport_for_provider("devin-cloud"), None);
    }

    fn write_state_db(state_db_path: &Path, metadata_cache: Value, event_log_index: Value) {
        let connection = Connection::open(state_db_path).expect("open db");
        connection
            .execute("create table ItemTable (key text, value blob)", [])
            .expect("create table");
        connection
            .execute(
                "insert into ItemTable (key, value) values (?1, ?2)",
                (METADATA_CACHE_KEY, metadata_cache.to_string()),
            )
            .expect("insert metadata");
        connection
            .execute(
                "insert into ItemTable (key, value) values (?1, ?2)",
                (EVENT_LOG_INDEX_KEY, event_log_index.to_string()),
            )
            .expect("insert index");
    }
}

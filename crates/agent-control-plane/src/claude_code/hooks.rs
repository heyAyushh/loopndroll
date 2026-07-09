// allow: SIZE_OK — Claude hook adapter keeps register/inspect/unregister JSON mutation semantics atomic.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::hook_registration::LOOPER_HOOK_MARKER;
use crate::mobile::session::MobileHookPayload;

const CLAUDE_SETTINGS_FILE: &str = "settings.json";
const CLAUDE_THREAD_PREFIX: &str = "claude:";
const LOOPER_CLAUDE_HOOK_ENV: &str = "LOOPER_CLAUDE_HOOK";
const LOOPER_CLAUDE_HOOK_VALUE: &str = "1";
const SESSION_HOOK_TIMEOUT_SECONDS: u64 = 30;
const STOP_HOOK_TIMEOUT_SECONDS: u64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClaudeHookRegistrationChange {
    pub removed_handlers: usize,
    pub installed_handlers: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaudeHookStatus {
    pub registered_events: Vec<String>,
    pub active_command: Option<String>,
    pub owner: ClaudeHookOwner,
    pub health: String,
    pub settings_path: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ClaudeHookOwner {
    LooperRust,
    Unknown,
    #[default]
    None,
}

pub fn default_claude_settings_path(claude_home: &Path) -> PathBuf {
    claude_home.join(CLAUDE_SETTINGS_FILE)
}

pub fn register_owned_claude_hooks(
    claude_home: &Path,
    hook_command: &str,
) -> Result<ClaudeHookRegistrationChange> {
    fs::create_dir_all(claude_home).with_context(|| format!("create {}", claude_home.display()))?;
    let settings_path = default_claude_settings_path(claude_home);
    let mut document = load_settings_document(&settings_path)?;
    let hooks = ensure_hooks_object(&mut document);
    let removed_handlers = remove_owned_hooks_from_events(hooks);
    upsert_owned_hook_groups(hooks, normalize_claude_hook_command(hook_command));

    let mut content = serde_json::to_string_pretty(&document)?;
    content.push('\n');
    fs::write(&settings_path, content)
        .with_context(|| format!("write {}", settings_path.display()))?;

    Ok(ClaudeHookRegistrationChange {
        removed_handlers,
        installed_handlers: owned_event_names().len(),
    })
}

pub fn unregister_owned_claude_hooks(claude_home: &Path) -> Result<usize> {
    let settings_path = default_claude_settings_path(claude_home);
    if !settings_path.exists() {
        return Ok(0);
    }

    let mut document: Value = serde_json::from_slice(
        &fs::read(&settings_path).with_context(|| format!("read {}", settings_path.display()))?,
    )
    .with_context(|| format!("parse {}", settings_path.display()))?;
    let removed_handlers = document
        .get_mut("hooks")
        .map(remove_owned_hooks_from_events)
        .unwrap_or(0);

    if removed_handlers > 0 {
        let mut content = serde_json::to_string_pretty(&document)?;
        content.push('\n');
        fs::write(&settings_path, content)
            .with_context(|| format!("write {}", settings_path.display()))?;
    }

    Ok(removed_handlers)
}

pub fn inspect_claude_hooks(claude_home: &Path) -> ClaudeHookStatus {
    let settings_path = default_claude_settings_path(claude_home);
    let inspection = read_hooks_json(&settings_path).unwrap_or_default();
    let health = if inspection.registered_events.is_empty() {
        "missing"
    } else if matches!(inspection.owner, ClaudeHookOwner::LooperRust) {
        "healthy"
    } else {
        "configured"
    }
    .to_owned();

    ClaudeHookStatus {
        registered_events: inspection.registered_events,
        active_command: inspection.active_command,
        owner: inspection.owner,
        health,
        settings_path: settings_path
            .exists()
            .then(|| settings_path.display().to_string()),
    }
}

pub fn is_claude_hook_invocation() -> bool {
    std::env::var(LOOPER_CLAUDE_HOOK_ENV).as_deref() == Ok(LOOPER_CLAUDE_HOOK_VALUE)
}

pub fn parse_claude_hook_payload(input: &str) -> Result<MobileHookPayload> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(MobileHookPayload {
            hook_event_name: String::new(),
            session_id: None,
            turn_id: None,
            cwd: None,
            last_assistant_message: None,
        });
    }

    let value: Value =
        serde_json::from_str(trimmed).context("parse Claude Code hook payload JSON from stdin")?;
    Ok(claude_payload_from_value(&value))
}

fn claude_payload_from_value(value: &Value) -> MobileHookPayload {
    let raw_session_id = first_string(value, &["session_id", "sessionId"]);
    MobileHookPayload {
        hook_event_name: first_string(value, &["hook_event_name", "hookEventName"])
            .map(|name| normalize_claude_event_name(&name))
            .unwrap_or_default(),
        session_id: raw_session_id
            .or_else(|| session_id_from_transcript_path(value))
            .map(|session_id| public_thread_id_for_claude_session(&session_id)),
        turn_id: first_string(value, &["turn_id", "turnId"]),
        cwd: first_string(value, &["cwd", "workspaceRoot", "workspace_root"]),
        last_assistant_message: first_string(
            value,
            &["last_assistant_message", "lastAssistantMessage"],
        ),
    }
}

fn session_id_from_transcript_path(value: &Value) -> Option<String> {
    let transcript_path = first_string(value, &["transcript_path", "transcriptPath"])?;
    Path::new(&transcript_path)
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_owned)
}

pub fn public_thread_id_for_claude_session(session_id: &str) -> String {
    let session_id = session_id.trim();
    if session_id.starts_with(CLAUDE_THREAD_PREFIX) {
        return session_id.to_owned();
    }
    format!("{CLAUDE_THREAD_PREFIX}{}", session_id.replace('/', ":"))
}

pub fn claude_session_id_from_public_thread_id(thread_id: &str) -> String {
    let thread_id = thread_id.trim();
    thread_id
        .strip_prefix(CLAUDE_THREAD_PREFIX)
        .unwrap_or(thread_id)
        .replace(':', "/")
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn normalize_claude_event_name(name: &str) -> String {
    match name {
        "stop" | "Stop" => "Stop".to_owned(),
        "session_start" | "SessionStart" => "SessionStart".to_owned(),
        "user_prompt_submit" | "UserPromptSubmit" => "UserPromptSubmit".to_owned(),
        "session_end" | "SessionEnd" => "SessionEnd".to_owned(),
        other => other.to_owned(),
    }
}

fn load_settings_document(settings_path: &Path) -> Result<Value> {
    if !settings_path.exists() {
        return Ok(json!({}));
    }

    serde_json::from_slice(
        &fs::read(settings_path).with_context(|| format!("read {}", settings_path.display()))?,
    )
    .with_context(|| format!("parse {}", settings_path.display()))
}

fn ensure_hooks_object(document: &mut Value) -> &mut Value {
    if !document.is_object() {
        *document = json!({});
    }
    match document {
        Value::Object(document) => {
            let hooks = document.entry("hooks").or_insert_with(|| json!({}));
            if !hooks.is_object() {
                *hooks = json!({});
            }
            hooks
        }
        _ => document,
    }
}

#[derive(Default)]
struct ClaudeHooksInspection {
    registered_events: Vec<String>,
    active_command: Option<String>,
    owner: ClaudeHookOwner,
}

fn read_hooks_json(path: &Path) -> Result<ClaudeHooksInspection> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let Some(object) = value.get("hooks").and_then(Value::as_object) else {
        return Ok(ClaudeHooksInspection::default());
    };
    let registered_events = object.keys().cloned().collect::<Vec<_>>();
    let owned_command = object
        .iter()
        .filter(|(event_name, _)| owned_event_names().contains(&event_name.as_str()))
        .find_map(|(_, event_value)| first_owned_hook_command(event_value))
        .map(str::to_owned);
    let fallback_command = object
        .values()
        .find_map(first_hook_command)
        .map(str::to_owned);
    let active_command = owned_command.or(fallback_command);
    let owner = if owned_event_names()
        .iter()
        .all(|event_name| object.get(*event_name).is_some_and(event_has_owned_hook))
    {
        ClaudeHookOwner::LooperRust
    } else {
        classify_hook_owner(active_command.as_deref())
    };
    Ok(ClaudeHooksInspection {
        registered_events,
        active_command,
        owner,
    })
}

fn first_hook_command(value: &Value) -> Option<&str> {
    if let Some(command) = hook_command(value) {
        return Some(command);
    }
    if let Some(hooks) = value.get("hooks") {
        return first_hook_command(hooks);
    }
    value.as_array()?.iter().find_map(first_hook_command)
}

fn first_owned_hook_command(value: &Value) -> Option<&str> {
    if let Some(command) = hook_command(value)
        && is_owned_hook_command(command)
    {
        return Some(command);
    }
    if let Some(hooks) = value.get("hooks") {
        return first_owned_hook_command(hooks);
    }
    value.as_array()?.iter().find_map(first_owned_hook_command)
}

fn event_has_owned_hook(value: &Value) -> bool {
    first_owned_hook_command(value).is_some()
}

fn classify_hook_owner(command: Option<&str>) -> ClaudeHookOwner {
    let Some(command) = command else {
        return ClaudeHookOwner::None;
    };
    if is_owned_hook_command(command) {
        ClaudeHookOwner::LooperRust
    } else {
        ClaudeHookOwner::Unknown
    }
}

fn normalize_claude_hook_command(hook_command: &str) -> String {
    let trimmed = hook_command.trim();
    let command = if is_owned_hook_command(trimmed) {
        trimmed.to_owned()
    } else {
        format!("{trimmed} {LOOPER_HOOK_MARKER}")
    };
    if command.contains(LOOPER_CLAUDE_HOOK_ENV) {
        command
    } else {
        format!("{LOOPER_CLAUDE_HOOK_ENV}={LOOPER_CLAUDE_HOOK_VALUE} {command}")
    }
}

fn owned_event_names() -> [&'static str; 3] {
    ["SessionStart", "Stop", "UserPromptSubmit"]
}

fn upsert_owned_hook_groups(hooks: &mut Value, hook_command: String) {
    let Some(hooks) = hooks.as_object_mut() else {
        return;
    };
    for event_name in owned_event_names() {
        prepend_owned_hook_group(
            hooks,
            event_name,
            json!({
                "hooks": [
                    owned_hook_handler(&hook_command, hook_timeout_for_event(event_name))
                ]
            }),
        );
    }
}

fn hook_timeout_for_event(event_name: &str) -> u64 {
    if event_name == "Stop" {
        STOP_HOOK_TIMEOUT_SECONDS
    } else {
        SESSION_HOOK_TIMEOUT_SECONDS
    }
}

fn prepend_owned_hook_group(hooks: &mut Map<String, Value>, event_name: &str, owned_group: Value) {
    let event_value = hooks
        .remove(event_name)
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let mut groups = match event_value {
        Value::Array(groups) => groups,
        value if event_is_empty(&value) => Vec::new(),
        value => vec![value],
    };
    groups.insert(0, owned_group);
    hooks.insert(event_name.to_owned(), Value::Array(groups));
}

fn owned_hook_handler(command: &str, timeout: u64) -> Value {
    json!({
        "type": "command",
        "command": command,
        "timeout": timeout,
    })
}

fn remove_owned_hooks_from_events(events: &mut Value) -> usize {
    let Some(events) = events.as_object_mut() else {
        return 0;
    };

    let mut removed_handlers = 0;
    let mut empty_event_names = Vec::new();
    for (event_name, event_value) in events.iter_mut() {
        removed_handlers += remove_owned_hooks_from_event(event_value);
        if event_is_empty(event_value) {
            empty_event_names.push(event_name.clone());
        }
    }
    for event_name in empty_event_names {
        events.remove(&event_name);
    }
    removed_handlers
}

fn remove_owned_hooks_from_event(event_value: &mut Value) -> usize {
    let Some(groups) = event_value.as_array_mut() else {
        return remove_owned_hook_handler(event_value) as usize;
    };

    let mut removed_handlers = 0;
    for group in groups.iter_mut() {
        removed_handlers += remove_owned_hooks_from_group(group);
    }
    groups.retain(|group| !event_is_empty(group));
    removed_handlers
}

fn remove_owned_hooks_from_group(group: &mut Value) -> usize {
    let Some(hooks) = group.get_mut("hooks") else {
        return remove_owned_hook_handler(group) as usize;
    };
    match hooks {
        Value::Array(handlers) => {
            let before = handlers.len();
            handlers.retain(|handler| {
                hook_command(handler).is_none_or(|command| !is_owned_hook_command(command))
            });
            before.saturating_sub(handlers.len())
        }
        value => remove_owned_hook_handler(value) as usize,
    }
}

fn remove_owned_hook_handler(value: &mut Value) -> bool {
    let should_remove = hook_command(value).is_some_and(is_owned_hook_command);
    if should_remove {
        *value = Value::Null;
    }
    should_remove
}

fn hook_command(value: &Value) -> Option<&str> {
    value.get("command").and_then(Value::as_str).map(str::trim)
}

fn is_owned_hook_command(command: &str) -> bool {
    command.contains(LOOPER_HOOK_MARKER)
}

fn event_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(values) => values.is_empty() || values.iter().all(event_is_empty),
        Value::Object(object) => object.is_empty() || object.values().all(event_is_empty),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_claude_hooks_in_settings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let change = register_owned_claude_hooks(temp_dir.path(), "agent-control-plane --hook")
            .expect("register");

        assert_eq!(change.installed_handlers, 3);
        let status = inspect_claude_hooks(temp_dir.path());
        assert_eq!(status.health, "healthy");
        assert_eq!(status.owner, ClaudeHookOwner::LooperRust);
        assert!(status.registered_events.contains(&"Stop".to_owned()));

        let settings =
            fs::read_to_string(default_claude_settings_path(temp_dir.path())).expect("settings");
        assert!(settings.contains("LOOPER_CLAUDE_HOOK=1"));
        assert!(settings.contains("SessionStart"));
        assert!(settings.contains("Stop"));
        assert!(settings.contains("UserPromptSubmit"));
    }

    #[test]
    fn inspect_prefers_owned_session_hooks_over_foreign_user_hooks() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings_path = default_claude_settings_path(temp_dir.path());
        fs::write(
            &settings_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "PreToolUse": [
                        {"hooks": [{"type": "command", "command": "rtk hook claude"}]}
                    ],
                    "SessionStart": [
                        {"hooks": [{"type": "command", "command": "LOOPER_CLAUDE_HOOK=1 agent-control-plane --hook --managed-by looper"}]}
                    ],
                    "Stop": [
                        {"hooks": [{"type": "command", "command": "LOOPER_CLAUDE_HOOK=1 agent-control-plane --hook --managed-by looper"}]}
                    ],
                    "UserPromptSubmit": [
                        {"hooks": [{"type": "command", "command": "LOOPER_CLAUDE_HOOK=1 agent-control-plane --hook --managed-by looper"}]}
                    ]
                }
            }))
            .expect("json"),
        )
        .expect("write");

        let status = inspect_claude_hooks(temp_dir.path());

        assert_eq!(status.health, "healthy");
        assert_eq!(status.owner, ClaudeHookOwner::LooperRust);
        assert_eq!(
            status.active_command.as_deref(),
            Some("LOOPER_CLAUDE_HOOK=1 agent-control-plane --hook --managed-by looper")
        );
        assert!(status.registered_events.contains(&"PreToolUse".to_owned()));
    }

    #[test]
    fn preserves_user_claude_hooks_on_unregister() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings_path = default_claude_settings_path(temp_dir.path());
        fs::write(
            &settings_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "Stop": [
                        {"hooks": [{"type": "command", "command": "/usr/local/bin/user-hook"}]},
                        {"hooks": [{"type": "command", "command": "agent-control-plane --hook --managed-by looper"}]}
                    ]
                },
                "model": "sonnet"
            }))
            .expect("json"),
        )
        .expect("write");

        let removed = unregister_owned_claude_hooks(temp_dir.path()).expect("unregister");

        assert_eq!(removed, 1);
        let settings = fs::read_to_string(settings_path).expect("settings");
        assert!(settings.contains("/usr/local/bin/user-hook"));
        assert!(settings.contains("\"model\""));
        assert!(!settings.contains("--managed-by looper"));
    }

    #[test]
    fn parses_claude_hook_payload_into_public_thread_id() {
        let payload = parse_claude_hook_payload(
            r#"{"hook_event_name":"stop","session_id":"claude-session-1","cwd":"/tmp/project"}"#,
        )
        .expect("parse");

        assert_eq!(payload.hook_event_name, "Stop");
        assert_eq!(
            payload.session_id.as_deref(),
            Some("claude:claude-session-1")
        );
        assert_eq!(payload.cwd.as_deref(), Some("/tmp/project"));
    }

    #[test]
    fn strips_claude_public_thread_prefix_for_resume() {
        assert_eq!(
            claude_session_id_from_public_thread_id("claude:session-1"),
            "session-1"
        );
    }

    #[test]
    fn parses_claude_transcript_path_when_session_id_is_absent() {
        let payload = parse_claude_hook_payload(
            r#"{"hook_event_name":"Stop","transcript_path":"/Users/test/.claude/projects/-tmp/uuid-1.jsonl"}"#,
        )
        .expect("parse");

        assert_eq!(payload.session_id.as_deref(), Some("claude:uuid-1"));
    }
}

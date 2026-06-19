use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::hook_registration::LOOPER_HOOK_MARKER;
use crate::mobile::session::MobileHookPayload;

const DEVIN_CONFIG_RELATIVE_PATH: &str = ".config/devin/config.json";
const DEVIN_LOCAL_PROVIDER_ID: &str = "devin-cli";
const DEVIN_ACP_SESSION_PREFIX: &str = "acp/";
const LOOPER_DEVIN_HOOK_ENV: &str = "LOOPER_DEVIN_HOOK";
const LOOPER_DEVIN_HOOK_VALUE: &str = "1";
const SESSION_HOOK_TIMEOUT_SECONDS: u64 = 30;
const STOP_HOOK_TIMEOUT_SECONDS: u64 = 86_400;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevinHookRegistrationChange {
    pub removed_handlers: usize,
    pub installed_handlers: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinHookStatus {
    pub registered_events: Vec<String>,
    pub active_command: Option<String>,
    pub owner: DevinHookOwner,
    pub health: String,
    pub config_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DevinHookOwner {
    LooperRust,
    Unknown,
    None,
}

pub fn default_devin_config_path(home_path: &Path) -> PathBuf {
    home_path.join(DEVIN_CONFIG_RELATIVE_PATH)
}

pub fn register_owned_devin_hooks(
    home_path: &Path,
    hook_command: &str,
) -> Result<DevinHookRegistrationChange> {
    let config_path = default_devin_config_path(home_path);
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let mut document = load_config_document(&config_path)?;
    let hooks = ensure_hooks_object(&mut document);
    let removed_handlers = remove_owned_hooks_from_events(hooks);
    upsert_owned_hook_groups(hooks, normalize_devin_hook_command(hook_command));

    let mut content = serde_json::to_string_pretty(&document)?;
    content.push('\n');
    fs::write(&config_path, content).with_context(|| format!("write {}", config_path.display()))?;

    Ok(DevinHookRegistrationChange {
        removed_handlers,
        installed_handlers: owned_event_names().len(),
    })
}

pub fn unregister_owned_devin_hooks(home_path: &Path) -> Result<usize> {
    let config_path = default_devin_config_path(home_path);
    if !config_path.exists() {
        return Ok(0);
    }

    let mut document: Value = serde_json::from_slice(
        &fs::read(&config_path).with_context(|| format!("read {}", config_path.display()))?,
    )
    .with_context(|| format!("parse {}", config_path.display()))?;
    let removed_handlers = document
        .get_mut("hooks")
        .map(remove_owned_hooks_from_events)
        .unwrap_or(0);

    if removed_handlers > 0 {
        let mut content = serde_json::to_string_pretty(&document)?;
        content.push('\n');
        fs::write(&config_path, content)
            .with_context(|| format!("write {}", config_path.display()))?;
    }

    Ok(removed_handlers)
}

pub fn inspect_devin_hooks(home_path: &Path) -> DevinHookStatus {
    let config_path = default_devin_config_path(home_path);
    let (registered_events, active_command) = match read_hooks_json(&config_path) {
        Ok(value) => value,
        Err(_) => (Vec::new(), None),
    };
    let owner = classify_hook_owner(active_command.as_deref());
    let health = if registered_events.is_empty() {
        "missing"
    } else if matches!(owner, DevinHookOwner::LooperRust) {
        "healthy"
    } else {
        "configured"
    }
    .to_owned();

    DevinHookStatus {
        registered_events,
        active_command,
        owner,
        health,
        config_path: config_path
            .exists()
            .then(|| config_path.display().to_string()),
    }
}

pub fn is_devin_hook_invocation() -> bool {
    std::env::var(LOOPER_DEVIN_HOOK_ENV).as_deref() == Ok(LOOPER_DEVIN_HOOK_VALUE)
}

pub fn parse_devin_hook_payload(input: &str) -> Result<MobileHookPayload> {
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
        serde_json::from_str(trimmed).context("parse Devin hook payload JSON from stdin")?;
    Ok(devin_payload_from_value(&value))
}

fn devin_payload_from_value(value: &Value) -> MobileHookPayload {
    let raw_session_id = first_string(value, &["session_id", "sessionId"]);
    MobileHookPayload {
        hook_event_name: first_string(value, &["hook_event_name", "hookEventName"])
            .map(|name| normalize_devin_event_name(&name))
            .unwrap_or_default(),
        session_id: raw_session_id
            .map(|session_id| public_thread_id_for_devin_session(&session_id)),
        turn_id: first_string(value, &["turn_id", "turnId"]),
        cwd: first_string(value, &["cwd", "workspaceRoot", "workspace_root"]),
        last_assistant_message: first_string(
            value,
            &["last_assistant_message", "lastAssistantMessage"],
        ),
    }
}

fn public_thread_id_for_devin_session(session_id: &str) -> String {
    let session_id = session_id.trim();
    if session_id.starts_with("devin:") {
        return session_id.to_owned();
    }
    if let Some(local_session_id) = session_id.strip_prefix(DEVIN_ACP_SESSION_PREFIX)
        && let Some((provider_id, provider_session_id)) = local_session_id.split_once('/')
    {
        return format!(
            "devin:{provider_id}:{}",
            provider_session_id.replace('/', ":")
        );
    }
    format!(
        "devin:{DEVIN_LOCAL_PROVIDER_ID}:{}",
        session_id.replace('/', ":")
    )
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

fn normalize_devin_event_name(name: &str) -> String {
    match name {
        "stop" | "Stop" => "Stop".to_owned(),
        "session_start" | "SessionStart" => "SessionStart".to_owned(),
        "user_prompt_submit" | "UserPromptSubmit" => "UserPromptSubmit".to_owned(),
        "session_end" | "SessionEnd" => "SessionEnd".to_owned(),
        other => other.to_owned(),
    }
}

fn load_config_document(config_path: &Path) -> Result<Value> {
    if !config_path.exists() {
        return Ok(json!({}));
    }

    serde_json::from_slice(
        &fs::read(config_path).with_context(|| format!("read {}", config_path.display()))?,
    )
    .with_context(|| format!("parse {}", config_path.display()))
}

fn ensure_hooks_object(document: &mut Value) -> &mut Value {
    if !document.is_object() {
        *document = json!({});
    }
    if document.get("hooks").is_none() || !document["hooks"].is_object() {
        document["hooks"] = json!({});
    }
    document.get_mut("hooks").expect("hooks object")
}

fn read_hooks_json(path: &Path) -> Result<(Vec<String>, Option<String>)> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let Some(object) = value.get("hooks").and_then(Value::as_object) else {
        return Ok((Vec::new(), None));
    };
    let registered_events = object.keys().cloned().collect::<Vec<_>>();
    let active_command = object
        .values()
        .find_map(first_hook_command)
        .map(str::to_owned);
    Ok((registered_events, active_command))
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

fn classify_hook_owner(command: Option<&str>) -> DevinHookOwner {
    let Some(command) = command else {
        return DevinHookOwner::None;
    };
    if is_owned_hook_command(command) {
        DevinHookOwner::LooperRust
    } else {
        DevinHookOwner::Unknown
    }
}

fn normalize_devin_hook_command(hook_command: &str) -> String {
    let trimmed = hook_command.trim();
    let command = if is_owned_hook_command(trimmed) {
        trimmed.to_owned()
    } else {
        format!("{trimmed} {LOOPER_HOOK_MARKER}")
    };
    if command.contains(LOOPER_DEVIN_HOOK_ENV) {
        command
    } else {
        format!("{LOOPER_DEVIN_HOOK_ENV}={LOOPER_DEVIN_HOOK_VALUE} {command}")
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
    fn registers_devin_hooks_in_global_config() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let change = register_owned_devin_hooks(temp_dir.path(), "agent-control-plane --hook")
            .expect("register");

        assert_eq!(change.installed_handlers, 3);
        let config =
            fs::read_to_string(default_devin_config_path(temp_dir.path())).expect("read config");
        assert!(config.contains("LOOPER_DEVIN_HOOK=1"));
        assert!(config.contains("SessionStart"));
        assert!(config.contains("Stop"));
        assert!(config.contains("UserPromptSubmit"));
    }

    #[test]
    fn preserves_user_devin_hooks_on_unregister() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let config_path = default_devin_config_path(temp_dir.path());
        fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir");
        fs::write(
            &config_path,
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

        let removed = unregister_owned_devin_hooks(temp_dir.path()).expect("unregister");

        assert_eq!(removed, 1);
        let config = fs::read_to_string(config_path).expect("read config");
        assert!(config.contains("/usr/local/bin/user-hook"));
        assert!(config.contains("\"model\""));
        assert!(!config.contains("--managed-by looper"));
    }

    #[test]
    fn parses_devin_hook_payload_into_public_thread_id() {
        let payload = parse_devin_hook_payload(
            r#"{"hook_event_name":"stop","session_id":"shadow-canidae","cwd":"/tmp/project"}"#,
        )
        .expect("parse");

        assert_eq!(payload.hook_event_name, "Stop");
        assert_eq!(
            payload.session_id.as_deref(),
            Some("devin:devin-cli:shadow-canidae")
        );
        assert_eq!(payload.cwd.as_deref(), Some("/tmp/project"));
    }

    #[test]
    fn parses_devin_acp_provider_payload_into_public_thread_id() {
        let payload = parse_devin_hook_payload(
            r#"{"hook_event_name":"stop","session_id":"acp/claude-acp/bd6aa5c3-b6d1-4331-97e0-045c44652e2d"}"#,
        )
        .expect("parse");

        assert_eq!(
            payload.session_id.as_deref(),
            Some("devin:claude-acp:bd6aa5c3-b6d1-4331-97e0-045c44652e2d")
        );
    }
}

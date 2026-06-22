// allow: SIZE_OK — single-file Grok hook config adapter keeps register/inspect/unregister schema edits atomic.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::hook_registration::LOOPER_HOOK_MARKER;

const GROK_HOOKS_DIR: &str = "hooks";
const LOOPER_GROK_HOOK_FILE: &str = "looper.json";
const SESSION_HOOK_TIMEOUT_SECONDS: u64 = 30;
const STOP_HOOK_TIMEOUT_SECONDS: u64 = 120;
const SESSION_STATUS_MESSAGE: &str = "looper is registering the Grok Build session";
const STOP_STATUS_MESSAGE: &str = "looper is deciding whether Grok Build should continue";
const PROMPT_STATUS_MESSAGE: &str = "looper is capturing the Grok Build prompt";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrokHookRegistrationChange {
    pub removed_handlers: usize,
    pub installed_handlers: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GrokHookStatus {
    pub registered_events: Vec<String>,
    pub active_command: Option<String>,
    pub owner: GrokHookOwner,
    pub health: String,
    pub hooks_path: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GrokHookOwner {
    LooperRust,
    Unknown,
    None,
}

pub fn default_grok_home(home_path: &Path) -> PathBuf {
    home_path.join(".grok")
}

pub fn register_owned_grok_hooks(
    grok_home: &Path,
    hook_command: &str,
) -> Result<GrokHookRegistrationChange> {
    let hooks_dir = grok_home.join(GROK_HOOKS_DIR);
    fs::create_dir_all(&hooks_dir).with_context(|| format!("create {}", hooks_dir.display()))?;
    let hooks_path = hooks_dir.join(LOOPER_GROK_HOOK_FILE);
    let mut document = load_hooks_document(&hooks_path)?;
    let hooks = ensure_nested_hooks_object(&mut document);
    let removed_handlers = remove_owned_hooks_from_events(hooks);
    upsert_owned_hook_groups(hooks, normalize_hook_command(hook_command));

    let mut content = serde_json::to_string_pretty(&document)?;
    content.push('\n');
    fs::write(&hooks_path, content).with_context(|| format!("write {}", hooks_path.display()))?;

    Ok(GrokHookRegistrationChange {
        removed_handlers,
        installed_handlers: owned_event_names().len(),
    })
}

pub fn unregister_owned_grok_hooks(grok_home: &Path) -> Result<usize> {
    let hooks_path = grok_home.join(GROK_HOOKS_DIR).join(LOOPER_GROK_HOOK_FILE);
    if !hooks_path.exists() {
        return Ok(0);
    }

    let mut document: Value = serde_json::from_slice(
        &fs::read(&hooks_path).with_context(|| format!("read {}", hooks_path.display()))?,
    )
    .with_context(|| format!("parse {}", hooks_path.display()))?;
    let removed_handlers = if let Some(hooks) = document.get_mut("hooks") {
        remove_owned_hooks_from_events(hooks)
    } else {
        remove_owned_hooks_from_events(&mut document)
    };

    if removed_handlers > 0 {
        let mut content = serde_json::to_string_pretty(&document)?;
        content.push('\n');
        fs::write(&hooks_path, content)
            .with_context(|| format!("write {}", hooks_path.display()))?;
    }

    Ok(removed_handlers)
}

pub fn inspect_grok_hooks(grok_home: &Path) -> GrokHookStatus {
    let hooks_path = grok_home.join(GROK_HOOKS_DIR).join(LOOPER_GROK_HOOK_FILE);
    let (registered_events, active_command) = read_hooks_json(&hooks_path).unwrap_or_default();
    let owner = classify_hook_owner(active_command.as_deref());
    let health = if registered_events.is_empty() {
        "missing"
    } else if matches!(owner, GrokHookOwner::LooperRust) {
        "healthy"
    } else {
        "configured"
    }
    .to_owned();

    GrokHookStatus {
        registered_events,
        active_command,
        owner,
        health,
        hooks_path: hooks_path
            .exists()
            .then(|| hooks_path.display().to_string()),
    }
}

fn read_hooks_json(path: &Path) -> Result<(Vec<String>, Option<String>)> {
    let value: Value = serde_json::from_slice(&fs::read(path)?)?;
    let hook_value = value.get("hooks").unwrap_or(&value);
    let Some(object) = hook_value.as_object() else {
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

fn classify_hook_owner(command: Option<&str>) -> GrokHookOwner {
    let Some(command) = command else {
        return GrokHookOwner::None;
    };
    if is_owned_hook_command(command) {
        GrokHookOwner::LooperRust
    } else {
        GrokHookOwner::Unknown
    }
}

fn normalize_hook_command(hook_command: &str) -> String {
    let trimmed = hook_command.trim();
    if is_owned_hook_command(trimmed) {
        trimmed.to_owned()
    } else {
        format!("{trimmed} {LOOPER_HOOK_MARKER}")
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
                    owned_hook_handler(&hook_command, hook_timeout_for_event(event_name), status_message_for_event(event_name))
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

fn status_message_for_event(event_name: &str) -> &'static str {
    match event_name {
        "Stop" => STOP_STATUS_MESSAGE,
        "UserPromptSubmit" => PROMPT_STATUS_MESSAGE,
        _ => SESSION_STATUS_MESSAGE,
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

fn owned_hook_handler(command: &str, timeout: u64, status_message: &str) -> Value {
    json!({
        "type": "command",
        "command": command,
        "timeout": timeout,
        "statusMessage": status_message,
    })
}

fn load_hooks_document(hooks_path: &Path) -> Result<Value> {
    if !hooks_path.exists() {
        return Ok(json!({ "hooks": {} }));
    }

    serde_json::from_slice(
        &fs::read(hooks_path).with_context(|| format!("read {}", hooks_path.display()))?,
    )
    .with_context(|| format!("parse {}", hooks_path.display()))
}

fn ensure_nested_hooks_object(document: &mut Value) -> &mut Value {
    if !document.is_object() {
        *document = json!({ "hooks": {} });
    }
    let has_hooks = document
        .as_object()
        .map(|document| document.contains_key("hooks"))
        .unwrap_or(false);
    if !has_hooks {
        let current_hooks = std::mem::replace(document, json!({ "hooks": {} }));
        document["hooks"] = current_hooks;
    }
    match document {
        Value::Object(document_object) => {
            let hooks = document_object.entry("hooks").or_insert_with(|| json!({}));
            if !hooks.is_object() {
                *hooks = json!({});
            }
            hooks
        }
        _ => document,
    }
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

fn remove_owned_hooks_from_event(event: &mut Value) -> usize {
    if let Some(command) = hook_command(event) {
        if is_owned_hook_command(command) {
            *event = Value::Null;
            return 1;
        }
        return 0;
    }

    let Some(groups) = event.as_array_mut() else {
        return 0;
    };

    let mut removed_handlers = 0;
    let mut retained_groups = Vec::with_capacity(groups.len());
    for mut group in std::mem::take(groups) {
        removed_handlers += remove_owned_hooks_from_group(&mut group);
        if !event_is_empty(&group) {
            retained_groups.push(group);
        }
    }
    *groups = retained_groups;
    removed_handlers
}

fn remove_owned_hooks_from_group(group: &mut Value) -> usize {
    if let Some(command) = hook_command(group) {
        if is_owned_hook_command(command) {
            *group = Value::Null;
            return 1;
        }
        return 0;
    }

    let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
        return 0;
    };
    let original_count = hooks.len();
    hooks.retain(|hook| {
        hook_command(hook)
            .map(|command| !is_owned_hook_command(command))
            .unwrap_or(true)
    });
    original_count - hooks.len()
}

fn event_is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(entries) => entries.is_empty(),
        Value::Object(object) => object
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty),
        _ => false,
    }
}

fn hook_command(value: &Value) -> Option<&str> {
    if let Some(command) = value.as_str() {
        return Some(command);
    }
    value.get("command").and_then(Value::as_str)
}

fn is_owned_hook_command(command: &str) -> bool {
    command.to_ascii_lowercase().contains(LOOPER_HOOK_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_and_inspects_looper_grok_hooks() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let grok_home = temp_dir.path();
        let hook_command = "/tmp/looper-server --hook --managed-by looper";

        let change =
            register_owned_grok_hooks(grok_home, hook_command).expect("register grok hooks");
        assert_eq!(change.installed_handlers, 3);

        let status = inspect_grok_hooks(grok_home);
        assert_eq!(status.health, "healthy");
        assert_eq!(status.owner, GrokHookOwner::LooperRust);
        assert!(status.registered_events.contains(&"Stop".to_owned()));

        let removed = unregister_owned_grok_hooks(grok_home).expect("unregister grok hooks");
        assert_eq!(removed, 3);
        assert_eq!(inspect_grok_hooks(grok_home).health, "missing");
    }
}

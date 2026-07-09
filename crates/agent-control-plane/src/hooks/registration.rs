use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};
use toml_edit::{DocumentMut, Item, Table, value};

use super::adapter::HookRegistrationSpec;

pub const LOOPER_HOOK_MARKER: &str = "--managed-by looper";

const LEGACY_BUN_HOOK_COMMAND_MARKERS: &[&str] = &["looper-hook", "managed-hook-script"];
const SESSION_START_MATCHER: &str = "startup|resume";
const CODEX_SESSION_STATUS_MESSAGE: &str = "looper is registering the Codex chat";
const CODEX_STOP_STATUS_MESSAGE: &str = "looper is deciding whether Codex should continue";
const CODEX_PROMPT_STATUS_MESSAGE: &str = "looper is capturing the chat prompt";
const GROK_SESSION_STATUS_MESSAGE: &str = "looper is registering the Grok Build session";
const GROK_STOP_STATUS_MESSAGE: &str = "looper is deciding whether Grok Build should continue";
const GROK_PROMPT_STATUS_MESSAGE: &str = "looper is capturing the Grok Build prompt";
const HOOKS_FEATURE_KEY: &str = "hooks";
const LEGACY_CODEX_HOOKS_FEATURE_KEY: &str = "codex_hooks";
const ENABLED_KEY: &str = "enabled";
const OWNED_HOOK_GROUP_INDEX: usize = 0;
const OWNED_HOOK_HANDLER_INDEX: usize = 0;
const OWNED_HOOK_STATE_EVENT_KEYS: &[&str] = &["session_start", "stop", "user_prompt_submit"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookConfigStyle {
    ClaudeSettings,
    CodexHooksJson,
    DevinConfig,
    GrokDir,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookRegistrationChange {
    pub removed_handlers: usize,
    pub installed_handlers: usize,
}

pub fn register_owned_hooks_for_spec(
    spec: &HookRegistrationSpec,
    hook_command: &str,
) -> Result<HookRegistrationChange> {
    create_config_parent(&spec.config_path)?;
    if spec.config_style == HookConfigStyle::CodexHooksJson {
        ensure_hooks_feature(&spec.config_path)?;
    }

    let mut document = load_hooks_document(&spec.config_path, spec.config_style)?;
    let hooks = ensure_hooks_object(&mut document, spec.config_style);
    let removed_handlers = remove_owned_hooks_from_events(hooks);
    upsert_owned_hook_groups(
        hooks,
        spec,
        normalize_hook_command(hook_command, spec.env_marker),
    );

    let mut content = serde_json::to_string_pretty(&document)?;
    content.push('\n');
    fs::write(&spec.config_path, content)
        .with_context(|| format!("write {}", spec.config_path.display()))?;

    Ok(HookRegistrationChange {
        removed_handlers,
        installed_handlers: owned_event_names().len(),
    })
}

pub fn unregister_owned_hooks_for_spec(spec: &HookRegistrationSpec) -> Result<usize> {
    if !spec.config_path.exists() {
        return Ok(0);
    }

    let mut document: Value = serde_json::from_slice(
        &fs::read(&spec.config_path)
            .with_context(|| format!("read {}", spec.config_path.display()))?,
    )
    .with_context(|| format!("parse {}", spec.config_path.display()))?;
    let removed_handlers = remove_owned_hooks_for_style(&mut document, spec.config_style);

    if removed_handlers > 0 {
        let mut content = serde_json::to_string_pretty(&document)?;
        content.push('\n');
        fs::write(&spec.config_path, content)
            .with_context(|| format!("write {}", spec.config_path.display()))?;
    }

    Ok(removed_handlers)
}

pub fn owned_hook_state_keys_for_hooks_path(hooks_path: &Path) -> Vec<String> {
    OWNED_HOOK_STATE_EVENT_KEYS
        .iter()
        .map(|event_name| {
            format!(
                "{}:{event_name}:{OWNED_HOOK_GROUP_INDEX}:{OWNED_HOOK_HANDLER_INDEX}",
                hooks_path.display()
            )
        })
        .collect()
}

pub fn normalize_hook_command(hook_command: &str, env_marker: Option<&'static str>) -> String {
    let trimmed = hook_command.trim();
    let mut command = if is_owned_hook_command(trimmed) {
        trimmed.to_owned()
    } else {
        format!("{trimmed} {LOOPER_HOOK_MARKER}")
    };

    if let Some(marker) = env_marker {
        let marker_name = marker
            .split_once('=')
            .map(|(name, _)| name)
            .unwrap_or(marker);
        if !command.contains(marker_name) {
            command = format!("{marker} {command}");
        }
    }

    command
}

pub fn is_owned_hook_command(command: &str) -> bool {
    let normalized = command.to_ascii_lowercase();
    std::iter::once(LOOPER_HOOK_MARKER)
        .chain(LEGACY_BUN_HOOK_COMMAND_MARKERS.iter().copied())
        .any(|marker| normalized.contains(marker))
}

fn create_config_parent(config_path: &Path) -> Result<()> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    Ok(())
}

fn load_hooks_document(config_path: &Path, style: HookConfigStyle) -> Result<Value> {
    if !config_path.exists() {
        return Ok(default_document(style));
    }

    serde_json::from_slice(
        &fs::read(config_path).with_context(|| format!("read {}", config_path.display()))?,
    )
    .with_context(|| format!("parse {}", config_path.display()))
}

fn default_document(style: HookConfigStyle) -> Value {
    if style.uses_nested_hooks_document() {
        json!({ "hooks": {} })
    } else {
        json!({})
    }
}

pub fn ensure_hooks_object(document: &mut Value, style: HookConfigStyle) -> &mut Value {
    if style.uses_nested_hooks_document() {
        ensure_nested_hooks_object(document)
    } else {
        ensure_root_hooks_object(document)
    }
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
    if !document["hooks"].is_object() {
        document["hooks"] = json!({});
    }
    document.get_mut("hooks").expect("hooks object")
}

fn ensure_root_hooks_object(document: &mut Value) -> &mut Value {
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

fn remove_owned_hooks_for_style(document: &mut Value, style: HookConfigStyle) -> usize {
    if style.uses_nested_hooks_document() {
        if let Some(hooks) = document.get_mut("hooks") {
            remove_owned_hooks_from_events(hooks)
        } else {
            remove_owned_hooks_from_events(document)
        }
    } else {
        document
            .get_mut("hooks")
            .map(remove_owned_hooks_from_events)
            .unwrap_or(0)
    }
}

fn owned_event_names() -> [&'static str; 3] {
    ["SessionStart", "Stop", "UserPromptSubmit"]
}

fn upsert_owned_hook_groups(hooks: &mut Value, spec: &HookRegistrationSpec, hook_command: String) {
    let Some(hooks) = hooks.as_object_mut() else {
        return;
    };
    for event_name in owned_event_names() {
        prepend_owned_hook_group(
            hooks,
            event_name,
            owned_hook_group(event_name, spec, &hook_command),
        );
    }
}

pub fn prepend_owned_hook_group(
    hooks: &mut Map<String, Value>,
    event_name: &str,
    owned_group: Value,
) {
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

fn owned_hook_group(event_name: &str, spec: &HookRegistrationSpec, hook_command: &str) -> Value {
    let handler = owned_hook_handler(
        hook_command,
        hook_timeout_for_event(event_name, spec),
        spec.config_style.status_message_for_event(event_name),
    );
    if spec.config_style == HookConfigStyle::CodexHooksJson && event_name == "SessionStart" {
        json!({
            "matcher": SESSION_START_MATCHER,
            "hooks": [handler],
        })
    } else {
        json!({
            "hooks": [handler],
        })
    }
}

fn hook_timeout_for_event(event_name: &str, spec: &HookRegistrationSpec) -> u64 {
    match event_name {
        "Stop" => spec.stop_timeout_secs,
        "UserPromptSubmit" => spec.prompt_timeout_secs,
        _ => spec.session_timeout_secs,
    }
}

fn owned_hook_handler(command: &str, timeout: u64, status_message: Option<&'static str>) -> Value {
    let mut handler = Map::new();
    handler.insert("type".to_owned(), json!("command"));
    handler.insert("command".to_owned(), json!(command));
    handler.insert("timeout".to_owned(), json!(timeout));
    if let Some(status_message) = status_message {
        handler.insert("statusMessage".to_owned(), json!(status_message));
    }
    Value::Object(handler)
}

fn ensure_hooks_feature(hooks_path: &Path) -> Result<()> {
    let Some(codex_home) = hooks_path.parent() else {
        return Ok(());
    };
    let config_path = codex_home.join("config.toml");
    let current = fs::read_to_string(&config_path).unwrap_or_default();
    let next = repair_hooks_config(&current, hooks_path)?;
    if next != current {
        fs::write(&config_path, next)
            .with_context(|| format!("write {}", config_path.display()))?;
    }
    Ok(())
}

fn repair_hooks_config(config_text: &str, hooks_path: &Path) -> Result<String> {
    let mut document = if config_text.trim().is_empty() {
        DocumentMut::new()
    } else {
        config_text
            .parse::<DocumentMut>()
            .context("parse Codex config.toml")?
    };
    set_hooks_enabled(&mut document);
    set_owned_hook_state_enabled(
        &mut document,
        &owned_hook_state_keys_for_hooks_path(hooks_path),
    );
    let mut next = document.to_string();
    if !next.ends_with('\n') {
        next.push('\n');
    }
    Ok(next)
}

fn set_hooks_enabled(document: &mut DocumentMut) {
    let features = ensure_table(document.as_table_mut(), "features");
    features.remove(LEGACY_CODEX_HOOKS_FEATURE_KEY);
    features[HOOKS_FEATURE_KEY] = value(true);
}

fn set_owned_hook_state_enabled(document: &mut DocumentMut, owned_state_keys: &[String]) {
    let Some(hooks) = document
        .as_table_mut()
        .get_mut("hooks")
        .and_then(Item::as_table_mut)
    else {
        return;
    };
    let Some(state) = hooks.get_mut("state").and_then(Item::as_table_mut) else {
        return;
    };
    for state_key in owned_state_keys {
        if let Some(item) = state.get_mut(state_key) {
            if !item.is_table() {
                *item = Item::Table(Table::new());
            }
            if let Some(table) = item.as_table_mut() {
                table[ENABLED_KEY] = value(true);
                table.sort_values_by(|left_key, _, right_key, _| {
                    match (
                        left_key.get() == ENABLED_KEY,
                        right_key.get() == ENABLED_KEY,
                    ) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        _ => std::cmp::Ordering::Equal,
                    }
                });
            }
        }
    }
}

fn ensure_table<'a>(parent: &'a mut Table, key: &str) -> &'a mut Table {
    if !parent.contains_key(key) || !parent[key].is_table() {
        parent[key] = Item::Table(Table::new());
    }
    parent[key].as_table_mut().expect("table")
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
        return Some(command.trim());
    }
    value.get("command").and_then(Value::as_str).map(str::trim)
}

impl HookConfigStyle {
    fn uses_nested_hooks_document(self) -> bool {
        matches!(self, Self::CodexHooksJson | Self::GrokDir)
    }

    fn status_message_for_event(self, event_name: &str) -> Option<&'static str> {
        match self {
            Self::CodexHooksJson => match event_name {
                "Stop" => Some(CODEX_STOP_STATUS_MESSAGE),
                "UserPromptSubmit" => Some(CODEX_PROMPT_STATUS_MESSAGE),
                _ => Some(CODEX_SESSION_STATUS_MESSAGE),
            },
            Self::GrokDir => match event_name {
                "Stop" => Some(GROK_STOP_STATUS_MESSAGE),
                "UserPromptSubmit" => Some(GROK_PROMPT_STATUS_MESSAGE),
                _ => Some(GROK_SESSION_STATUS_MESSAGE),
            },
            Self::ClaudeSettings | Self::DevinConfig => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;
    use crate::hooks::adapter::{
        ClaudeHookAdapter, CodexHookAdapter, DevinHookAdapter, GrokHookAdapter, Homes, HookAdapter,
    };

    #[test]
    fn hook_feature_writer_uses_stable_key_for_empty_config() {
        let next = repair_hooks_config("", Path::new("/Users/ay/.codex/hooks.json"))
            .expect("repair config");
        assert_eq!(next, "[features]\nhooks = true\n");
    }

    #[test]
    fn hook_feature_writer_replaces_disabled_stable_key() {
        let next = repair_hooks_config(
            "[features]\nhooks = false\n",
            Path::new("/Users/ay/.codex/hooks.json"),
        )
        .expect("repair config");

        assert_eq!(next, "[features]\nhooks = true\n");
    }

    #[test]
    fn hook_feature_writer_removes_legacy_key() {
        let next = repair_hooks_config(
            "[model]\ndefault = \"gpt-5.5\"\n\n[features]\ncodex_hooks = false\n",
            Path::new("/Users/ay/.codex/hooks.json"),
        )
        .expect("repair config");

        assert!(next.contains("[model]"));
        assert!(next.contains("[features]"));
        assert!(next.contains("hooks = true"));
        assert!(!next.contains("codex_hooks"));
    }

    #[test]
    fn hook_config_repair_reenables_owned_prompt_state_only() {
        let next = repair_hooks_config(
            r#"[features]
hooks = true

[hooks.state."/Users/ay/.codex/hooks.json:user_prompt_submit:0:0"]
enabled = false
trusted_hash = "sha256:owned"

[hooks.state."/Users/ay/.codex/hooks.json:user_prompt_submit:1:0"]
enabled = false
trusted_hash = "sha256:user"

[hooks.state."/tmp/other/hooks.json:user_prompt_submit:0:0"]
enabled = false
"#,
            Path::new("/Users/ay/.codex/hooks.json"),
        )
        .expect("repair config");

        assert!(next.contains(
            r#"[hooks.state."/Users/ay/.codex/hooks.json:user_prompt_submit:0:0"]
enabled = true
trusted_hash = "sha256:owned""#
        ));
        assert!(next.contains(
            r#"[hooks.state."/Users/ay/.codex/hooks.json:user_prompt_submit:1:0"]
enabled = false
trusted_hash = "sha256:user""#
        ));
        assert!(next.contains(
            r#"[hooks.state."/tmp/other/hooks.json:user_prompt_submit:0:0"]
enabled = false"#
        ));
    }

    #[test]
    fn hook_config_repair_adds_enabled_to_owned_state() {
        let next = repair_hooks_config(
            r#"[features]
hooks = false

[hooks.state."/Users/ay/.codex/hooks.json:stop:0:0"]
trusted_hash = "sha256:owned"
"#,
            Path::new("/Users/ay/.codex/hooks.json"),
        )
        .expect("repair config");

        assert!(next.contains("hooks = true"));
        assert!(next.contains(
            r#"[hooks.state."/Users/ay/.codex/hooks.json:stop:0:0"]
enabled = true
trusted_hash = "sha256:owned""#
        ));
    }

    #[test]
    fn codex_registration_is_idempotent_and_replaces_legacy_markers() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let spec = CodexHookAdapter
            .spec(&Homes::for_codex_home(temp_dir.path()))
            .clone();
        fs::write(
            &spec.config_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "Stop": [
                        {"hooks": [
                            {"type": "command", "command": "bun legacy/bun/managed-hook-script.ts"},
                            {"type": "command", "command": "looper-hook stop"}
                        ]}
                    ]
                }
            }))
            .expect("json"),
        )
        .expect("write hooks");

        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let first = fs::read_to_string(&spec.config_path).expect("read first");
        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let second = fs::read_to_string(&spec.config_path).expect("read second");

        assert_eq!(first, second);
        assert_eq!(first.matches("--managed-by looper").count(), 3);
        assert!(!first.contains("managed-hook-script"));
        assert!(!first.contains("looper-hook"));
    }

    #[test]
    fn claude_registration_is_idempotent_and_replaces_legacy_markers() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let spec = ClaudeHookAdapter
            .spec(&Homes::for_claude_home(temp_dir.path()))
            .clone();
        fs::write(
            &spec.config_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "Stop": [
                        {"hooks": [
                            {"type": "command", "command": "bun legacy/bun/managed-hook-script.ts"},
                            {"type": "command", "command": "looper-hook stop"}
                        ]}
                    ]
                }
            }))
            .expect("json"),
        )
        .expect("write hooks");

        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let first = fs::read_to_string(&spec.config_path).expect("read first");
        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let second = fs::read_to_string(&spec.config_path).expect("read second");

        assert_eq!(first, second);
        assert_eq!(first.matches("--managed-by looper").count(), 3);
        assert!(first.contains("LOOPER_CLAUDE_HOOK=1"));
        assert!(!first.contains("managed-hook-script"));
        assert!(!first.contains("looper-hook"));
    }

    #[test]
    fn devin_registration_is_idempotent_and_replaces_legacy_markers() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let spec = DevinHookAdapter
            .spec(&Homes::for_home_path(temp_dir.path()))
            .clone();
        fs::create_dir_all(spec.config_path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &spec.config_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "Stop": [
                        {"hooks": [
                            {"type": "command", "command": "bun legacy/bun/managed-hook-script.ts"},
                            {"type": "command", "command": "looper-hook stop"}
                        ]}
                    ]
                }
            }))
            .expect("json"),
        )
        .expect("write hooks");

        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let first = fs::read_to_string(&spec.config_path).expect("read first");
        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let second = fs::read_to_string(&spec.config_path).expect("read second");

        assert_eq!(first, second);
        assert_eq!(first.matches("--managed-by looper").count(), 3);
        assert!(first.contains("LOOPER_DEVIN_HOOK=1"));
        assert!(!first.contains("managed-hook-script"));
        assert!(!first.contains("looper-hook"));
    }

    #[test]
    fn grok_registration_is_idempotent_and_replaces_legacy_markers() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let spec = GrokHookAdapter
            .spec(&Homes::for_grok_home(temp_dir.path()))
            .clone();
        fs::create_dir_all(spec.config_path.parent().expect("parent")).expect("mkdir");
        fs::write(
            &spec.config_path,
            serde_json::to_string_pretty(&json!({
                "hooks": {
                    "Stop": [
                        {"hooks": [
                            {"type": "command", "command": "bun legacy/bun/managed-hook-script.ts"},
                            {"type": "command", "command": "looper-hook stop"}
                        ]}
                    ]
                }
            }))
            .expect("json"),
        )
        .expect("write hooks");

        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let first = fs::read_to_string(&spec.config_path).expect("read first");
        register_owned_hooks_for_spec(&spec, "agent-control-plane --hook").expect("register");
        let second = fs::read_to_string(&spec.config_path).expect("read second");

        assert_eq!(first, second);
        assert_eq!(first.matches("--managed-by looper").count(), 3);
        assert!(!first.contains("managed-hook-script"));
        assert!(!first.contains("looper-hook"));
    }
}

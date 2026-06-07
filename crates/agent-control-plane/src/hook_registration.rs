use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

pub const LOOPER_HOOK_MARKER: &str = "--managed-by looper";

const CURRENT_HOOK_COMMAND_MARKERS: &[&str] = &[LOOPER_HOOK_MARKER];
const LEGACY_BUN_HOOK_COMMAND_MARKERS: &[&str] = &["looper-hook", "managed-hook-script"];
const SESSION_START_MATCHER: &str = "startup|resume";
const SESSION_HOOK_TIMEOUT_SECONDS: u64 = 30;
const STOP_HOOK_TIMEOUT_SECONDS: u64 = 86_400;
const SESSION_STATUS_MESSAGE: &str = "looper is registering the Codex chat";
const STOP_STATUS_MESSAGE: &str = "looper is deciding whether Codex should continue";
const PROMPT_STATUS_MESSAGE: &str = "looper is capturing the chat prompt";
const FEATURES_TABLE_HEADER: &str = "[features]";
const HOOKS_FEATURE_KEY: &str = "hooks";
const LEGACY_CODEX_HOOKS_FEATURE_KEY: &str = "codex_hooks";
const HOOKS_FEATURE_LINE: &str = "hooks = true";
const HOOK_STATE_TABLE_PREFIX: &str = "[hooks.state.\"";
const TOML_TABLE_SUFFIX: &str = "\"]";
const ENABLED_KEY: &str = "enabled";
const HOOK_ENABLED_LINE: &str = "enabled = true";
const OWNED_HOOK_GROUP_INDEX: usize = 0;
const OWNED_HOOK_HANDLER_INDEX: usize = 0;
const OWNED_HOOK_STATE_EVENT_KEYS: &[&str] = &["session_start", "stop", "user_prompt_submit"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookRegistrationChange {
    pub removed_handlers: usize,
    pub installed_handlers: usize,
}

pub fn register_owned_hooks(
    codex_home: &Path,
    hook_command: &str,
) -> Result<HookRegistrationChange> {
    fs::create_dir_all(codex_home).with_context(|| format!("create {}", codex_home.display()))?;
    ensure_hooks_feature(codex_home)?;

    let hooks_path = codex_home.join("hooks.json");
    let mut document = load_hooks_document(&hooks_path)?;
    let hooks = ensure_nested_hooks_object(&mut document);
    let removed_handlers = remove_owned_hooks_from_events(hooks);
    upsert_owned_hook_groups(hooks, normalize_hook_command(hook_command));

    let mut content = serde_json::to_string_pretty(&document)?;
    content.push('\n');
    fs::write(&hooks_path, content).with_context(|| format!("write {}", hooks_path.display()))?;

    Ok(HookRegistrationChange {
        removed_handlers,
        installed_handlers: owned_event_names().len(),
    })
}

pub fn unregister_owned_hooks(codex_home: &Path) -> Result<usize> {
    let hooks_path = codex_home.join("hooks.json");
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
    if document.get("hooks").is_none() {
        let current_hooks = std::mem::replace(document, json!({ "hooks": {} }));
        document["hooks"] = current_hooks;
    }
    if !document["hooks"].is_object() {
        document["hooks"] = json!({});
    }
    document.get_mut("hooks").expect("hooks object")
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
    prepend_owned_hook_group(
        hooks,
        "SessionStart",
        json!(
            {
                "matcher": SESSION_START_MATCHER,
                "hooks": [
                    owned_hook_handler(&hook_command, SESSION_HOOK_TIMEOUT_SECONDS, SESSION_STATUS_MESSAGE)
                ]
            }
        ),
    );
    prepend_owned_hook_group(
        hooks,
        "Stop",
        json!(
            {
                "hooks": [
                    owned_hook_handler(&hook_command, STOP_HOOK_TIMEOUT_SECONDS, STOP_STATUS_MESSAGE)
                ]
            }
        ),
    );
    prepend_owned_hook_group(
        hooks,
        "UserPromptSubmit",
        json!(
            {
                "hooks": [
                    owned_hook_handler(&hook_command, SESSION_HOOK_TIMEOUT_SECONDS, PROMPT_STATUS_MESSAGE)
                ]
            }
        ),
    );
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

fn ensure_hooks_feature(codex_home: &Path) -> Result<()> {
    let config_path = codex_home.join("config.toml");
    let hooks_path = codex_home.join("hooks.json");
    let current = fs::read_to_string(&config_path).unwrap_or_default();
    let next = repair_hooks_config(&current, &hooks_path);
    if next != current {
        fs::write(&config_path, next)
            .with_context(|| format!("write {}", config_path.display()))?;
    }
    Ok(())
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

fn repair_hooks_config(config_text: &str, hooks_path: &Path) -> String {
    let owned_state_keys = owned_hook_state_keys_for_hooks_path(hooks_path);
    let with_feature_enabled = set_hooks_enabled(config_text);
    set_owned_hook_state_enabled(&with_feature_enabled, &owned_state_keys)
}

fn set_hooks_enabled(config_text: &str) -> String {
    let mut lines = config_text
        .lines()
        .map(str::to_owned)
        .collect::<Vec<String>>();
    lines.retain(|line| !is_toml_key_assignment(line, LEGACY_CODEX_HOOKS_FEATURE_KEY));

    if lines.is_empty() {
        return format!("{FEATURES_TABLE_HEADER}\n{HOOKS_FEATURE_LINE}\n");
    }

    if let Some(features_index) = lines
        .iter()
        .position(|line| line.trim() == FEATURES_TABLE_HEADER)
    {
        let block_end_index = lines
            .iter()
            .enumerate()
            .skip(features_index + 1)
            .find_map(|(index, line)| {
                let trimmed = line.trim();
                (trimmed.starts_with('[') && trimmed.ends_with(']')).then_some(index)
            })
            .unwrap_or(lines.len());
        if let Some(hooks_index) = lines[features_index + 1..block_end_index]
            .iter()
            .position(|line| is_toml_key_assignment(line, HOOKS_FEATURE_KEY))
            .map(|index| features_index + 1 + index)
        {
            lines[hooks_index] = HOOKS_FEATURE_LINE.to_owned();
        } else {
            lines.insert(block_end_index, HOOKS_FEATURE_LINE.to_owned());
        }
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(FEATURES_TABLE_HEADER.to_owned());
        lines.push(HOOKS_FEATURE_LINE.to_owned());
    }

    finish_config_lines(lines)
}

fn set_owned_hook_state_enabled(config_text: &str, owned_state_keys: &[String]) -> String {
    let lines = config_text
        .lines()
        .map(str::to_owned)
        .collect::<Vec<String>>();
    let mut repaired_lines = Vec::with_capacity(lines.len() + owned_state_keys.len());
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].clone();
        let state_key = hook_state_table_key(line.trim());
        if state_key.is_some_and(|key| owned_state_keys.iter().any(|owned| owned == key)) {
            repaired_lines.push(line);
            index += 1;
            let owned_section_start = repaired_lines.len();
            let mut found_enabled_key = false;

            while index < lines.len() && !is_toml_table_header(lines[index].trim()) {
                if is_toml_key_assignment(&lines[index], ENABLED_KEY) {
                    repaired_lines.push(HOOK_ENABLED_LINE.to_owned());
                    found_enabled_key = true;
                } else {
                    repaired_lines.push(lines[index].clone());
                }
                index += 1;
            }

            if !found_enabled_key {
                repaired_lines.insert(owned_section_start, HOOK_ENABLED_LINE.to_owned());
            }
        } else {
            repaired_lines.push(line);
            index += 1;
        }
    }

    finish_config_lines(repaired_lines)
}

fn hook_state_table_key(header: &str) -> Option<&str> {
    header
        .strip_prefix(HOOK_STATE_TABLE_PREFIX)
        .and_then(|key| key.strip_suffix(TOML_TABLE_SUFFIX))
}

fn is_toml_table_header(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']')
}

fn is_toml_key_assignment(line: &str, key: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    let Some((candidate_key, _)) = trimmed.split_once('=') else {
        return false;
    };
    candidate_key.trim_end() == key
}

fn finish_config_lines(lines: Vec<String>) -> String {
    let mut next = lines.join("\n");
    next.push('\n');
    next
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
    let normalized = command.to_ascii_lowercase();
    CURRENT_HOOK_COMMAND_MARKERS
        .iter()
        .chain(LEGACY_BUN_HOOK_COMMAND_MARKERS)
        .any(|marker| normalized.contains(marker))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{repair_hooks_config, set_hooks_enabled};

    #[test]
    fn hook_feature_writer_uses_stable_key_for_empty_config() {
        assert_eq!(set_hooks_enabled(""), "[features]\nhooks = true\n");
    }

    #[test]
    fn hook_feature_writer_replaces_disabled_stable_key() {
        assert_eq!(
            set_hooks_enabled("[features]\nhooks = false\n"),
            "[features]\nhooks = true\n"
        );
    }

    #[test]
    fn hook_feature_writer_removes_legacy_key() {
        let next = set_hooks_enabled(
            "[model]\ndefault = \"gpt-5.5\"\n\n[features]\ncodex_hooks = false\n",
        );

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
        );

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
        );

        assert!(next.contains("hooks = true"));
        assert!(next.contains(
            r#"[hooks.state."/Users/ay/.codex/hooks.json:stop:0:0"]
enabled = true
trusted_hash = "sha256:owned""#
        ));
    }
}

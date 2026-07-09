// allow: SIZE_OK — Devin hook adapter keeps status inspection and payload parsing compatibility atomic.
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hook_registration::{
    HookRegistrationChange, is_owned_hook_command, register_owned_hooks_for_spec,
    unregister_owned_hooks_for_spec,
};
use crate::hooks::adapter::{DevinHookAdapter, Homes, HookAdapter};
pub use crate::hooks::adapter::{
    default_devin_config_path, is_devin_hook_invocation, parse_devin_hook_payload,
};

pub type DevinHookRegistrationChange = HookRegistrationChange;

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

pub fn register_owned_devin_hooks(
    home_path: &Path,
    hook_command: &str,
) -> Result<DevinHookRegistrationChange> {
    register_owned_hooks_for_spec(
        &DevinHookAdapter.spec(&Homes::for_home_path(home_path)),
        hook_command,
    )
}

pub fn unregister_owned_devin_hooks(home_path: &Path) -> Result<usize> {
    unregister_owned_hooks_for_spec(&DevinHookAdapter.spec(&Homes::for_home_path(home_path)))
}

pub fn inspect_devin_hooks(home_path: &Path) -> DevinHookStatus {
    let config_path = default_devin_config_path(home_path);
    let (registered_events, active_command) = read_hooks_json(&config_path).unwrap_or_default();
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

fn hook_command(value: &Value) -> Option<&str> {
    value.get("command").and_then(Value::as_str).map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

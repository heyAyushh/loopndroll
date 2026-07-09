// allow: SIZE_OK — single-file Grok hook config adapter keeps status inspection compatibility atomic.
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::hook_registration::{
    HookRegistrationChange, is_owned_hook_command, register_owned_hooks_for_spec,
    unregister_owned_hooks_for_spec,
};
use crate::hooks::adapter::{GrokHookAdapter, Homes, HookAdapter, default_grok_hooks_path};

pub type GrokHookRegistrationChange = HookRegistrationChange;

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
    register_owned_hooks_for_spec(
        &GrokHookAdapter.spec(&Homes::for_grok_home(grok_home)),
        hook_command,
    )
}

pub fn unregister_owned_grok_hooks(grok_home: &Path) -> Result<usize> {
    unregister_owned_hooks_for_spec(&GrokHookAdapter.spec(&Homes::for_grok_home(grok_home)))
}

pub fn inspect_grok_hooks(grok_home: &Path) -> GrokHookStatus {
    let hooks_path = default_grok_hooks_path(grok_home);
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

fn hook_command(value: &Value) -> Option<&str> {
    if let Some(command) = value.as_str() {
        return Some(command);
    }
    value.get("command").and_then(Value::as_str)
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

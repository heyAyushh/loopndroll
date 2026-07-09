// allow: SIZE_OK — Claude hook adapter keeps status inspection and payload parsing compatibility atomic.
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::entity_id::{
    claude_session_id_from_public_thread_id, public_thread_id_for_claude_session,
};
use crate::hook_registration::{
    HookRegistrationChange, is_owned_hook_command, register_owned_hooks_for_spec,
    unregister_owned_hooks_for_spec,
};
use crate::hooks::adapter::{ClaudeHookAdapter, Homes, HookAdapter};
pub use crate::hooks::adapter::{
    default_claude_settings_path, is_claude_hook_invocation, parse_claude_hook_payload,
};

pub type ClaudeHookRegistrationChange = HookRegistrationChange;

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

pub fn register_owned_claude_hooks(
    claude_home: &Path,
    hook_command: &str,
) -> Result<ClaudeHookRegistrationChange> {
    register_owned_hooks_for_spec(
        &ClaudeHookAdapter.spec(&Homes::for_claude_home(claude_home)),
        hook_command,
    )
}

pub fn unregister_owned_claude_hooks(claude_home: &Path) -> Result<usize> {
    unregister_owned_hooks_for_spec(&ClaudeHookAdapter.spec(&Homes::for_claude_home(claude_home)))
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

fn owned_event_names() -> [&'static str; 3] {
    ["SessionStart", "Stop", "UserPromptSubmit"]
}

fn hook_command(value: &Value) -> Option<&str> {
    value.get("command").and_then(Value::as_str).map(str::trim)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

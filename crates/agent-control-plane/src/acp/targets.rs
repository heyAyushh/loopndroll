use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const ACP_LAUNCH_METADATA_KEYS: &[&str] = &[
    "args",
    "argv",
    "command",
    "command_line",
    "commandLine",
    "distribution",
    "entrypoint",
    "executable",
    "launch",
    "path",
    "runtime",
    "stdio",
    "transport",
    "websocket",
];
const ACP_LAUNCH_METHOD_KEYS: &[&str] = &[
    "command",
    "command_line",
    "commandLine",
    "entrypoint",
    "executable",
    "npx",
    "node",
    "runtime",
    "stdio",
    "transport",
    "websocket",
];
const ACP_LAUNCH_FALLBACK_METHOD: &str = "configured";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpLaunchMetadata {
    pub configured: bool,
    pub methods: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpTarget {
    pub id: String,
    pub client: String,
    pub client_name: String,
    pub agent_id: String,
    pub name: String,
    pub source: String,
    pub source_path: Option<String>,
    pub enabled: bool,
    pub preferred: bool,
    pub launch_configured: bool,
    pub launch: AcpLaunchMetadata,
    pub ready: bool,
    pub status: String,
    pub detail: String,
}

pub fn inspect_launch_metadata(value: &Value) -> AcpLaunchMetadata {
    let mut methods = BTreeSet::new();
    let configured = collect_launch_metadata(value, &mut methods);
    if configured && methods.is_empty() {
        methods.insert(ACP_LAUNCH_FALLBACK_METHOD.to_owned());
    }
    AcpLaunchMetadata {
        configured,
        methods: methods.into_iter().collect(),
    }
}

fn collect_launch_metadata(value: &Value, methods: &mut BTreeSet<String>) -> bool {
    match value {
        Value::Object(object) => {
            let mut configured = false;
            for (key, value) in object {
                if is_launch_metadata_key(key) && !value.is_null() {
                    configured = true;
                }
                if is_launch_method_key(key) && !value.is_null() {
                    methods.insert(sanitized_launch_method(key));
                }
                configured |= collect_launch_metadata(value, methods);
            }
            configured
        }
        Value::Array(values) => values
            .iter()
            .any(|value| collect_launch_metadata(value, methods)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

fn is_launch_metadata_key(key: &str) -> bool {
    ACP_LAUNCH_METADATA_KEYS.contains(&key)
}

fn is_launch_method_key(key: &str) -> bool {
    ACP_LAUNCH_METHOD_KEYS.contains(&key)
}

fn sanitized_launch_method(key: &str) -> String {
    key.chars()
        .map(|character| match character {
            '_' => '-',
            character if character.is_ascii_uppercase() => character.to_ascii_lowercase(),
            character => character,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_metadata_detection_stays_sanitized() {
        let agent = serde_json::json!({
            "id": "codex",
            "name": "Codex",
            "distribution": {
                "npx": {
                    "package": "@agentclientprotocol/codex-acp"
                }
            }
        });

        let parsed = inspect_launch_metadata(&agent);

        assert!(parsed.configured);
        assert_eq!(parsed.methods, vec!["npx"]);
        assert!(
            !serde_json::to_string(&parsed)
                .expect("json")
                .contains("@agentclientprotocol")
        );
    }

    #[test]
    fn zed_custom_agent_command_is_launch_metadata() {
        let agent_server = serde_json::json!({
            "type": "custom",
            "command": "looper",
            "args": ["acp", "stdio"],
            "env": {
                "TOKEN": "must-not-leak"
            }
        });

        let parsed = inspect_launch_metadata(&agent_server);

        assert!(parsed.configured);
        assert_eq!(parsed.methods, vec!["command"]);
        assert!(
            !serde_json::to_string(&parsed)
                .expect("json")
                .contains("must-not-leak")
        );
    }
}

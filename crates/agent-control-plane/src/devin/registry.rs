use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::read_json_object;

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
];
const ACP_LAUNCH_FALLBACK_METHOD: &str = "configured";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpRegistryStatus {
    pub path: String,
    pub exists: bool,
    pub version: Option<String>,
    pub agents: Vec<DevinAcpAgent>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpAgent {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub launch_configured: bool,
    pub launch: DevinAcpLaunchMetadata,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpLaunchMetadata {
    pub configured: bool,
    pub methods: Vec<String>,
}

pub(super) fn inspect_acp_registry(path: &Path) -> DevinAcpRegistryStatus {
    let document = read_json_object(path);
    DevinAcpRegistryStatus {
        path: path.display().to_string(),
        exists: document.is_some(),
        version: document
            .as_ref()
            .and_then(|document| document.get("version"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        agents: document
            .as_ref()
            .and_then(|document| document.get("agents"))
            .and_then(Value::as_array)
            .map(|agents| agents.iter().filter_map(parse_acp_agent).collect())
            .unwrap_or_default(),
    }
}

fn parse_acp_agent(agent: &Value) -> Option<DevinAcpAgent> {
    let id = agent.get("id").and_then(Value::as_str)?;
    let name = agent.get("name").and_then(Value::as_str).unwrap_or(id);
    let launch = inspect_launch_metadata(agent);
    Some(DevinAcpAgent {
        id: id.to_owned(),
        name: name.to_owned(),
        version: agent
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_owned),
        description: agent
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        launch_configured: launch.configured,
        launch,
    })
}

fn inspect_launch_metadata(value: &Value) -> DevinAcpLaunchMetadata {
    let mut methods = BTreeSet::new();
    let configured = collect_launch_metadata(value, &mut methods);
    if configured && methods.is_empty() {
        methods.insert(ACP_LAUNCH_FALLBACK_METHOD.to_owned());
    }
    DevinAcpLaunchMetadata {
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
        Value::Array(values) => {
            let mut configured = false;
            for value in values {
                configured |= collect_launch_metadata(value, methods);
            }
            configured
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

fn is_launch_metadata_key(key: &str) -> bool {
    ACP_LAUNCH_METADATA_KEYS
        .iter()
        .any(|launch_key| *launch_key == key)
}

fn is_launch_method_key(key: &str) -> bool {
    ACP_LAUNCH_METHOD_KEYS
        .iter()
        .any(|launch_key| *launch_key == key)
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

        let parsed = parse_acp_agent(&agent).expect("agent");

        assert_eq!(parsed.launch_configured, true);
        assert_eq!(parsed.launch.methods, vec!["npx"]);
        assert!(
            !serde_json::to_string(&parsed)
                .expect("json")
                .contains("@agentclientprotocol")
        );
    }
}

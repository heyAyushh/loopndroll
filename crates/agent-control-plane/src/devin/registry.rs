use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp::targets::{AcpLaunchMetadata, inspect_launch_metadata};

use super::read_json_object;

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
    pub launch: AcpLaunchMetadata,
}

pub type DevinAcpLaunchMetadata = AcpLaunchMetadata;

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

        assert!(parsed.launch_configured);
        assert_eq!(parsed.launch.methods, vec!["npx"]);
        assert!(
            !serde_json::to_string(&parsed)
                .expect("json")
                .contains("@agentclientprotocol")
        );
    }
}

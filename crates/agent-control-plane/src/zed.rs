use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::acp_targets::{AcpLaunchMetadata, AcpTarget, inspect_launch_metadata};

pub const ZED_CLIENT_ID: &str = "zed";
pub const ZED_CLIENT_NAME: &str = "Zed";

const HOME_ZED_SETTINGS_RELATIVE_PATH: &str = ".zed/settings.json";
const XDG_ZED_SETTINGS_RELATIVE_PATH: &str = ".config/zed/settings.json";
const ZED_AGENT_SERVERS_KEY: &str = "agent_servers";
const ZED_ACP_TARGET_SOURCE: &str = "zed-agent-servers";
const ZED_READY_STATUS: &str = "ready";
const ZED_BLOCKED_STATUS: &str = "blocked";
const ZED_CONFIGURED_DETAIL: &str =
    "Zed External Agent target is configured; Looper did not execute its command.";
const ZED_MISSING_LAUNCH_DETAIL: &str =
    "Zed External Agent target lacks command or transport metadata.";
const ZED_PROCESS_NEEDLES: &[&str] = &[
    "/applications/zed.app/",
    "zed.app/contents/macos/zed",
    "com.zed.dev",
    "zed --foreground",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedStatus {
    pub settings_path: String,
    pub settings_exists: bool,
    pub running: bool,
    pub installed: bool,
    pub summary: String,
    pub acp_target_count: usize,
    pub acp_targets: Vec<ZedAcpTarget>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedAcpTarget {
    pub id: String,
    pub name: String,
    pub target_type: Option<String>,
    pub launch_configured: bool,
    pub launch: AcpLaunchMetadata,
}

pub fn inspect_zed_for_home(home: &Path) -> ZedStatus {
    inspect_zed_for_home_with_processes(home, &current_process_commands())
}

pub fn inspect_zed_for_home_with_processes(home: &Path, process_commands: &[String]) -> ZedStatus {
    let settings_path = selected_settings_path(home);
    let settings = read_json_object(&settings_path);
    let acp_targets = settings
        .as_ref()
        .and_then(|settings| settings.get(ZED_AGENT_SERVERS_KEY))
        .and_then(Value::as_object)
        .map(|agent_servers| {
            agent_servers
                .iter()
                .filter_map(|(id, value)| parse_zed_acp_target(id, value))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let settings_exists = settings.is_some();
    let running = zed_is_running(process_commands);
    let installed = running || settings_path.exists();

    ZedStatus {
        settings_path: settings_path.display().to_string(),
        settings_exists,
        running,
        installed,
        summary: zed_summary(settings_exists, running, acp_targets.len()),
        acp_target_count: acp_targets.len(),
        acp_targets,
    }
}

pub fn zed_acp_targets(status: &ZedStatus) -> Vec<AcpTarget> {
    status
        .acp_targets
        .iter()
        .map(|target| zed_acp_target(status, target))
        .collect()
}

fn parse_zed_acp_target(id: &str, value: &Value) -> Option<ZedAcpTarget> {
    let object = value.as_object()?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(id)
        .to_owned();
    let target_type = object
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let launch = inspect_launch_metadata(value);
    Some(ZedAcpTarget {
        id: id.to_owned(),
        name,
        target_type,
        launch_configured: launch.configured,
        launch,
    })
}

fn zed_acp_target(status: &ZedStatus, target: &ZedAcpTarget) -> AcpTarget {
    let ready = target.launch_configured;
    AcpTarget {
        id: format!("{ZED_CLIENT_ID}:{}", target.id),
        client: ZED_CLIENT_ID.to_owned(),
        client_name: ZED_CLIENT_NAME.to_owned(),
        agent_id: target.id.clone(),
        name: target.name.clone(),
        source: ZED_ACP_TARGET_SOURCE.to_owned(),
        source_path: Some(status.settings_path.clone()),
        enabled: true,
        preferred: false,
        launch_configured: target.launch_configured,
        launch: target.launch.clone(),
        ready,
        status: if ready {
            ZED_READY_STATUS.to_owned()
        } else {
            ZED_BLOCKED_STATUS.to_owned()
        },
        detail: if ready {
            ZED_CONFIGURED_DETAIL.to_owned()
        } else {
            ZED_MISSING_LAUNCH_DETAIL.to_owned()
        },
    }
}

fn selected_settings_path(home: &Path) -> PathBuf {
    settings_path_candidates(home)
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| home.join(HOME_ZED_SETTINGS_RELATIVE_PATH))
}

fn settings_path_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(HOME_ZED_SETTINGS_RELATIVE_PATH),
        home.join(XDG_ZED_SETTINGS_RELATIVE_PATH),
    ]
}

fn zed_summary(settings_exists: bool, running: bool, target_count: usize) -> String {
    match (settings_exists, target_count, running) {
        (true, 0, _) => "Zed settings contain no ACP External Agent targets".to_owned(),
        (true, 1, true) => "Zed is running with 1 configured ACP External Agent target".to_owned(),
        (true, count, true) => {
            format!("Zed is running with {count} configured ACP External Agent targets")
        }
        (true, 1, false) => "Zed has 1 configured ACP External Agent target".to_owned(),
        (true, count, false) => {
            format!("Zed has {count} configured ACP External Agent targets")
        }
        (false, _, true) => "Zed is running; settings file not found".to_owned(),
        (false, _, false) => "Zed settings file not found".to_owned(),
    }
}

fn read_json_object(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let value = serde_json::from_slice::<Value>(&bytes).ok()?;
    value.is_object().then_some(value)
}

fn zed_is_running(process_commands: &[String]) -> bool {
    process_commands.iter().any(|command| {
        let normalized = command.to_ascii_lowercase();
        ZED_PROCESS_NEEDLES
            .iter()
            .any(|needle| normalized.contains(&needle.to_ascii_lowercase()))
    })
}

fn current_process_commands() -> Vec<String> {
    let output = std::process::Command::new("/bin/ps")
        .args(["-axo", "command="])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zed_status_reads_agent_servers_without_leaking_values() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(
            &settings_path,
            serde_json::json!({
                "agent_servers": {
                    "looper": {
                        "type": "custom",
                        "command": "looper",
                        "args": ["acp", "stdio"],
                        "env": {
                            "TOKEN": "must-not-leak"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("write zed settings");

        let status = inspect_zed_for_home_with_processes(
            temp_dir.path(),
            &["/Applications/Zed.app/Contents/MacOS/zed --foreground".to_owned()],
        );

        assert!(status.running);
        assert!(status.installed);
        assert_eq!(status.acp_target_count, 1);
        assert_eq!(status.acp_targets[0].id, "looper");
        assert_eq!(status.acp_targets[0].launch.methods, vec!["command"]);
        let json = serde_json::to_string(&status).expect("zed status json");
        assert!(!json.contains("must-not-leak"));
        assert!(!json.contains("\"args\""));
    }

    #[test]
    fn zed_acp_targets_report_blocked_missing_launch_metadata() {
        let status = ZedStatus {
            settings_path: "/tmp/.zed/settings.json".to_owned(),
            settings_exists: true,
            running: false,
            installed: true,
            summary: "Zed has 1 configured ACP External Agent target".to_owned(),
            acp_target_count: 1,
            acp_targets: vec![ZedAcpTarget {
                id: "configured-only".to_owned(),
                name: "configured-only".to_owned(),
                target_type: Some("custom".to_owned()),
                launch_configured: false,
                launch: AcpLaunchMetadata::default(),
            }],
        };

        let targets = zed_acp_targets(&status);

        assert_eq!(targets[0].id, "zed:configured-only");
        assert_eq!(targets[0].client, "zed");
        assert_eq!(targets[0].status, "blocked");
        assert!(!targets[0].ready);
    }
}

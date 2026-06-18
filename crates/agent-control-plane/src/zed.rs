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
const ZED_READ_ONLY_STATUS: &str = "read-only";
const ZED_BLOCKED_STATUS: &str = "blocked";
const ZED_READ_ONLY_DETAIL: &str = "Zed External Agent target is configured; Looper reports it read-only and does not execute its command.";
const ZED_MISSING_LAUNCH_DETAIL: &str =
    "Zed External Agent target lacks command or transport metadata.";
const ZED_PROCESS_NEEDLES: &[&str] = &[
    "/applications/zed.app/",
    "zed.app/contents/macos/zed",
    "com.zed.dev",
    "zed --foreground",
];

/// Snapshot of Zed's local ACP visibility state.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedStatus {
    /// Settings file selected for Zed ACP discovery.
    pub settings_path: String,
    /// Whether the selected settings path exists.
    pub settings_exists: bool,
    /// Sanitized read or parse failure category, when settings could not be used.
    pub settings_error: Option<String>,
    /// Whether a Zed process is currently visible.
    pub running: bool,
    /// Whether Looper can infer that Zed is installed.
    pub installed: bool,
    /// User-facing visibility summary.
    pub summary: String,
    /// Number of configured ACP External Agent targets.
    pub acp_target_count: usize,
    /// Sanitized ACP target metadata parsed from `agent_servers`.
    pub acp_targets: Vec<ZedAcpTarget>,
}

/// Zed External Agent target parsed from `agent_servers`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZedAcpTarget {
    /// Target id from Zed's `agent_servers` object key.
    pub id: String,
    /// Display name, falling back to the target id.
    pub name: String,
    /// Zed target type, for example `custom`.
    pub target_type: Option<String>,
    /// Whether sanitized launch metadata was present.
    pub launch_configured: bool,
    /// Redacted launch metadata; raw commands, args, and environment are omitted.
    pub launch: AcpLaunchMetadata,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZedSettingsReadError {
    ReadFailed,
    InvalidUtf8,
    InvalidJson,
    NonObject,
}

impl ZedSettingsReadError {
    fn code(self) -> &'static str {
        match self {
            Self::ReadFailed => "read-failed",
            Self::InvalidUtf8 => "invalid-utf8",
            Self::InvalidJson => "invalid-json",
            Self::NonObject => "non-object-json",
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::ReadFailed => "Zed settings file is unreadable",
            Self::InvalidUtf8 => "Zed settings file is not valid UTF-8",
            Self::InvalidJson => "Zed settings file is not valid JSON or JSONC",
            Self::NonObject => "Zed settings file is not a JSON object",
        }
    }
}

/// Inspect Zed settings and running processes for the current machine user.
pub fn inspect_zed_for_home(home: &Path) -> ZedStatus {
    inspect_zed_for_home_with_processes(home, &current_process_commands())
}

/// Inspect Zed settings using caller-supplied process command lines.
///
/// Tests use this to keep Zed detection deterministic without probing the host
/// process table.
pub fn inspect_zed_for_home_with_processes(home: &Path, process_commands: &[String]) -> ZedStatus {
    let settings_path = selected_settings_path(home);
    let settings_exists = settings_path.exists();
    let settings_result = settings_exists
        .then(|| read_json_object(&settings_path))
        .transpose();
    let settings_error = settings_result.as_ref().err().copied();
    let settings = settings_result.ok().flatten();
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
    let running = zed_is_running(process_commands);
    let installed = running;

    ZedStatus {
        settings_path: settings_path.display().to_string(),
        settings_exists,
        settings_error: settings_error.map(|error| error.code().to_owned()),
        running,
        installed,
        summary: zed_summary(settings_exists, settings_error, running, acp_targets.len()),
        acp_target_count: acp_targets.len(),
        acp_targets,
    }
}

/// Convert Zed External Agent targets into Looper's generic ACP target model.
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
        ready: false,
        status: if target.launch_configured {
            ZED_READ_ONLY_STATUS.to_owned()
        } else {
            ZED_BLOCKED_STATUS.to_owned()
        },
        detail: if target.launch_configured {
            ZED_READ_ONLY_DETAIL.to_owned()
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

fn zed_summary(
    settings_exists: bool,
    settings_error: Option<ZedSettingsReadError>,
    running: bool,
    target_count: usize,
) -> String {
    match (settings_exists, settings_error, target_count, running) {
        (true, Some(error), _, _) => error.summary().to_owned(),
        (true, None, 0, _) => "Zed settings contain no ACP External Agent targets".to_owned(),
        (true, None, 1, true) => {
            "Zed is running with 1 configured ACP External Agent target".to_owned()
        }
        (true, None, count, true) => {
            format!("Zed is running with {count} configured ACP External Agent targets")
        }
        (true, None, 1, false) => "Zed has 1 configured ACP External Agent target".to_owned(),
        (true, None, count, false) => {
            format!("Zed has {count} configured ACP External Agent targets")
        }
        (false, _, _, true) => "Zed is running; settings file not found".to_owned(),
        (false, _, _, false) => "Zed settings file not found".to_owned(),
    }
}

fn read_json_object(path: &Path) -> Result<Value, ZedSettingsReadError> {
    let bytes = fs::read(path).map_err(|_| ZedSettingsReadError::ReadFailed)?;
    let value = match serde_json::from_slice::<Value>(&bytes) {
        Ok(value) => value,
        Err(_) => {
            let settings_text =
                std::str::from_utf8(&bytes).map_err(|_| ZedSettingsReadError::InvalidUtf8)?;
            let jsonc = strip_jsonc_comments(settings_text);
            let json = remove_json_trailing_commas(&jsonc);
            serde_json::from_str::<Value>(&json).map_err(|_| ZedSettingsReadError::InvalidJson)?
        }
    };
    if value.is_object() {
        Ok(value)
    } else {
        Err(ZedSettingsReadError::NonObject)
    }
}

fn strip_jsonc_comments(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    let mut inside_string = false;
    let mut escaped = false;

    while let Some(character) = characters.next() {
        if inside_string {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                inside_string = false;
            }
            continue;
        }

        if character == '"' {
            inside_string = true;
            output.push(character);
            continue;
        }

        if character != '/' {
            output.push(character);
            continue;
        }

        match characters.peek() {
            Some('/') => {
                characters.next();
                for comment_character in characters.by_ref() {
                    if comment_character == '\n' {
                        output.push('\n');
                        break;
                    }
                }
            }
            Some('*') => {
                characters.next();
                let mut previous_character = '\0';
                for comment_character in characters.by_ref() {
                    if comment_character == '\n' {
                        output.push('\n');
                    }
                    if previous_character == '*' && comment_character == '/' {
                        break;
                    }
                    previous_character = comment_character;
                }
            }
            _ => output.push(character),
        }
    }

    output
}

fn remove_json_trailing_commas(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    let mut inside_string = false;
    let mut escaped = false;

    while let Some(character) = characters.next() {
        if inside_string {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                inside_string = false;
            }
            continue;
        }

        if character == '"' {
            inside_string = true;
            output.push(character);
            continue;
        }

        if character == ',' && next_non_whitespace_is_container_end(characters.clone()) {
            continue;
        }

        output.push(character);
    }

    output
}

fn next_non_whitespace_is_container_end(
    characters: std::iter::Peekable<std::str::Chars<'_>>,
) -> bool {
    for character in characters {
        if !character.is_whitespace() {
            return character == '}' || character == ']';
        }
    }
    false
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
            r#"
            // Zed settings are JSONC.
            {
                "agent_servers": {
                    "looper": {
                        "type": "custom",
                        "command": "looper",
                        "args": ["acp", "stdio"],
                        "env": {
                            "TOKEN": "must-not-leak",
                            "URL": "https://example.test/not-a-comment"
                        },
                    },
                }
            }
            "#,
        )
        .expect("write zed settings");

        let status = inspect_zed_for_home_with_processes(
            temp_dir.path(),
            &["/Applications/Zed.app/Contents/MacOS/zed --foreground".to_owned()],
        );

        assert!(status.running);
        assert!(status.installed);
        assert_eq!(status.settings_error, None);
        assert_eq!(status.acp_target_count, 1);
        assert_eq!(status.acp_targets[0].id, "looper");
        assert_eq!(status.acp_targets[0].launch.methods, vec!["command"]);
        let json = serde_json::to_string(&status).expect("zed status json");
        assert!(!json.contains("must-not-leak"));
        assert!(!json.contains("\"args\""));
    }

    #[test]
    fn zed_status_reports_invalid_jsonc_settings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(&settings_path, "{ invalid json").expect("write zed settings");

        let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);

        assert!(status.settings_exists);
        assert!(!status.installed);
        assert_eq!(status.settings_error.as_deref(), Some("invalid-json"));
        assert_eq!(status.acp_target_count, 0);
        assert_eq!(
            status.summary,
            "Zed settings file is not valid JSON or JSONC"
        );
    }

    #[test]
    fn zed_status_reports_invalid_utf8_settings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(&settings_path, [0xff, 0xfe]).expect("write zed settings");

        let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);

        assert_eq!(status.settings_error.as_deref(), Some("invalid-utf8"));
        assert_eq!(status.summary, "Zed settings file is not valid UTF-8");
    }

    #[test]
    fn zed_status_reports_non_object_json_settings() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(&settings_path, "[]").expect("write zed settings");

        let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);

        assert_eq!(status.settings_error.as_deref(), Some("non-object-json"));
        assert_eq!(status.summary, "Zed settings file is not a JSON object");
    }

    #[test]
    fn zed_acp_targets_report_configured_targets_read_only() {
        let status = ZedStatus {
            settings_path: "/tmp/.zed/settings.json".to_owned(),
            settings_exists: true,
            settings_error: None,
            running: true,
            installed: true,
            summary: "Zed is running with 1 configured ACP External Agent target".to_owned(),
            acp_target_count: 1,
            acp_targets: vec![ZedAcpTarget {
                id: "looper".to_owned(),
                name: "looper".to_owned(),
                target_type: Some("custom".to_owned()),
                launch_configured: true,
                launch: AcpLaunchMetadata {
                    configured: true,
                    methods: vec!["command".to_owned()],
                },
            }],
        };

        let targets = zed_acp_targets(&status);

        assert_eq!(targets[0].id, "zed:looper");
        assert_eq!(targets[0].client, "zed");
        assert_eq!(targets[0].status, "read-only");
        assert!(!targets[0].ready);
        assert!(targets[0].detail.contains("read-only"));
    }

    #[test]
    fn zed_acp_targets_report_blocked_missing_launch_metadata() {
        let status = ZedStatus {
            settings_path: "/tmp/.zed/settings.json".to_owned(),
            settings_exists: true,
            settings_error: None,
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

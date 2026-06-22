use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::targets::parse_zed_acp_target;
use super::{
    HOME_ZED_SETTINGS_RELATIVE_PATH, XDG_ZED_SETTINGS_RELATIVE_PATH, ZED_AGENT_SERVERS_KEY,
    ZED_PROCESS_NEEDLES, ZedStatus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ZedSettingsReadError {
    ReadFailed,
    InvalidUtf8,
    InvalidJson,
    NonObject,
}

impl ZedSettingsReadError {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::ReadFailed => "read-failed",
            Self::InvalidUtf8 => "invalid-utf8",
            Self::InvalidJson => "invalid-json",
            Self::NonObject => "non-object-json",
        }
    }

    pub(super) fn summary(self) -> &'static str {
        match self {
            Self::ReadFailed => "Zed settings file is unreadable",
            Self::InvalidUtf8 => "Zed settings file is not valid UTF-8",
            Self::InvalidJson => "Zed settings file is not valid JSON or JSONC",
            Self::NonObject => "Zed settings file is not a JSON object",
        }
    }
}

pub fn inspect_zed_for_home(home: &Path) -> ZedStatus {
    inspect_zed_for_home_with_processes(home, &current_process_commands())
}

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

pub(super) fn write_json_document(path: &Path, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| anyhow::anyhow!("create {}: {error}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{content}\n"))
        .map_err(|error| anyhow::anyhow!("write {}: {error}", path.display()))
}

pub(super) fn selected_settings_path(home: &Path) -> PathBuf {
    settings_path_candidates(home)
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| home.join(HOME_ZED_SETTINGS_RELATIVE_PATH))
}

pub(super) fn settings_path_candidates(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(HOME_ZED_SETTINGS_RELATIVE_PATH),
        home.join(XDG_ZED_SETTINGS_RELATIVE_PATH),
    ]
}

pub(super) fn read_json_object(path: &Path) -> Result<Value, ZedSettingsReadError> {
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

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::privacy::redact_command_for_display;

const SUPERCONDUCTOR_BUNDLE_ID: &str = "com.zarifpour.superconductor";
const CURSOR_BUNDLE_ID: &str = "com.todesktop.230313mzl4w4u92";
const DEFAULT_CLAUDE_BUNDLE_ID: &str = "com.anthropic.claudefordesktop";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AssistantKind {
    Codex,
    Superconductor,
    Cursor,
    ClaudeCode,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "openclaw")]
    OpenClaw,
    Hermes,
    Poke,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantAdapterCapability {
    pub assistant_kind: AssistantKind,
    pub live_sessions: bool,
    pub tool_inventory: bool,
    pub spawn_graph: bool,
    pub diff_summary: bool,
    pub auth_capabilities: bool,
    pub runtimes: Vec<AssistantRuntime>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssistantRuntime {
    pub kind: AssistantRuntimeKind,
    pub running: bool,
    pub installed: bool,
    pub label: String,
    pub bundle_id: Option<String>,
    pub executable: Option<String>,
    pub command: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistantRuntimeKind {
    Gui,
    Cli,
}

pub fn adapter_capabilities() -> Vec<AssistantAdapterCapability> {
    discover_assistant_adapters()
}

pub fn discover_assistant_adapters() -> Vec<AssistantAdapterCapability> {
    let process_commands = current_process_commands();
    let cli_paths = current_cli_paths(&[
        "codex",
        "superconductor",
        "cursor",
        "claude",
        "claude-code",
        "opencode",
        "openclaw",
        "open-claw",
        "hermes",
        "poke",
    ]);
    discover_assistant_adapters_from_sources(&process_commands, &cli_paths)
}

pub fn discover_assistant_adapters_from_processes(
    process_commands: &[String],
) -> Vec<AssistantAdapterCapability> {
    discover_assistant_adapters_from_sources(process_commands, &BTreeMap::new())
}

pub fn discover_assistant_adapters_from_sources(
    process_commands: &[String],
    cli_paths: &BTreeMap<String, String>,
) -> Vec<AssistantAdapterCapability> {
    let detections = AdapterDetections::from_sources(process_commands, cli_paths);
    vec![
        AssistantAdapterCapability {
            assistant_kind: AssistantKind::Codex,
            live_sessions: true,
            tool_inventory: true,
            spawn_graph: true,
            diff_summary: true,
            auth_capabilities: true,
            runtimes: vec![
                detections.runtime(
                    AssistantRuntimeKind::Gui,
                    "Codex app",
                    Some("com.openai.codex"),
                    &["codex.app", "com.openai.codex", "codex app-server"],
                ),
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Codex CLI",
                    None,
                    &["codex"],
                ),
            ],
            detail: "local Codex state, logs, rollout files, hooks, and automations".to_owned(),
        },
        runtime_only(
            AssistantKind::Superconductor,
            vec![
                detections.runtime(
                    AssistantRuntimeKind::Gui,
                    "Superconductor app",
                    Some(SUPERCONDUCTOR_BUNDLE_ID),
                    &["superconductor", SUPERCONDUCTOR_BUNDLE_ID],
                ),
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Superconductor CLI",
                    None,
                    &["superconductor"],
                ),
            ],
            "GUI/CLI runtime detection only; no session reader yet",
        ),
        runtime_only(
            AssistantKind::Cursor,
            vec![
                detections.runtime(
                    AssistantRuntimeKind::Gui,
                    "Cursor app",
                    Some(CURSOR_BUNDLE_ID),
                    &["/cursor.app/", "cursor --type", CURSOR_BUNDLE_ID],
                ),
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Cursor CLI",
                    None,
                    &["cursor"],
                ),
            ],
            "GUI/CLI runtime detection only; Codex sessions spawned by Cursor are attributed separately",
        ),
        runtime_only(
            AssistantKind::ClaudeCode,
            vec![
                detections.runtime(
                    AssistantRuntimeKind::Gui,
                    "Claude desktop",
                    Some(DEFAULT_CLAUDE_BUNDLE_ID),
                    &["claudefordesktop", DEFAULT_CLAUDE_BUNDLE_ID],
                ),
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Claude Code CLI",
                    None,
                    &["claude ", "claude-code"],
                ),
            ],
            "GUI/CLI runtime detection only; session reader not implemented in this phase",
        ),
        runtime_only(
            AssistantKind::OpenCode,
            vec![detections.runtime_by_executable(
                AssistantRuntimeKind::Cli,
                "opencode CLI",
                None,
                &["opencode"],
            )],
            "CLI runtime detection only; opencode session reader not implemented in this phase",
        ),
        runtime_only(
            AssistantKind::OpenClaw,
            vec![detections.runtime_by_executable(
                AssistantRuntimeKind::Cli,
                "OpenClaw CLI",
                None,
                &["openclaw", "open-claw"],
            )],
            "CLI runtime detection only; session reader not implemented in this phase",
        ),
        runtime_only(
            AssistantKind::Hermes,
            vec![detections.runtime_by_executable(
                AssistantRuntimeKind::Cli,
                "Hermes CLI",
                None,
                &["hermes"],
            )],
            "CLI runtime detection only; session reader not implemented in this phase",
        ),
        runtime_only(
            AssistantKind::Poke,
            vec![detections.runtime_by_executable(
                AssistantRuntimeKind::Cli,
                "Poke CLI",
                None,
                &["poke"],
            )],
            "CLI runtime detection only; session reader not implemented in this phase",
        ),
    ]
}

fn runtime_only(
    assistant_kind: AssistantKind,
    runtimes: Vec<AssistantRuntime>,
    detail: &str,
) -> AssistantAdapterCapability {
    AssistantAdapterCapability {
        assistant_kind,
        live_sessions: false,
        tool_inventory: false,
        spawn_graph: false,
        diff_summary: false,
        auth_capabilities: true,
        runtimes,
        detail: detail.to_owned(),
    }
}

#[derive(Debug)]
struct AdapterDetections<'a> {
    process_commands: &'a [String],
    cli_paths: &'a BTreeMap<String, String>,
}

impl<'a> AdapterDetections<'a> {
    fn from_sources(
        process_commands: &'a [String],
        cli_paths: &'a BTreeMap<String, String>,
    ) -> Self {
        Self {
            process_commands,
            cli_paths,
        }
    }

    fn runtime(
        &self,
        kind: AssistantRuntimeKind,
        label: &str,
        bundle_id: Option<&str>,
        needles: &[&str],
    ) -> AssistantRuntime {
        let matched_command = self.find_matching_process(needles);
        let executable = matched_command
            .as_deref()
            .and_then(first_executable_from_command);
        AssistantRuntime {
            kind,
            running: matched_command.is_some(),
            installed: matched_command.is_some(),
            label: label.to_owned(),
            bundle_id: bundle_id.map(str::to_owned),
            executable,
            command: matched_command.as_deref().map(redact_command_for_display),
        }
    }

    fn runtime_by_executable(
        &self,
        kind: AssistantRuntimeKind,
        label: &str,
        bundle_id: Option<&str>,
        executable_names: &[&str],
    ) -> AssistantRuntime {
        let matched_command = self.find_matching_executable(executable_names);
        let executable = matched_command
            .as_deref()
            .and_then(first_executable_from_command);
        let installed_executable = executable_names
            .iter()
            .find_map(|name| self.cli_paths.get(*name))
            .cloned();
        AssistantRuntime {
            kind,
            running: matched_command.is_some(),
            installed: matched_command.is_some() || installed_executable.is_some(),
            label: label.to_owned(),
            bundle_id: bundle_id.map(str::to_owned),
            executable: executable.or(installed_executable),
            command: matched_command.as_deref().map(redact_command_for_display),
        }
    }

    fn find_matching_process(&self, needles: &[&str]) -> Option<String> {
        self.process_commands.iter().find_map(|command| {
            let normalized = command.to_ascii_lowercase();
            needles
                .iter()
                .any(|needle| normalized.contains(&needle.to_ascii_lowercase()))
                .then(|| command.clone())
        })
    }

    fn find_matching_executable(&self, executable_names: &[&str]) -> Option<String> {
        self.process_commands.iter().find_map(|command| {
            let normalized = command.to_ascii_lowercase();
            if normalized.contains(".app/contents/") || normalized.contains("helper") {
                return None;
            }
            let executable = first_executable_from_command(command)?;
            let executable_name = std::path::Path::new(&executable)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&executable)
                .to_ascii_lowercase();
            executable_names
                .iter()
                .any(|candidate| executable_name == candidate.to_ascii_lowercase())
                .then(|| command.clone())
        })
    }
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

fn current_cli_paths(command_names: &[&str]) -> BTreeMap<String, String> {
    command_names
        .iter()
        .filter_map(|command_name| {
            let output = std::process::Command::new("/usr/bin/env")
                .args(["zsh", "-lc", &format!("command -v {command_name}")])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if path.is_empty() {
                None
            } else {
                Some(((*command_name).to_owned(), path))
            }
        })
        .collect()
}

fn first_executable_from_command(command: &str) -> Option<String> {
    command.split_whitespace().next().map(str::to_owned)
}

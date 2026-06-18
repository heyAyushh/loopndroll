use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::privacy::redact_command_for_display;

const SUPERCONDUCTOR_BUNDLE_ID: &str = "com.zarifpour.superconductor";
const CURSOR_BUNDLE_ID: &str = "com.todesktop.230313mzl4w4u92";
const DEFAULT_CLAUDE_BUNDLE_ID: &str = "com.anthropic.claudefordesktop";
const DEVIN_DESKTOP_CLI: &str = "devin-desktop-next";
const GROK_BUILD_CLI: &str = "grok";
const ZED_CLI: &str = "zed";
const GROK_BUILD_PROCESS_NEEDLES: &[&str] =
    &["/.grok/", ".grok/sessions", "grok agent", "grok-build"];
const ZED_PROCESS_NEEDLES: &[&str] = &[
    "/applications/zed.app/",
    "zed.app/contents/macos/zed",
    "com.zed.dev",
    "zed --foreground",
];
const CODEX_CLIENT: &str = "codex";
const DEVIN_CLIENT: &str = "devin";
const GROK_BUILD_CLIENT: &str = "grok-build";
const CLAUDE_CODE_CLIENT: &str = "claude-code";
const CODEX_ORIGINATOR_NEEDLES: &[&str] = &["codex desktop", "codex app"];
const DEVIN_ORIGINATOR_NEEDLES: &[&str] = &["devin", "devin desktop", "devin next", "devin - next"];
const CLAUDE_ORIGINATOR_NEEDLES: &[&str] = &[
    "claude code",
    "claude desktop",
    "claudefordesktop",
    "anthropic claude",
];
const CODEX_SURFACE_CLIENTS: &[&str] = &[CODEX_CLIENT, "cursor", "super-engineering", "openclaw"];
const DEVIN_SURFACE_CLIENTS: &[&str] = &[DEVIN_CLIENT];
const GROK_BUILD_SURFACE_CLIENTS: &[&str] = &[GROK_BUILD_CLIENT];
const CLAUDE_CODE_SURFACE_CLIENTS: &[&str] = &[CLAUDE_CODE_CLIENT];
const ZED_SURFACE_CLIENTS: &[&str] = &["zed"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AssistantKind {
    Codex,
    DevinDesktop,
    Superconductor,
    Cursor,
    ClaudeCode,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "openclaw")]
    OpenClaw,
    Hermes,
    Poke,
    #[serde(rename = "grok-build")]
    GrokBuild,
    Zed,
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

pub fn static_adapter_capabilities() -> Vec<AssistantAdapterCapability> {
    discover_assistant_adapters_from_sources(&[], &BTreeMap::new())
}

pub fn discover_assistant_adapters() -> Vec<AssistantAdapterCapability> {
    let process_commands = current_process_commands();
    let cli_paths = current_cli_paths(&[
        "codex",
        DEVIN_DESKTOP_CLI,
        "superconductor",
        "cursor",
        "claude",
        "claude-code",
        "opencode",
        "openclaw",
        "open-claw",
        "hermes",
        "poke",
        GROK_BUILD_CLI,
        ZED_CLI,
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
        AssistantAdapterCapability {
            assistant_kind: AssistantKind::DevinDesktop,
            live_sessions: true,
            tool_inventory: false,
            spawn_graph: false,
            diff_summary: false,
            auth_capabilities: true,
            runtimes: vec![
                detections.runtime(
                    AssistantRuntimeKind::Gui,
                    "Devin Desktop",
                    None,
                    &[
                        "/applications/devin.app/",
                        "/applications/devin - next.app/",
                        ".devin-next",
                        "devin - next helper",
                        "devin-desktop",
                        "devin desktop",
                    ],
                ),
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Devin Desktop CLI",
                    None,
                    &[DEVIN_DESKTOP_CLI],
                ),
            ],
            detail: "Devin Desktop runtime, Devin Cloud metadata, and local ACP event-log sessions"
                .to_owned(),
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
        AssistantAdapterCapability {
            assistant_kind: AssistantKind::ClaudeCode,
            live_sessions: true,
            tool_inventory: false,
            spawn_graph: false,
            diff_summary: false,
            auth_capabilities: true,
            runtimes: vec![
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
                    &["claude", "claude-code"],
                ),
            ],
            detail: "Claude Code sessions read from ~/.claude/projects; prompts delivered through Looper-owned Claude hooks".to_owned(),
        },
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
        AssistantAdapterCapability {
            assistant_kind: AssistantKind::GrokBuild,
            live_sessions: true,
            tool_inventory: false,
            spawn_graph: false,
            diff_summary: false,
            auth_capabilities: false,
            runtimes: vec![
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Grok Build CLI",
                    None,
                    &[GROK_BUILD_CLI],
                ),
                detections.runtime(
                    AssistantRuntimeKind::Cli,
                    "Grok Build session",
                    None,
                    GROK_BUILD_PROCESS_NEEDLES,
                ),
            ],
            detail: "Grok hooks at ~/.grok/hooks/looper.json; sessions read from ~/.grok/sessions/"
                .to_owned(),
        },
        runtime_only(
            AssistantKind::Zed,
            vec![
                detections.runtime(AssistantRuntimeKind::Gui, "Zed app", None, ZED_PROCESS_NEEDLES),
                detections.runtime_by_executable(
                    AssistantRuntimeKind::Cli,
                    "Zed CLI",
                    None,
                    &[ZED_CLI],
                ),
            ],
            "Zed ACP External Agent targets read from ~/.zed/settings.json or ~/.config/zed/settings.json agent_servers",
        ),
    ]
}

pub fn infer_assistant_client_from_paths(
    transcript_path: Option<&str>,
    cwd: Option<&str>,
    source: Option<&str>,
    originator: Option<&str>,
    agent_path: Option<&str>,
) -> &'static str {
    let primary_haystack = [transcript_path, source, originator, agent_path]
        .into_iter()
        .flatten()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ");
    let path_haystack = [transcript_path, cwd, source, agent_path]
        .into_iter()
        .flatten()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>()
        .join(" ");

    if path_contains_any(&primary_haystack, GROK_BUILD_PROCESS_NEEDLES) {
        return GROK_BUILD_CLIENT;
    }
    if originator_contains_any(originator, DEVIN_ORIGINATOR_NEEDLES) {
        return DEVIN_CLIENT;
    }
    if originator_contains_any(originator, CLAUDE_ORIGINATOR_NEEDLES)
        || path_contains_any(&primary_haystack, &[CLAUDE_CODE_CLIENT, "claudefordesktop"])
    {
        return CLAUDE_CODE_CLIENT;
    }
    if originator_contains_any(originator, CODEX_ORIGINATOR_NEEDLES) {
        return CODEX_CLIENT;
    }
    if path_contains_any(
        &path_haystack,
        &[
            ".devin-next",
            "/applications/devin.app/",
            "/applications/devin - next.app/",
            "devin-desktop",
        ],
    ) {
        return DEVIN_CLIENT;
    }
    if path_contains_any(&path_haystack, GROK_BUILD_PROCESS_NEEDLES) {
        return GROK_BUILD_CLIENT;
    }
    if path_contains_any(
        &path_haystack,
        &["/.cursor/", ".cursor/extensions", "/cursor.app/"],
    ) {
        return "cursor";
    }
    if path_contains_any(
        &path_haystack,
        &[".claude/", "claude-code", "claudefordesktop"],
    ) {
        return CLAUDE_CODE_CLIENT;
    }
    if path_contains_any(&path_haystack, &[".superconductor", "super-engineering"]) {
        return "super-engineering";
    }
    if path_contains_any(&path_haystack, &["openclaw", "open-claw"]) {
        return "openclaw";
    }
    if path_contains_any(
        &path_haystack,
        &[
            "/.zed/",
            "/applications/zed.app/",
            "zed acp",
            "zed-agent-servers",
            "zed.dev",
        ],
    ) {
        return "zed";
    }
    if path_contains_any(&path_haystack, &["/.codex/", ".codex/sessions"]) {
        return CODEX_CLIENT;
    }

    CODEX_CLIENT
}

pub fn assistant_kind_from_client(client: &str) -> AssistantKind {
    match client {
        "grok-build" => AssistantKind::GrokBuild,
        "devin" => AssistantKind::DevinDesktop,
        "cursor" => AssistantKind::Cursor,
        CLAUDE_CODE_CLIENT => AssistantKind::ClaudeCode,
        "super-engineering" => AssistantKind::Superconductor,
        "openclaw" => AssistantKind::OpenClaw,
        "zed" => AssistantKind::Zed,
        _ => AssistantKind::Codex,
    }
}

pub fn session_matches_assistant_surface(
    transcript_path: Option<&str>,
    cwd: Option<&str>,
    source: Option<&str>,
    originator: Option<&str>,
    agent_path: Option<&str>,
    surface: &str,
) -> bool {
    let client =
        infer_assistant_client_from_paths(transcript_path, cwd, source, originator, agent_path);
    assistant_client_matches_surface(client, surface)
}

pub fn assistant_client_matches_surface(client: &str, surface: &str) -> bool {
    match surface {
        DEVIN_CLIENT => DEVIN_SURFACE_CLIENTS.contains(&client),
        GROK_BUILD_CLIENT => GROK_BUILD_SURFACE_CLIENTS.contains(&client),
        CLAUDE_CODE_CLIENT => CLAUDE_CODE_SURFACE_CLIENTS.contains(&client),
        "zed" => ZED_SURFACE_CLIENTS.contains(&client),
        _ => CODEX_SURFACE_CLIENTS.contains(&client),
    }
}

fn path_contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| haystack.contains(&needle.to_ascii_lowercase()))
}

fn originator_contains_any(originator: Option<&str>, needles: &[&str]) -> bool {
    originator
        .map(str::to_ascii_lowercase)
        .is_some_and(|originator| needles.iter().any(|needle| originator.contains(needle)))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_grok_build_client_from_grok_session_paths() {
        assert_eq!(
            infer_assistant_client_from_paths(
                Some("/Users/test/.grok/sessions/project/thread.jsonl"),
                Some("/Users/test/project"),
                None,
                None,
                None,
            ),
            "grok-build"
        );
    }

    #[test]
    fn infers_grok_build_client_from_worktree_cwd() {
        assert_eq!(
            infer_assistant_client_from_paths(
                None,
                Some("/Users/test/.grok/worktrees/documents-looper/sse"),
                None,
                None,
                None,
            ),
            "grok-build"
        );
    }

    #[test]
    fn infers_grok_build_client_from_agent_path() {
        assert_eq!(
            infer_assistant_client_from_paths(
                None,
                Some("/Users/test/project"),
                None,
                None,
                Some("/Users/test/.grok/bin/grok"),
            ),
            "grok-build"
        );
    }

    #[test]
    fn infers_codex_client_when_paths_are_ambiguous() {
        assert_eq!(
            infer_assistant_client_from_paths(
                Some("/Users/test/.codex/sessions/thread-main.jsonl"),
                Some("/Users/test/project"),
                None,
                None,
                None,
            ),
            "codex"
        );
    }

    #[test]
    fn infers_codex_client_from_codex_desktop_originator_with_vscode_source() {
        assert_eq!(
            infer_assistant_client_from_paths(
                Some("/Users/test/.codex/sessions/thread-main.jsonl"),
                None,
                Some("vscode"),
                Some("Codex Desktop"),
                None,
            ),
            "codex"
        );
    }

    #[test]
    fn infers_devin_client_from_devin_originator_with_vscode_source() {
        assert_eq!(
            infer_assistant_client_from_paths(
                Some("/Users/test/.codex/sessions/thread-main.jsonl"),
                Some("/Users/test/project"),
                Some("vscode"),
                Some("Devin - Next"),
                None,
            ),
            "devin"
        );
    }

    #[test]
    fn infers_claude_client_from_claude_originator_with_vscode_source() {
        assert_eq!(
            infer_assistant_client_from_paths(
                Some("/Users/test/.codex/sessions/thread-main.jsonl"),
                Some("/Users/test/project"),
                Some("vscode"),
                Some("Claude Code"),
                None,
            ),
            "claude-code"
        );
    }

    #[test]
    fn infers_zed_client_from_zed_acp_metadata() {
        assert_eq!(
            infer_assistant_client_from_paths(
                Some("/Users/test/.zed/sessions/thread-main.jsonl"),
                Some("/Users/test/project"),
                Some("zed-agent-servers"),
                Some("Zed ACP"),
                None,
            ),
            "zed"
        );
    }

    #[test]
    fn assistant_surface_filter_hides_cross_surface_sessions() {
        assert!(session_matches_assistant_surface(
            Some("/Users/test/.grok/sessions/thread.jsonl"),
            None,
            None,
            None,
            None,
            "grok-build",
        ));
        assert!(!session_matches_assistant_surface(
            Some("/Users/test/.grok/sessions/thread.jsonl"),
            None,
            None,
            None,
            None,
            "codex",
        ));
        assert!(session_matches_assistant_surface(
            Some("/Users/test/.codex/sessions/thread-main.jsonl"),
            None,
            None,
            Some("Codex Desktop"),
            None,
            "codex",
        ));
        assert!(session_matches_assistant_surface(
            Some("/Users/test/.codex/sessions/claude-thread.jsonl"),
            Some("/Users/test/project"),
            Some("vscode"),
            Some("Claude Code"),
            None,
            "claude-code",
        ));
        assert!(!session_matches_assistant_surface(
            Some("/Users/test/.codex/sessions/claude-thread.jsonl"),
            Some("/Users/test/project"),
            Some("vscode"),
            Some("Claude Code"),
            None,
            "codex",
        ));
        assert!(!session_matches_assistant_surface(
            Some("/Users/test/.codex/sessions/claude-thread.jsonl"),
            Some("/Users/test/project"),
            Some("vscode"),
            Some("Claude Code"),
            None,
            "devin",
        ));
        assert!(session_matches_assistant_surface(
            Some("/Users/test/.codex/sessions/devin-thread.jsonl"),
            Some("/Users/test/project"),
            Some("vscode"),
            Some("Devin - Next"),
            None,
            "devin",
        ));
        assert!(!session_matches_assistant_surface(
            Some("/Users/test/.codex/sessions/devin-thread.jsonl"),
            Some("/Users/test/project"),
            Some("vscode"),
            Some("Devin - Next"),
            None,
            "codex",
        ));
        assert!(session_matches_assistant_surface(
            Some("/Users/test/.zed/sessions/zed-thread.jsonl"),
            Some("/Users/test/project"),
            Some("zed-agent-servers"),
            Some("Zed ACP"),
            None,
            "zed",
        ));
        assert!(!session_matches_assistant_surface(
            Some("/Users/test/.zed/sessions/zed-thread.jsonl"),
            Some("/Users/test/project"),
            Some("zed-agent-servers"),
            Some("Zed ACP"),
            None,
            "codex",
        ));
    }
}

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

use super::registration::HookConfigStyle;
use crate::entity_id::{public_thread_id_for_claude_session, public_thread_id_for_devin_session};
use crate::mobile::session::{MobileHookPayload, MobileStopDecision};

const CODEX_HOOKS_FILE: &str = "hooks.json";
const CLAUDE_SETTINGS_FILE: &str = "settings.json";
const DEVIN_CONFIG_RELATIVE_PATH: &str = ".config/devin/config.json";
const GROK_HOOKS_RELATIVE_PATH: &str = "hooks/looper.json";
const LOOPER_CLAUDE_HOOK_ENV: &str = "LOOPER_CLAUDE_HOOK";
const LOOPER_CLAUDE_HOOK_VALUE: &str = "1";
const LOOPER_DEVIN_HOOK_ENV: &str = "LOOPER_DEVIN_HOOK";
const LOOPER_DEVIN_HOOK_VALUE: &str = "1";
const GROK_HOOK_EVENT_ENV: &str = "GROK_HOOK_EVENT";
const GROK_SESSION_ID_ENV: &str = "GROK_SESSION_ID";
const GROK_WORKSPACE_ROOT_ENV: &str = "GROK_WORKSPACE_ROOT";
const SESSION_HOOK_TIMEOUT_SECONDS: u64 = 30;
const STOP_HOOK_TIMEOUT_SECONDS: u64 = 86_400;
const PROMPT_HOOK_TIMEOUT_SECONDS: u64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Homes {
    pub home_path: PathBuf,
    pub codex_home: PathBuf,
    pub claude_home: PathBuf,
    pub grok_home: PathBuf,
}

impl Homes {
    pub fn new(
        home_path: PathBuf,
        codex_home: PathBuf,
        claude_home: PathBuf,
        grok_home: PathBuf,
    ) -> Self {
        Self {
            home_path,
            codex_home,
            claude_home,
            grok_home,
        }
    }

    pub fn for_codex_home(codex_home: &Path) -> Self {
        Self::new(
            codex_home
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
            codex_home.to_path_buf(),
            PathBuf::from("."),
            PathBuf::from("."),
        )
    }

    pub fn for_claude_home(claude_home: &Path) -> Self {
        Self::new(
            claude_home
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
            PathBuf::from("."),
            claude_home.to_path_buf(),
            PathBuf::from("."),
        )
    }

    pub fn for_home_path(home_path: &Path) -> Self {
        Self::new(
            home_path.to_path_buf(),
            PathBuf::from("."),
            PathBuf::from("."),
            PathBuf::from("."),
        )
    }

    pub fn for_grok_home(grok_home: &Path) -> Self {
        Self::new(
            grok_home
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from(".")),
            PathBuf::from("."),
            PathBuf::from("."),
            grok_home.to_path_buf(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookRegistrationSpec {
    pub config_path: PathBuf,
    pub config_style: HookConfigStyle,
    pub env_marker: Option<&'static str>,
    pub stop_timeout_secs: u64,
    pub session_timeout_secs: u64,
    pub prompt_timeout_secs: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopDelivery {
    Stdout(MobileStopDecision),
    SpawnContinue { prompt: String },
}

pub trait HookAdapter: Sync {
    fn spec(&self, homes: &Homes) -> HookRegistrationSpec;
    fn parse_payload(&self, stdin_json: &Value) -> Option<MobileHookPayload>;
    fn deliver_stop_decision(&self, decision: &MobileStopDecision) -> StopDelivery;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookAdapterKind {
    Codex,
    Claude,
    Devin,
    Grok,
}

static CODEX_ADAPTER: CodexHookAdapter = CodexHookAdapter;
static CLAUDE_ADAPTER: ClaudeHookAdapter = ClaudeHookAdapter;
static DEVIN_ADAPTER: DevinHookAdapter = DevinHookAdapter;
static GROK_ADAPTER: GrokHookAdapter = GrokHookAdapter;

impl HookAdapterKind {
    pub fn from_environment() -> Self {
        if is_devin_hook_invocation() {
            Self::Devin
        } else if is_claude_hook_invocation() {
            Self::Claude
        } else if is_grok_hook_invocation() {
            Self::Grok
        } else {
            Self::Codex
        }
    }

    pub fn adapter(self) -> &'static dyn HookAdapter {
        match self {
            Self::Codex => &CODEX_ADAPTER,
            Self::Claude => &CLAUDE_ADAPTER,
            Self::Devin => &DEVIN_ADAPTER,
            Self::Grok => &GROK_ADAPTER,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CodexHookAdapter;

#[derive(Clone, Copy, Debug)]
pub struct ClaudeHookAdapter;

#[derive(Clone, Copy, Debug)]
pub struct DevinHookAdapter;

#[derive(Clone, Copy, Debug)]
pub struct GrokHookAdapter;

impl HookAdapter for CodexHookAdapter {
    fn spec(&self, homes: &Homes) -> HookRegistrationSpec {
        HookRegistrationSpec {
            config_path: homes.codex_home.join(CODEX_HOOKS_FILE),
            config_style: HookConfigStyle::CodexHooksJson,
            env_marker: None,
            stop_timeout_secs: STOP_HOOK_TIMEOUT_SECONDS,
            session_timeout_secs: SESSION_HOOK_TIMEOUT_SECONDS,
            prompt_timeout_secs: PROMPT_HOOK_TIMEOUT_SECONDS,
        }
    }

    fn parse_payload(&self, stdin_json: &Value) -> Option<MobileHookPayload> {
        if stdin_json.is_null() {
            return Some(empty_hook_payload());
        }
        serde_json::from_value(stdin_json.clone()).ok()
    }

    fn deliver_stop_decision(&self, decision: &MobileStopDecision) -> StopDelivery {
        StopDelivery::Stdout(decision.clone())
    }
}

impl HookAdapter for ClaudeHookAdapter {
    fn spec(&self, homes: &Homes) -> HookRegistrationSpec {
        HookRegistrationSpec {
            config_path: homes.claude_home.join(CLAUDE_SETTINGS_FILE),
            config_style: HookConfigStyle::ClaudeSettings,
            env_marker: Some("LOOPER_CLAUDE_HOOK=1"),
            stop_timeout_secs: STOP_HOOK_TIMEOUT_SECONDS,
            session_timeout_secs: SESSION_HOOK_TIMEOUT_SECONDS,
            prompt_timeout_secs: PROMPT_HOOK_TIMEOUT_SECONDS,
        }
    }

    fn parse_payload(&self, stdin_json: &Value) -> Option<MobileHookPayload> {
        Some(claude_payload_from_value(stdin_json))
    }

    fn deliver_stop_decision(&self, decision: &MobileStopDecision) -> StopDelivery {
        StopDelivery::Stdout(decision.clone())
    }
}

impl HookAdapter for DevinHookAdapter {
    fn spec(&self, homes: &Homes) -> HookRegistrationSpec {
        HookRegistrationSpec {
            config_path: homes.home_path.join(DEVIN_CONFIG_RELATIVE_PATH),
            config_style: HookConfigStyle::DevinConfig,
            env_marker: Some("LOOPER_DEVIN_HOOK=1"),
            stop_timeout_secs: STOP_HOOK_TIMEOUT_SECONDS,
            session_timeout_secs: SESSION_HOOK_TIMEOUT_SECONDS,
            prompt_timeout_secs: PROMPT_HOOK_TIMEOUT_SECONDS,
        }
    }

    fn parse_payload(&self, stdin_json: &Value) -> Option<MobileHookPayload> {
        Some(devin_payload_from_value(stdin_json))
    }

    fn deliver_stop_decision(&self, decision: &MobileStopDecision) -> StopDelivery {
        StopDelivery::Stdout(decision.clone())
    }
}

impl HookAdapter for GrokHookAdapter {
    fn spec(&self, homes: &Homes) -> HookRegistrationSpec {
        HookRegistrationSpec {
            config_path: homes.grok_home.join(GROK_HOOKS_RELATIVE_PATH),
            config_style: HookConfigStyle::GrokDir,
            env_marker: None,
            stop_timeout_secs: STOP_HOOK_TIMEOUT_SECONDS,
            session_timeout_secs: SESSION_HOOK_TIMEOUT_SECONDS,
            prompt_timeout_secs: PROMPT_HOOK_TIMEOUT_SECONDS,
        }
    }

    fn parse_payload(&self, stdin_json: &Value) -> Option<MobileHookPayload> {
        Some(grok_payload_from_value(stdin_json))
    }

    fn deliver_stop_decision(&self, decision: &MobileStopDecision) -> StopDelivery {
        if decision.decision == "block" {
            StopDelivery::SpawnContinue {
                prompt: decision.reason.clone(),
            }
        } else {
            StopDelivery::Stdout(decision.clone())
        }
    }
}

pub fn default_claude_settings_path(claude_home: &Path) -> PathBuf {
    ClaudeHookAdapter
        .spec(&Homes::for_claude_home(claude_home))
        .config_path
}

pub fn default_devin_config_path(home_path: &Path) -> PathBuf {
    DevinHookAdapter
        .spec(&Homes::for_home_path(home_path))
        .config_path
}

pub fn default_grok_hooks_path(grok_home: &Path) -> PathBuf {
    GrokHookAdapter
        .spec(&Homes::for_grok_home(grok_home))
        .config_path
}

pub fn is_claude_hook_invocation() -> bool {
    std::env::var(LOOPER_CLAUDE_HOOK_ENV).as_deref() == Ok(LOOPER_CLAUDE_HOOK_VALUE)
}

pub fn is_devin_hook_invocation() -> bool {
    std::env::var(LOOPER_DEVIN_HOOK_ENV).as_deref() == Ok(LOOPER_DEVIN_HOOK_VALUE)
}

pub fn is_grok_hook_invocation() -> bool {
    std::env::var(GROK_HOOK_EVENT_ENV).is_ok()
}

pub fn parse_input_json(input: &str, context: &'static str) -> Result<Value> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(trimmed).context(context)
}

pub fn parse_codex_hook_payload(input: &str) -> Result<MobileHookPayload> {
    let value = parse_input_json(input, "parse Codex hook payload JSON from stdin")?;
    Ok(CodexHookAdapter
        .parse_payload(&value)
        .unwrap_or_else(empty_hook_payload))
}

pub fn parse_claude_hook_payload(input: &str) -> Result<MobileHookPayload> {
    let value = parse_input_json(input, "parse Claude Code hook payload JSON from stdin")?;
    Ok(ClaudeHookAdapter
        .parse_payload(&value)
        .unwrap_or_else(empty_hook_payload))
}

pub fn parse_devin_hook_payload(input: &str) -> Result<MobileHookPayload> {
    let value = parse_input_json(input, "parse Devin hook payload JSON from stdin")?;
    Ok(DevinHookAdapter
        .parse_payload(&value)
        .unwrap_or_else(empty_hook_payload))
}

pub fn parse_grok_hook_payload(input: &str) -> Result<MobileHookPayload> {
    let value = parse_input_json(input, "parse hook payload JSON from stdin")?;
    if !is_grok_hook_payload(&value) && !value.is_null() {
        return serde_json::from_value(value).context("decode Codex hook payload");
    }
    Ok(GrokHookAdapter
        .parse_payload(&value)
        .unwrap_or_else(empty_hook_payload))
}

pub fn empty_hook_payload() -> MobileHookPayload {
    MobileHookPayload {
        hook_event_name: String::new(),
        session_id: None,
        turn_id: None,
        cwd: None,
        last_assistant_message: None,
    }
}

fn claude_payload_from_value(value: &Value) -> MobileHookPayload {
    if value.is_null() {
        return empty_hook_payload();
    }
    let raw_session_id = first_string(value, &["session_id", "sessionId"]);
    MobileHookPayload {
        hook_event_name: first_string(value, &["hook_event_name", "hookEventName"])
            .map(|name| normalize_common_event_name(&name))
            .unwrap_or_default(),
        session_id: raw_session_id
            .or_else(|| session_id_from_transcript_path(value))
            .map(|session_id| public_thread_id_for_claude_session(&session_id)),
        turn_id: first_string(value, &["turn_id", "turnId"]),
        cwd: first_string(value, &["cwd", "workspaceRoot", "workspace_root"]),
        last_assistant_message: first_string(
            value,
            &["last_assistant_message", "lastAssistantMessage"],
        ),
    }
}

fn devin_payload_from_value(value: &Value) -> MobileHookPayload {
    if value.is_null() {
        return empty_hook_payload();
    }
    let raw_session_id = first_string(value, &["session_id", "sessionId"]);
    MobileHookPayload {
        hook_event_name: first_string(value, &["hook_event_name", "hookEventName"])
            .map(|name| normalize_common_event_name(&name))
            .unwrap_or_default(),
        session_id: raw_session_id
            .map(|session_id| public_thread_id_for_devin_session(&session_id)),
        turn_id: first_string(value, &["turn_id", "turnId"]),
        cwd: first_string(value, &["cwd", "workspaceRoot", "workspace_root"]),
        last_assistant_message: first_string(
            value,
            &["last_assistant_message", "lastAssistantMessage"],
        ),
    }
}

fn grok_payload_from_value(value: &Value) -> MobileHookPayload {
    if value.is_null() {
        return MobileHookPayload {
            hook_event_name: String::new(),
            session_id: std::env::var(GROK_SESSION_ID_ENV).ok(),
            turn_id: None,
            cwd: std::env::var(GROK_WORKSPACE_ROOT_ENV).ok(),
            last_assistant_message: None,
        };
    }
    let hook_event_name = first_string(value, &["hookEventName", "hook_event_name"])
        .or_else(|| std::env::var(GROK_HOOK_EVENT_ENV).ok())
        .map(|name| normalize_grok_event_name(&name))
        .unwrap_or_default();
    let session_id = first_string(value, &["sessionId", "session_id"])
        .or_else(|| std::env::var(GROK_SESSION_ID_ENV).ok());
    let cwd = first_string(value, &["cwd", "workspaceRoot", "workspace_root"])
        .or_else(|| std::env::var(GROK_WORKSPACE_ROOT_ENV).ok());
    let last_assistant_message =
        first_string(value, &["lastAssistantMessage", "last_assistant_message"]);

    MobileHookPayload {
        hook_event_name,
        session_id,
        turn_id: first_string(value, &["turnId", "turn_id"]),
        cwd,
        last_assistant_message,
    }
}

fn is_grok_hook_payload(value: &Value) -> bool {
    if value.is_null() || std::env::var(GROK_HOOK_EVENT_ENV).is_ok() {
        return true;
    }
    value
        .get("hookEventName")
        .and_then(Value::as_str)
        .is_some_and(|name| normalize_grok_event_name(name) != name || name.contains('_'))
}

fn session_id_from_transcript_path(value: &Value) -> Option<String> {
    let transcript_path = first_string(value, &["transcript_path", "transcriptPath"])?;
    Path::new(&transcript_path)
        .file_stem()
        .and_then(|file_stem| file_stem.to_str())
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_owned)
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn normalize_common_event_name(name: &str) -> String {
    match name {
        "stop" | "Stop" => "Stop".to_owned(),
        "session_start" | "SessionStart" => "SessionStart".to_owned(),
        "user_prompt_submit" | "UserPromptSubmit" => "UserPromptSubmit".to_owned(),
        "session_end" | "SessionEnd" => "SessionEnd".to_owned(),
        other => other.to_owned(),
    }
}

fn normalize_grok_event_name(name: &str) -> String {
    match name {
        "notification" | "Notification" => "Notification".to_owned(),
        other if other.contains('_') => {
            let pascal = other
                .split('_')
                .filter(|segment| !segment.is_empty())
                .map(|segment| {
                    let mut chars = segment.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    }
                })
                .collect::<String>();
            if pascal.is_empty() {
                other.to_owned()
            } else {
                pascal
            }
        }
        other => normalize_common_event_name(other),
    }
}

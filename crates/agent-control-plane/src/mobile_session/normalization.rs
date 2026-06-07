use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::presets::is_valid_preset;
use super::{DEFAULT_REMOTE_PROMPT, MobileSessionError, MobileSessionResult};

pub(super) const ENABLED_FLAG: i64 = 1;
pub(super) const DISABLED_FLAG: i64 = 0;
pub(super) const DEFAULT_SCOPE: &str = "global";
pub(super) const DEFAULT_ASSISTANT_SURFACE: &str = "codex";

const LOOP_SCOPES: &[&str] = &["global", "per-task"];
const ASSISTANT_SURFACES: &[&str] = &["codex", "devin", "grok-build"];

pub(super) fn normalized_preset(preset: Option<&str>) -> MobileSessionResult<Option<String>> {
    let Some(preset) = preset.and_then(normalized_optional) else {
        return Ok(None);
    };
    if is_valid_preset(&preset) {
        return Ok(Some(preset));
    }

    Err(MobileSessionError::InvalidPreset)
}

pub(super) fn normalized_scope(scope: &str) -> MobileSessionResult<String> {
    let Some(scope) = normalized_optional(scope) else {
        return Ok(DEFAULT_SCOPE.to_owned());
    };
    if LOOP_SCOPES.contains(&scope.as_str()) {
        return Ok(scope);
    }

    Err(MobileSessionError::InvalidScope)
}

pub(super) fn normalized_assistant_surface(surface: &str) -> MobileSessionResult<String> {
    let Some(surface) = normalized_optional(surface) else {
        return Ok(DEFAULT_ASSISTANT_SURFACE.to_owned());
    };
    if ASSISTANT_SURFACES.contains(&surface.as_str()) {
        return Ok(surface);
    }

    Err(MobileSessionError::InvalidAssistantSurface)
}

pub(super) fn normalized_commands(commands: &[String]) -> MobileSessionResult<Vec<String>> {
    let commands = commands
        .iter()
        .filter_map(|command| normalized_optional(command))
        .collect::<Vec<_>>();
    if commands.is_empty() {
        return Err(MobileSessionError::InvalidCompletionCheck);
    }
    Ok(commands)
}

pub(super) fn parse_commands_json(commands_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(commands_json)
        .map(|commands| {
            commands
                .into_iter()
                .filter_map(|command| normalized_optional(&command))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn normalized_required(value: &str) -> Option<String> {
    normalized_optional(value)
}

pub(super) fn normalized_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

pub(super) fn normalized_option(value: &Option<String>) -> Option<String> {
    value.as_deref().and_then(normalized_optional)
}

pub(super) fn normalize_prompt_or_default(prompt: &str) -> String {
    normalized_optional(prompt).unwrap_or_else(|| DEFAULT_REMOTE_PROMPT.to_owned())
}

pub(super) fn bool_to_flag(value: bool) -> i64 {
    if value { ENABLED_FLAG } else { DISABLED_FLAG }
}

pub(super) fn now_iso_string() -> MobileSessionResult<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

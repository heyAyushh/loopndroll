pub(super) const PROMPT_DELIVERY_MODE_ONCE: &str = "once";
pub(super) const PROMPT_DELIVERY_MODE_PERSISTENT: &str = "persistent";

const MAX_TURNS_1: i64 = 1;
const MAX_TURNS_2: i64 = 2;
const MAX_TURNS_3: i64 = 3;
const MOBILE_SESSION_MODE_PRESETS: &[&str] = &[
    "infinite",
    "await-reply",
    "completion-checks",
    "max-turns-1",
    "max-turns-2",
    "max-turns-3",
];

pub(super) fn is_valid_preset(preset: &str) -> bool {
    MOBILE_SESSION_MODE_PRESETS.contains(&preset)
}

pub(super) fn remote_prompt_delivery_mode(preset: &str) -> &'static str {
    if is_persistent_prompt_preset(preset) {
        PROMPT_DELIVERY_MODE_PERSISTENT
    } else {
        PROMPT_DELIVERY_MODE_ONCE
    }
}

pub(super) fn is_persistent_prompt_preset(preset: &str) -> bool {
    matches!(
        preset,
        "infinite" | "max-turns-1" | "max-turns-2" | "max-turns-3"
    )
}

pub(super) fn max_turns_for_preset(preset: &str) -> Option<i64> {
    match preset {
        "max-turns-1" => Some(MAX_TURNS_1),
        "max-turns-2" => Some(MAX_TURNS_2),
        "max-turns-3" => Some(MAX_TURNS_3),
        _ => None,
    }
}

pub(super) fn render_prompt(template: &str, remaining_turns: Option<i64>) -> String {
    template
        .replace(
            "{{remaining_turns}}",
            remaining_turns
                .map(|value| value.to_string())
                .as_deref()
                .unwrap_or_default(),
        )
        .trim()
        .to_owned()
}

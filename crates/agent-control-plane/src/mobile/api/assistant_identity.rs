use crate::assistant::{
    AssistantKind, assistant_client_matches_surface, infer_assistant_client_from_paths,
};
use crate::control_plane::DesktopThread;

pub(super) const CODEX_SOURCE_LABEL: &str = "Codex";
pub(super) const CLAUDE_SOURCE_LABEL: &str = "Claude Code";
pub(super) const DEVIN_SOURCE_LABEL: &str = "Devin";
pub(super) const GROK_BUILD_SOURCE_LABEL: &str = "Grok Build";
pub(super) const ZED_SOURCE_LABEL: &str = "Zed";

pub(super) const CODEX_ASSISTANT_CLIENT: &str = "codex";
pub(super) const DEVIN_ASSISTANT_CLIENT: &str = "devin";
pub(super) const GROK_BUILD_ASSISTANT_CLIENT: &str = "grok-build";
pub(super) const CLAUDE_CODE_ASSISTANT_CLIENT: &str = "claude-code";
pub(super) const CURSOR_ASSISTANT_CLIENT: &str = "cursor";
pub(super) const OPENCLAW_ASSISTANT_CLIENT: &str = "openclaw";
pub(super) const SUPER_ENGINEERING_ASSISTANT_CLIENT: &str = "super-engineering";
pub(super) const ZED_ASSISTANT_CLIENT: &str = "zed";

pub(super) fn thread_matches_assistant_surface(thread: &DesktopThread, surface: &str) -> bool {
    assistant_client_matches_surface(assistant_client_for_thread(thread), surface)
}

pub(super) fn assistant_client_for_thread(thread: &DesktopThread) -> &'static str {
    match thread.capabilities.assistant_kind {
        AssistantKind::Codex => CODEX_ASSISTANT_CLIENT,
        AssistantKind::DevinDesktop => DEVIN_ASSISTANT_CLIENT,
        AssistantKind::GrokBuild => GROK_BUILD_ASSISTANT_CLIENT,
        AssistantKind::ClaudeCode => CLAUDE_CODE_ASSISTANT_CLIENT,
        AssistantKind::Cursor => CURSOR_ASSISTANT_CLIENT,
        AssistantKind::OpenClaw => OPENCLAW_ASSISTANT_CLIENT,
        AssistantKind::Superconductor => SUPER_ENGINEERING_ASSISTANT_CLIENT,
        AssistantKind::Zed => ZED_ASSISTANT_CLIENT,
        AssistantKind::Unknown => infer_assistant_client_from_paths(
            thread.transcript_path.as_deref(),
            thread.cwd.as_deref(),
            thread.source.as_deref(),
            thread.originator.as_deref(),
            thread.agent_path.as_deref(),
        ),
        _ => CODEX_ASSISTANT_CLIENT,
    }
}

pub(super) fn source_display_name(assistant_client: &str) -> &'static str {
    match assistant_client {
        CLAUDE_CODE_ASSISTANT_CLIENT => CLAUDE_SOURCE_LABEL,
        DEVIN_ASSISTANT_CLIENT => DEVIN_SOURCE_LABEL,
        GROK_BUILD_ASSISTANT_CLIENT => GROK_BUILD_SOURCE_LABEL,
        ZED_ASSISTANT_CLIENT => ZED_SOURCE_LABEL,
        _ => CODEX_SOURCE_LABEL,
    }
}

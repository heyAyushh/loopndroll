use crate::assistant::AssistantKind;
use crate::control_plane::DesktopThread;
use crate::devin::DevinPromptTransport;

use super::summary::ACTIVE_SESSION_STATUS;
use super::transport::{devin_prompt_transport, zed_acp_session_id};

const ARCHIVED_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "Archived sessions cannot receive prompts.";
pub(super) const DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "This Devin provider does not support mobile prompt delivery yet.";
pub(super) const DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON: &str =
    "This Devin Local session must be running before Looper can deliver prompts through hooks.";
pub(super) const INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "This session must be running before Looper can queue prompts.";
const UNSUPPORTED_PROMPT_DELIVERY_UNAVAILABLE_REASON: &str =
    "This assistant does not support mobile prompt delivery yet.";

pub(super) struct PromptDeliveryAvailability {
    pub(super) can_send_prompt: bool,
    pub(super) unavailable_reason: Option<&'static str>,
}

pub(super) fn assistant_supports_prompt_delivery(assistant_kind: &AssistantKind) -> bool {
    matches!(
        assistant_kind,
        AssistantKind::Codex
            | AssistantKind::DevinDesktop
            | AssistantKind::Zed
            | AssistantKind::GrokBuild
            | AssistantKind::ClaudeCode
    )
}

pub(super) fn prompt_delivery_availability(
    thread: &DesktopThread,
    is_archived: bool,
    status: &str,
) -> PromptDeliveryAvailability {
    if is_archived {
        return PromptDeliveryAvailability {
            can_send_prompt: false,
            unavailable_reason: Some(ARCHIVED_PROMPT_DELIVERY_UNAVAILABLE_REASON),
        };
    }

    if !assistant_supports_prompt_delivery(&thread.capabilities.assistant_kind) {
        return PromptDeliveryAvailability {
            can_send_prompt: false,
            unavailable_reason: Some(UNSUPPORTED_PROMPT_DELIVERY_UNAVAILABLE_REASON),
        };
    }
    if thread.capabilities.assistant_kind == AssistantKind::DevinDesktop {
        let Some((transport, _session_id)) = devin_prompt_transport(thread) else {
            return PromptDeliveryAvailability {
                can_send_prompt: false,
                unavailable_reason: Some(DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON),
            };
        };
        match transport {
            DevinPromptTransport::DevinAcpBridge => {}
            DevinPromptTransport::DevinHook if status != ACTIVE_SESSION_STATUS => {
                return PromptDeliveryAvailability {
                    can_send_prompt: false,
                    unavailable_reason: Some(
                        DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON,
                    ),
                };
            }
            DevinPromptTransport::CodexAppServer | DevinPromptTransport::DevinHook => {}
        }
    }
    if thread.capabilities.assistant_kind == AssistantKind::Zed {
        let has_live_zed_transport =
            zed_acp_session_id(thread).is_some() && status == ACTIVE_SESSION_STATUS;
        return PromptDeliveryAvailability {
            can_send_prompt: has_live_zed_transport,
            unavailable_reason: (!has_live_zed_transport).then_some(
                if zed_acp_session_id(thread).is_some() {
                    INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON
                } else {
                    UNSUPPORTED_PROMPT_DELIVERY_UNAVAILABLE_REASON
                },
            ),
        };
    }

    let requires_active_session = !matches!(
        thread.capabilities.assistant_kind,
        AssistantKind::Codex | AssistantKind::DevinDesktop | AssistantKind::ClaudeCode
    );
    if requires_active_session && status != ACTIVE_SESSION_STATUS {
        return PromptDeliveryAvailability {
            can_send_prompt: false,
            unavailable_reason: Some(INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON),
        };
    }

    PromptDeliveryAvailability {
        can_send_prompt: true,
        unavailable_reason: None,
    }
}

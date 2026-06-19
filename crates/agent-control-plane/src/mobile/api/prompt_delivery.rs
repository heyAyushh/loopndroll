use crate::assistant::AssistantKind;
use crate::control_plane::{DesktopSnapshot, DesktopThread};
use crate::devin::{
    DevinPromptTransport, DevinThreadIdentity, devin_prompt_transport_for_provider,
    devin_thread_identity_from_public_thread_id,
};
use crate::mobile::session::{MobileSessionError, MobileSessionState};

use super::assistant_identity::thread_matches_assistant_surface;
use super::{ACTIVE_SESSION_STATUS, effective_preset, session_override, session_status};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptResumeTarget {
    pub thread_id: String,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDeliveryAction {
    QueueForHook,
    SendDevinAcp { session_id: String },
    ResumeCodex(PromptResumeTarget),
}

pub fn validate_mobile_prompt_delivery_target(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<(), MobileSessionError> {
    prompt_delivery_action_for_visible_target(snapshot, session_state, thread_id, assistant_surface)
        .map(|_| ())
}

pub fn prompt_delivery_action_for_target(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id)
        .ok_or(MobileSessionError::SessionNotFound)?;

    prompt_delivery_action_for_thread(thread, session_state)
}

pub fn prompt_delivery_action_for_visible_target(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let thread = snapshot
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id)
        .ok_or(MobileSessionError::SessionNotFound)?;

    let visible_surface = assistant_surface.unwrap_or(&session_state.assistant_surface);
    if !thread_matches_assistant_surface(thread, visible_surface) {
        return Err(MobileSessionError::SessionNotFound);
    }

    prompt_delivery_action_for_thread(thread, session_state)
}

pub(super) fn prompt_delivery_action_for_thread(
    thread: &DesktopThread,
    session_state: &MobileSessionState,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let session_override = session_override(thread, session_state);
    if session_override.map(|state| state.deleted).unwrap_or(false) {
        return Err(MobileSessionError::SessionNotFound);
    }
    let is_archived = session_override
        .and_then(|state| state.archived)
        .unwrap_or(thread.archived);
    if is_archived {
        return Err(MobileSessionError::SessionArchived);
    }
    if !assistant_supports_prompt_delivery(&thread.capabilities.assistant_kind) {
        return Err(MobileSessionError::PromptDeliveryUnavailable);
    }
    if thread.capabilities.assistant_kind == AssistantKind::Codex {
        return Ok(PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
            thread_id: thread.thread_id.clone(),
            cwd: thread.cwd.clone(),
        }));
    }
    let effective_mode = effective_preset(session_override, session_state);
    let lifecycle = session_state.lifecycle.get(&thread.thread_id);
    let status = session_status(
        is_archived,
        effective_mode,
        lifecycle,
        thread.runtime_status.as_deref(),
    );
    if thread.capabilities.assistant_kind == AssistantKind::DevinDesktop {
        return devin_prompt_delivery_action(thread, &status);
    }

    match status {
        ACTIVE_SESSION_STATUS => Ok(PromptDeliveryAction::QueueForHook),
        _ => Err(MobileSessionError::PromptDeliveryUnavailableReason(
            INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON.to_owned(),
        )),
    }
}

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

fn assistant_supports_prompt_delivery(assistant_kind: &AssistantKind) -> bool {
    matches!(
        assistant_kind,
        AssistantKind::Codex
            | AssistantKind::DevinDesktop
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

    let requires_active_session = !matches!(
        thread.capabilities.assistant_kind,
        AssistantKind::Codex | AssistantKind::DevinDesktop
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

fn devin_prompt_transport(thread: &DesktopThread) -> Option<(DevinPromptTransport, String)> {
    let DevinThreadIdentity {
        provider_id,
        session_id,
    } = devin_thread_identity_from_public_thread_id(&thread.thread_id)?;
    let transport = devin_prompt_transport_for_provider(&provider_id)?;
    Some((transport, session_id))
}

fn devin_prompt_delivery_action(
    thread: &DesktopThread,
    status: &str,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let DevinThreadIdentity {
        provider_id,
        session_id,
    } = devin_thread_identity_from_public_thread_id(&thread.thread_id)
        .ok_or(MobileSessionError::PromptDeliveryUnavailable)?;
    let transport = devin_prompt_transport_for_provider(&provider_id).ok_or_else(|| {
        MobileSessionError::PromptDeliveryUnavailableReason(
            DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON.to_owned(),
        )
    })?;
    match transport {
        DevinPromptTransport::CodexAppServer => {
            Ok(PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
                thread_id: session_id,
                cwd: thread.cwd.clone(),
            }))
        }
        DevinPromptTransport::DevinAcpBridge => Ok(PromptDeliveryAction::SendDevinAcp {
            session_id: format!("acp/{provider_id}/{session_id}"),
        }),
        DevinPromptTransport::DevinHook if status == ACTIVE_SESSION_STATUS => {
            Ok(PromptDeliveryAction::QueueForHook)
        }
        DevinPromptTransport::DevinHook => {
            Err(MobileSessionError::PromptDeliveryUnavailableReason(
                DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON.to_owned(),
            ))
        }
    }
}

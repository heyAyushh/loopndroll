use crate::acp::client_host::DEVIN_ACP_CLIENT_HOST_ID;
use crate::assistant::AssistantKind;
use crate::claude_code::claude_session_id_from_public_thread_id;
use crate::control_plane::{DesktopSnapshot, DesktopThread};
use crate::devin::{
    DevinPromptTransport, DevinThreadIdentity, devin_prompt_transport_for_provider,
    devin_thread_identity_from_public_thread_id,
};
use crate::mobile::session::{MobileSessionError, MobileSessionState};
use crate::zed::ZED_CLIENT_ID;

use super::assistant_identity::thread_matches_assistant_surface;
use super::availability::{
    DEVIN_HOOK_PROMPT_DELIVERY_REQUIRES_ACTIVE_SESSION_REASON,
    DEVIN_PROVIDER_PROMPT_DELIVERY_UNAVAILABLE_REASON, INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON,
    assistant_supports_prompt_delivery,
};
use super::overrides::{effective_preset, session_override};
use super::summary::{ACTIVE_SESSION_STATUS, session_status};
use super::transport::zed_acp_session_id;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptResumeTarget {
    pub thread_id: String,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDeliveryAction {
    QueueForHook,
    SendAcp {
        client_id: String,
        session_id: String,
    },
    ResumeCodex(PromptResumeTarget),
    ResumeClaude(PromptResumeTarget),
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
        return devin_prompt_delivery_action(thread, status);
    }
    if thread.capabilities.assistant_kind == AssistantKind::Zed {
        if status != ACTIVE_SESSION_STATUS {
            return Err(MobileSessionError::PromptDeliveryUnavailableReason(
                INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON.to_owned(),
            ));
        }
        return zed_prompt_delivery_action(thread);
    }
    if thread.capabilities.assistant_kind == AssistantKind::ClaudeCode {
        if status == ACTIVE_SESSION_STATUS {
            return Ok(PromptDeliveryAction::QueueForHook);
        }
        return Ok(PromptDeliveryAction::ResumeClaude(PromptResumeTarget {
            thread_id: claude_session_id_from_public_thread_id(&thread.thread_id),
            cwd: thread.cwd.clone(),
        }));
    }

    match status {
        ACTIVE_SESSION_STATUS => Ok(PromptDeliveryAction::QueueForHook),
        _ => Err(MobileSessionError::PromptDeliveryUnavailableReason(
            INACTIVE_PROMPT_DELIVERY_UNAVAILABLE_REASON.to_owned(),
        )),
    }
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
        DevinPromptTransport::DevinAcpBridge => Ok(PromptDeliveryAction::SendAcp {
            client_id: DEVIN_ACP_CLIENT_HOST_ID.to_owned(),
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

fn zed_prompt_delivery_action(
    thread: &DesktopThread,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let session_id =
        zed_acp_session_id(thread).ok_or(MobileSessionError::PromptDeliveryUnavailable)?;
    Ok(PromptDeliveryAction::SendAcp {
        client_id: ZED_CLIENT_ID.to_owned(),
        session_id,
    })
}

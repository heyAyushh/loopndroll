use std::collections::BTreeSet;

use serde::Serialize;

use crate::codex_resume::{CodexResumeRequest, spawn_thread_resume};
use crate::control_plane::{ControlPlane, DesktopSnapshot};
use crate::mobile_api::{
    PromptDeliveryAction, prompt_delivery_action_for_target,
    prompt_delivery_action_for_visible_target,
};
use crate::mobile_events::{MobileEventInput, MobileEventKind};
use crate::mobile_session::MobileSessionError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDispatch {
    Delivered { prompt_id: String },
    Queued { prompt_id: String },
    Resumed,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BatchPromptResponse {
    pub prompted: usize,
    pub thread_ids: Vec<String>,
    pub prompt_ids: Vec<String>,
    pub resumed_thread_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchPromptInput {
    pub thread_ids: Vec<String>,
    pub prompt: String,
    pub preset: Option<String>,
}

pub fn send_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
) -> Result<PromptDispatch, MobileSessionError> {
    let dispatch = dispatch_session_prompt(control_plane, thread_id, assistant_surface, prompt)?;
    emit_prompt_dispatch(control_plane, thread_id, &dispatch);
    Ok(dispatch)
}

pub fn send_non_acp_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    prompt: &str,
) -> Result<PromptDispatch, MobileSessionError> {
    let prompt = required_prompt(prompt)?;
    let snapshot = mobile_desktop_snapshot(control_plane)
        .map_err(|error| MobileSessionError::PromptSnapshotUnavailable(error.to_string()))?;
    let session_state = control_plane.mobile_session_service().state()?;
    let action = prompt_delivery_action_for_target(&snapshot, &session_state, thread_id)?;
    if matches!(action, PromptDeliveryAction::SendDevinAcp { .. }) {
        return Err(MobileSessionError::PromptResumeUnavailable(
            "automation prompt direct ACP delivery is unsupported".to_owned(),
        ));
    }
    let dispatch = dispatch_session_prompt_with_action(control_plane, thread_id, &prompt, action)?;
    emit_prompt_dispatch(control_plane, thread_id, &dispatch);
    Ok(dispatch)
}

pub fn queue_desktop_batch_prompt(
    control_plane: &ControlPlane,
    input: BatchPromptInput,
) -> Result<BatchPromptResponse, MobileSessionError> {
    let prompt = required_prompt(&input.prompt)?;
    let thread_ids = unique_thread_ids(input.thread_ids);
    if thread_ids.is_empty() {
        return Err(MobileSessionError::SessionNotFound);
    }

    let snapshot = mobile_desktop_snapshot(control_plane)
        .map_err(|error| MobileSessionError::PromptSnapshotUnavailable(error.to_string()))?;
    let session_state = control_plane.mobile_session_service().state()?;
    let actions = thread_ids
        .iter()
        .map(|thread_id| prompt_delivery_action_for_target(&snapshot, &session_state, thread_id))
        .collect::<Result<Vec<_>, _>>()?;

    let session_service = control_plane.mobile_session_service();
    let mut prompt_ids = Vec::with_capacity(thread_ids.len());
    let mut resumed_thread_ids = Vec::new();
    for (thread_id, action) in thread_ids.iter().zip(actions) {
        if let Some(preset) = input.preset.as_deref() {
            session_service.set_session_preset(thread_id, Some(preset))?;
        }
        let dispatch =
            dispatch_session_prompt_with_action(control_plane, thread_id, &prompt, action)?;
        emit_prompt_dispatch(control_plane, thread_id, &dispatch);
        match dispatch {
            PromptDispatch::Delivered { prompt_id } => prompt_ids.push(prompt_id),
            PromptDispatch::Queued { prompt_id } => prompt_ids.push(prompt_id),
            PromptDispatch::Resumed => resumed_thread_ids.push(thread_id.clone()),
        }
    }

    Ok(BatchPromptResponse {
        prompted: thread_ids.len(),
        thread_ids,
        prompt_ids,
        resumed_thread_ids,
    })
}

pub fn mobile_desktop_snapshot(control_plane: &ControlPlane) -> anyhow::Result<DesktopSnapshot> {
    control_plane.desktop_menu_snapshot()
}

fn dispatch_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
) -> Result<PromptDispatch, MobileSessionError> {
    let prompt = required_prompt(prompt)?;
    let snapshot = mobile_desktop_snapshot(control_plane)
        .map_err(|error| MobileSessionError::PromptSnapshotUnavailable(error.to_string()))?;
    let session_state = control_plane.mobile_session_service().state()?;
    let action = match assistant_surface {
        Some(surface) => prompt_delivery_action_for_visible_target(
            &snapshot,
            &session_state,
            thread_id,
            Some(surface),
        )?,
        None => prompt_delivery_action_for_target(&snapshot, &session_state, thread_id)?,
    };
    dispatch_session_prompt_with_action(control_plane, thread_id, &prompt, action)
}

fn dispatch_session_prompt_with_action(
    control_plane: &ControlPlane,
    thread_id: &str,
    prompt: &str,
    action: PromptDeliveryAction,
) -> Result<PromptDispatch, MobileSessionError> {
    match action {
        PromptDeliveryAction::QueueForHook => {
            let prompt = control_plane
                .mobile_session_service()
                .queue_prompt(thread_id, prompt)?;
            Ok(PromptDispatch::Queued {
                prompt_id: prompt.id,
            })
        }
        PromptDeliveryAction::SendDevinAcp { session_id } => {
            let delivered = control_plane
                .devin_acp_runtime()
                .deliver_mobile_prompt(&session_id, prompt)
                .map_err(|error| MobileSessionError::PromptResumeUnavailable(error.to_string()))?;
            Ok(PromptDispatch::Delivered {
                prompt_id: delivered.prompt_id,
            })
        }
        PromptDeliveryAction::ResumeCodex(target) => {
            spawn_thread_resume(&CodexResumeRequest {
                thread_id: target.thread_id,
                prompt: prompt.to_owned(),
                cwd: target.cwd,
                codex_executable: control_plane.codex_executable().map(str::to_owned),
            })
            .map_err(|error| MobileSessionError::PromptResumeUnavailable(error.to_string()))?;
            Ok(PromptDispatch::Resumed)
        }
    }
}

fn required_prompt(prompt: &str) -> Result<String, MobileSessionError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(MobileSessionError::PromptRequired);
    }
    Ok(prompt.to_owned())
}

fn unique_thread_ids(thread_ids: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    thread_ids
        .into_iter()
        .filter_map(|thread_id| {
            let thread_id = thread_id.trim().to_owned();
            (!thread_id.is_empty() && seen.insert(thread_id.clone())).then_some(thread_id)
        })
        .collect()
}

fn emit_prompt_dispatch(control_plane: &ControlPlane, thread_id: &str, dispatch: &PromptDispatch) {
    match dispatch {
        PromptDispatch::Delivered { prompt_id } => {
            control_plane.emit_mobile_event(MobileEventInput {
                kind: MobileEventKind::PromptDelivered,
                thread_id: Some(thread_id.to_owned()),
                prompt_id: Some(prompt_id.to_owned()),
                detail: Some("devin-acp".to_owned()),
            });
            emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-delivered"));
        }
        PromptDispatch::Queued { prompt_id } => {
            emit_mobile_prompt_queued(control_plane, thread_id, prompt_id);
        }
        PromptDispatch::Resumed => {
            emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-resumed"));
        }
    }
}

fn emit_mobile_session_changed(
    control_plane: &ControlPlane,
    thread_id: Option<&str>,
    detail: Option<&str>,
) {
    control_plane.emit_mobile_event(MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: thread_id.map(str::to_owned),
        prompt_id: None,
        detail: detail.map(str::to_owned),
    });
}

fn emit_mobile_prompt_queued(control_plane: &ControlPlane, thread_id: &str, prompt_id: &str) {
    control_plane.emit_mobile_event(MobileEventInput {
        kind: MobileEventKind::PromptQueued,
        thread_id: Some(thread_id.to_owned()),
        prompt_id: Some(prompt_id.to_owned()),
        detail: None,
    });
    emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-queued"));
}

#[cfg(test)]
mod tests {
    use super::unique_thread_ids;

    #[test]
    fn unique_thread_ids_trims_and_preserves_first_seen_order() {
        let thread_ids = unique_thread_ids(vec![
            " first ".to_owned(),
            String::new(),
            "second".to_owned(),
            "first".to_owned(),
            " third".to_owned(),
        ]);

        assert_eq!(thread_ids, vec!["first", "second", "third"]);
    }
}

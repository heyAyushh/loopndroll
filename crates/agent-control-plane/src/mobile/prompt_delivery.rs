use std::collections::{BTreeSet, HashMap};
use std::sync::{Mutex, MutexGuard};
use std::thread;

use serde::Serialize;

use crate::codex_resume::{CodexResumeRequest, spawn_thread_resume};
use crate::control_plane::{ControlPlane, DesktopSnapshot};
use crate::mobile::api::{
    PromptDeliveryAction, prompt_delivery_action_for_target,
    prompt_delivery_action_for_visible_target, session_mini_records_allow_reply_mode_prompt,
};
use crate::mobile::events::{MobileEventInput, MobileEventKind};
use crate::mobile::session::{ASSISTANT_SURFACES, MobileSessionError, MobileSessionState};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PromptDispatch {
    Accepted,
    Delivered { prompt_id: String },
    Queued { prompt_id: String },
    Resumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptIntent {
    Queue,
    Steer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcceptedPromptDelivery {
    pub dispatch: PromptDispatch,
    pub after_ack: Option<PromptDeliveryAfterAck>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PromptDeliveryAfterAck {
    thread_id: String,
    prompt: String,
    action: PromptDeliveryAction,
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

const PROMPT_DELIVERY_WORKER_NAME: &str = "looper-mobile-prompt-delivery";
const DISPATCH_ACCEPTED: &str = "accepted";
const DISPATCH_DELIVERED: &str = "delivered";
const DISPATCH_QUEUED: &str = "queued";
const DISPATCH_RESUMED: &str = "resumed";
const PROMPT_INTENT_QUEUE: &str = "queue";
const PROMPT_INTENT_STEER: &str = "steer";
pub(crate) const DETAIL_PROMPT_DELIVERY_FAILED: &str = "prompt-delivery-failed";
const STEER_UNAVAILABLE_FOR_HOOK_REASON: &str =
    "steering is unavailable for hook-only sessions; choose Queue to send after the current run";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DeliveryActionCacheKey {
    thread_id: String,
    assistant_surface: Option<String>,
}

#[derive(Default)]
pub struct PromptDeliveryActionCache {
    actions: Mutex<HashMap<DeliveryActionCacheKey, PromptDeliveryAction>>,
}

impl PromptDeliveryActionCache {
    fn lock(&self) -> MutexGuard<'_, HashMap<DeliveryActionCacheKey, PromptDeliveryAction>> {
        self.actions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn locked_delivery_action_cache(
    control_plane: &ControlPlane,
) -> MutexGuard<'_, HashMap<DeliveryActionCacheKey, PromptDeliveryAction>> {
    control_plane.prompt_delivery_action_cache().lock()
}

pub fn invalidate_delivery_action_cache(control_plane: &ControlPlane, thread_id: &str) {
    locked_delivery_action_cache(control_plane).retain(|key, _| key.thread_id != thread_id);
}

pub fn prime_delivery_action_cache(
    control_plane: &ControlPlane,
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
) {
    let mut actions = Vec::new();
    for thread in &snapshot.threads {
        if let Ok(action) =
            prompt_delivery_action_for_target(snapshot, session_state, &thread.thread_id)
        {
            actions.push((delivery_action_cache_key(&thread.thread_id, None), action));
        }
        for surface in ASSISTANT_SURFACES {
            if let Ok(action) = prompt_delivery_action_for_visible_target(
                snapshot,
                session_state,
                &thread.thread_id,
                Some(surface),
            ) {
                actions.push((
                    delivery_action_cache_key(&thread.thread_id, Some(surface)),
                    action,
                ));
            }
        }
    }
    let mut cache = locked_delivery_action_cache(control_plane);
    cache.clear();
    cache.extend(actions);
}

fn resolve_delivery_action(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<PromptDeliveryAction, MobileSessionError> {
    let cache_key = delivery_action_cache_key(thread_id, assistant_surface);
    if let Some(cached) = locked_delivery_action_cache(control_plane)
        .get(&cache_key)
        .cloned()
    {
        return Ok(cached);
    }

    // Cold cache means the reconciler simply hasn't run since the last invalidation
    // (e.g. right after SetSessionMode). Rejecting the user's prompt over a cache miss
    // is wrong; compute the action from a fresh snapshot and re-warm the entry.
    let snapshot = control_plane.desktop_mobile_snapshot().map_err(|error| {
        MobileSessionError::PromptSnapshotUnavailable(format!(
            "prompt delivery snapshot unavailable: {error}"
        ))
    })?;
    let session_state = control_plane
        .mobile_session_service()
        .state()
        .map_err(|error| {
            MobileSessionError::PromptSnapshotUnavailable(format!(
                "prompt delivery session state unavailable: {error}"
            ))
        })?;
    let action = match assistant_surface {
        Some(surface) => prompt_delivery_action_for_visible_target(
            &snapshot,
            &session_state,
            thread_id,
            Some(surface),
        )?,
        None => prompt_delivery_action_for_target(&snapshot, &session_state, thread_id)?,
    };
    locked_delivery_action_cache(control_plane).insert(cache_key, action.clone());
    Ok(action)
}

pub fn accept_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
    intent: PromptIntent,
) -> Result<AcceptedPromptDelivery, MobileSessionError> {
    let prompt = required_prompt(prompt)?;
    match intent {
        PromptIntent::Queue => {
            if prompt_is_replyable_from_minis(control_plane, thread_id, assistant_surface)? {
                return Ok(AcceptedPromptDelivery {
                    dispatch: PromptDispatch::Accepted,
                    after_ack: Some(PromptDeliveryAfterAck {
                        thread_id: thread_id.to_owned(),
                        prompt,
                        action: PromptDeliveryAction::QueueForHook,
                    }),
                });
            }
            let action = resolve_delivery_action(control_plane, thread_id, assistant_surface)?;
            Ok(AcceptedPromptDelivery {
                dispatch: PromptDispatch::Accepted,
                after_ack: Some(PromptDeliveryAfterAck {
                    thread_id: thread_id.to_owned(),
                    prompt,
                    action,
                }),
            })
        }
        PromptIntent::Steer => {
            let action = resolve_delivery_action(control_plane, thread_id, assistant_surface)?;
            if matches!(action, PromptDeliveryAction::QueueForHook) {
                return Err(MobileSessionError::PromptDeliveryUnavailableReason(
                    STEER_UNAVAILABLE_FOR_HOOK_REASON.to_owned(),
                ));
            }
            Ok(AcceptedPromptDelivery {
                dispatch: PromptDispatch::Accepted,
                after_ack: Some(PromptDeliveryAfterAck {
                    thread_id: thread_id.to_owned(),
                    prompt,
                    action,
                }),
            })
        }
    }
}

pub fn prompt_intent_from_str(value: &str) -> Result<PromptIntent, MobileSessionError> {
    match value.trim() {
        "" | PROMPT_INTENT_QUEUE => Ok(PromptIntent::Queue),
        PROMPT_INTENT_STEER => Ok(PromptIntent::Steer),
        _ => Err(MobileSessionError::InvalidPromptIntent),
    }
}

fn accept_legacy_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
) -> Result<AcceptedPromptDelivery, MobileSessionError> {
    let prompt = required_prompt(prompt)?;
    if prompt_is_replyable_from_minis(control_plane, thread_id, assistant_surface)? {
        return Ok(AcceptedPromptDelivery {
            dispatch: PromptDispatch::Accepted,
            after_ack: Some(PromptDeliveryAfterAck {
                thread_id: thread_id.to_owned(),
                prompt,
                action: PromptDeliveryAction::QueueForHook,
            }),
        });
    }

    let action = resolve_delivery_action(control_plane, thread_id, assistant_surface)?;
    Ok(AcceptedPromptDelivery {
        dispatch: PromptDispatch::Accepted,
        after_ack: Some(PromptDeliveryAfterAck {
            thread_id: thread_id.to_owned(),
            prompt,
            action,
        }),
    })
}

fn accept_non_acp_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
) -> Result<AcceptedPromptDelivery, MobileSessionError> {
    let accepted_delivery =
        accept_legacy_session_prompt(control_plane, thread_id, assistant_surface, prompt)?;
    if let Some(delivery) = &accepted_delivery.after_ack {
        ensure_non_acp_delivery_action(&delivery.action)?;
    }
    Ok(accepted_delivery)
}

fn ensure_non_acp_delivery_action(action: &PromptDeliveryAction) -> Result<(), MobileSessionError> {
    if matches!(
        action,
        PromptDeliveryAction::SendDevinAcp { .. } | PromptDeliveryAction::SendLooperAcp { .. }
    ) {
        return Err(MobileSessionError::PromptResumeUnavailable(
            "automation prompt direct ACP delivery is unsupported".to_owned(),
        ));
    }
    Ok(())
}

pub fn dispatch_session_prompt_after_ack(
    control_plane: ControlPlane,
    delivery: Option<PromptDeliveryAfterAck>,
) {
    let Some(delivery) = delivery else {
        return;
    };
    let spawn_result = thread::Builder::new()
        .name(PROMPT_DELIVERY_WORKER_NAME.to_owned())
        .spawn(move || {
            let PromptDeliveryAfterAck {
                thread_id,
                prompt,
                action,
            } = delivery;
            match dispatch_session_prompt_with_action(&control_plane, &thread_id, &prompt, action) {
                Ok(dispatch) => emit_prompt_dispatch(&control_plane, &thread_id, &dispatch),
                Err(error) => emit_prompt_delivery_failed(&control_plane, &thread_id, &error),
            }
        });

    if let Err(error) = spawn_result {
        eprintln!("mobile prompt delivery worker failed to start: {error}");
    }
}

pub fn send_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
) -> Result<PromptDispatch, MobileSessionError> {
    let accepted_delivery =
        accept_legacy_session_prompt(control_plane, thread_id, assistant_surface, prompt)?;
    dispatch_session_prompt_now(control_plane, accepted_delivery)
}

fn dispatch_session_prompt_now(
    control_plane: &ControlPlane,
    accepted_delivery: AcceptedPromptDelivery,
) -> Result<PromptDispatch, MobileSessionError> {
    let Some(delivery) = accepted_delivery.after_ack else {
        return Ok(accepted_delivery.dispatch);
    };
    let dispatch = dispatch_session_prompt_with_action(
        control_plane,
        &delivery.thread_id,
        &delivery.prompt,
        delivery.action,
    )?;
    emit_prompt_dispatch(control_plane, &delivery.thread_id, &dispatch);
    Ok(dispatch)
}

fn prompt_is_replyable_from_minis(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Result<bool, MobileSessionError> {
    let records = control_plane
        .store()
        .mobile_session_minis_for_session(thread_id)
        .map_err(|error| MobileSessionError::PromptResumeUnavailable(error.to_string()))?;
    Ok(
        session_mini_records_allow_reply_mode_prompt(&records, thread_id, assistant_surface)
            == Some(true),
    )
}

pub fn send_non_acp_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    prompt: &str,
) -> Result<PromptDispatch, MobileSessionError> {
    let accepted_delivery = accept_non_acp_session_prompt(control_plane, thread_id, None, prompt)?;
    dispatch_session_prompt_now(control_plane, accepted_delivery)
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

    let accepted_deliveries = thread_ids
        .iter()
        .map(|thread_id| accept_non_acp_session_prompt(control_plane, thread_id, None, &prompt))
        .collect::<Result<Vec<_>, _>>()?;

    let session_service = control_plane.mobile_session_service();
    let mut prompt_ids = Vec::with_capacity(thread_ids.len());
    let mut resumed_thread_ids = Vec::new();
    for (thread_id, accepted_delivery) in thread_ids.iter().zip(accepted_deliveries) {
        if let Some(preset) = input.preset.as_deref() {
            session_service.set_session_preset(thread_id, Some(preset))?;
        }
        let dispatch = dispatch_session_prompt_now(control_plane, accepted_delivery)?;
        match dispatch {
            PromptDispatch::Accepted => {}
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
        PromptDeliveryAction::SendLooperAcp {
            client_id,
            session_id,
        } => {
            let runtime = control_plane
                .acp_runtime_for_client(&client_id)
                .ok_or_else(|| {
                    MobileSessionError::PromptResumeUnavailable(format!(
                        "ACP client host is unavailable: {client_id}"
                    ))
                })?;
            let delivered = runtime
                .deliver_mobile_prompt(&session_id, prompt)
                .map_err(|error| MobileSessionError::PromptResumeUnavailable(error.to_string()))?;
            Ok(PromptDispatch::Delivered {
                prompt_id: delivered.prompt_id,
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
            let request = CodexResumeRequest {
                thread_id: target.thread_id,
                prompt: prompt.to_owned(),
                cwd: target.cwd,
                codex_executable: control_plane.codex_executable().map(str::to_owned),
            };
            spawn_thread_resume(&request)
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

pub(crate) fn prompt_dispatch_fields(dispatch: &PromptDispatch) -> (&'static str, String) {
    match dispatch {
        PromptDispatch::Accepted => (DISPATCH_ACCEPTED, String::new()),
        PromptDispatch::Delivered { prompt_id } => (DISPATCH_DELIVERED, prompt_id.clone()),
        PromptDispatch::Queued { prompt_id } => (DISPATCH_QUEUED, prompt_id.clone()),
        PromptDispatch::Resumed => (DISPATCH_RESUMED, String::new()),
    }
}

fn delivery_action_cache_key(
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> DeliveryActionCacheKey {
    DeliveryActionCacheKey {
        thread_id: thread_id.to_owned(),
        assistant_surface: assistant_surface.map(str::to_owned),
    }
}

fn emit_prompt_dispatch(control_plane: &ControlPlane, thread_id: &str, dispatch: &PromptDispatch) {
    match dispatch {
        PromptDispatch::Accepted => {}
        PromptDispatch::Delivered { prompt_id } => {
            control_plane.emit_mobile_session_event(
                MobileEventInput {
                    kind: MobileEventKind::PromptDelivered,
                    thread_id: Some(thread_id.to_owned()),
                    prompt_id: Some(prompt_id.to_owned()),
                    detail: Some("looper-acp".to_owned()),
                },
                thread_id,
            );
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
    if let Some(id) = thread_id {
        invalidate_delivery_action_cache(control_plane, id);
    }
    let input = MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: thread_id.map(str::to_owned),
        prompt_id: None,
        detail: detail.map(str::to_owned),
    };
    match thread_id {
        Some(thread_id) => control_plane.emit_mobile_session_event(input, thread_id),
        None => control_plane.emit_mobile_event(input),
    }
}

fn emit_mobile_prompt_queued(control_plane: &ControlPlane, thread_id: &str, prompt_id: &str) {
    control_plane.emit_mobile_session_event(
        MobileEventInput {
            kind: MobileEventKind::PromptQueued,
            thread_id: Some(thread_id.to_owned()),
            prompt_id: Some(prompt_id.to_owned()),
            detail: None,
        },
        thread_id,
    );
    emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-queued"));
}

fn emit_prompt_delivery_failed(
    control_plane: &ControlPlane,
    thread_id: &str,
    error: &MobileSessionError,
) {
    eprintln!("mobile prompt delivery failed for {thread_id}: {error}");
    emit_mobile_session_changed(
        control_plane,
        Some(thread_id),
        Some(DETAIL_PROMPT_DELIVERY_FAILED),
    );
}

#[cfg(test)]
mod tests {
    use super::{
        delivery_action_cache_key, invalidate_delivery_action_cache, locked_delivery_action_cache,
        send_non_acp_session_prompt, unique_thread_ids,
    };
    use crate::control_plane::{ControlPlane, ControlPlaneConfig, HostEnvironment};
    use crate::mobile::session::MobileSessionError;
    use tempfile::TempDir;

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

    #[test]
    fn delivery_action_cache_hit_returns_without_recomputation() {
        use crate::mobile::api::{PromptDeliveryAction, PromptResumeTarget};

        let temp_dir = TempDir::new().expect("temp dir");
        let control_plane = test_control_plane(&temp_dir);
        let thread_id = "cache-test-thread-unique-adr006";

        // Ensure clean state (other tests in same process might have populated).
        invalidate_delivery_action_cache(&control_plane, thread_id);

        // Manually populate the cache as if a miss had already computed the action.
        let expected_action = PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
            thread_id: thread_id.to_owned(),
            cwd: None,
        });
        locked_delivery_action_cache(&control_plane).insert(
            delivery_action_cache_key(thread_id, None),
            expected_action.clone(),
        );

        let cached = locked_delivery_action_cache(&control_plane)
            .get(&delivery_action_cache_key(thread_id, None))
            .cloned();
        assert_eq!(
            cached,
            Some(expected_action),
            "cache hit must return stored action"
        );

        // Invalidation must clear the entry.
        invalidate_delivery_action_cache(&control_plane, thread_id);
        let after_invalidation = locked_delivery_action_cache(&control_plane)
            .get(&delivery_action_cache_key(thread_id, None))
            .cloned();
        assert!(
            after_invalidation.is_none(),
            "invalidation must remove cached action"
        );
    }

    #[test]
    fn delivery_action_cache_keys_include_assistant_surface() {
        use crate::mobile::api::{PromptDeliveryAction, PromptResumeTarget};

        let temp_dir = TempDir::new().expect("temp dir");
        let control_plane = test_control_plane(&temp_dir);
        let thread_id = "cache-surface-thread";
        let codex_action = PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
            thread_id: "codex-target".to_owned(),
            cwd: None,
        });
        let claude_action = PromptDeliveryAction::QueueForHook;

        let mut cache = locked_delivery_action_cache(&control_plane);
        cache.insert(
            delivery_action_cache_key(thread_id, Some("codex")),
            codex_action.clone(),
        );
        cache.insert(
            delivery_action_cache_key(thread_id, Some("claude")),
            claude_action.clone(),
        );
        drop(cache);

        assert_eq!(
            locked_delivery_action_cache(&control_plane)
                .get(&delivery_action_cache_key(thread_id, Some("codex")))
                .cloned(),
            Some(codex_action)
        );
        assert_eq!(
            locked_delivery_action_cache(&control_plane)
                .get(&delivery_action_cache_key(thread_id, Some("claude")))
                .cloned(),
            Some(claude_action)
        );
        invalidate_delivery_action_cache(&control_plane, thread_id);
        assert!(
            locked_delivery_action_cache(&control_plane)
                .keys()
                .all(|key| key.thread_id != thread_id)
        );
    }

    #[test]
    fn non_acp_prompt_recomputes_cold_delivery_action_from_fresh_snapshot() {
        let temp_dir = TempDir::new().expect("temp dir");
        let control_plane = test_control_plane(&temp_dir);
        let thread_id = "cold-cache-non-acp-prompt";

        // A cold cache no longer rejects the prompt outright; the action is computed
        // from a fresh snapshot. This thread doesn't exist in the snapshot, so the
        // error is about the session, not about the cache.
        invalidate_delivery_action_cache(&control_plane, thread_id);
        let error = send_non_acp_session_prompt(&control_plane, thread_id, "hello")
            .expect_err("unknown thread should fail session resolution, not cache lookup");

        match error {
            MobileSessionError::PromptSnapshotUnavailable(message) => {
                panic!("cold cache should recompute instead of rejecting: {message}")
            }
            _ => {}
        }
    }

    fn test_control_plane(temp_dir: &TempDir) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: temp_dir.path().join(".codex"),
            codex_executable: None,
            store_path: temp_dir.path().join("control-plane.sqlite"),
            hook_command: None,
            host_environment: HostEnvironment::hermetic(temp_dir.path().to_path_buf()),
        })
    }
}

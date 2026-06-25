use std::sync::{Arc, Mutex, MutexGuard};

use crate::error::ClientCoreError;
use crate::model::{
    ClientCommandAck, ClientCommandKind, ClientEndpoint, ClientPendingMutation, ClientStateDelta,
    ClientStateSnapshot, ConnectionPhase, OutboundSessionFrame, OutboundSessionFrameKind,
};
use crate::transport::validate_endpoint_url;

const INITIAL_SEQUENCE: i64 = 0;
const EMPTY_SEQUENCE: i64 = 0;

#[derive(Debug, Default)]
struct ClientCoreState {
    phase: ConnectionPhase,
    endpoint_url: String,
    latest_seq: i64,
    revision: String,
    pending_mutations: Vec<ClientPendingMutation>,
    outbox: Vec<OutboundSessionFrame>,
    last_error: String,
}

#[derive(Debug, uniffi::Object)]
pub struct LooperClientCore {
    state: Mutex<ClientCoreState>,
}

#[uniffi::export]
impl LooperClientCore {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(ClientCoreState {
                latest_seq: INITIAL_SEQUENCE,
                ..ClientCoreState::default()
            }),
        })
    }

    pub fn connect(
        &self,
        endpoints: Vec<ClientEndpoint>,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let endpoint = select_endpoint(&endpoints)?;
        validate_endpoint_url(&endpoint.url)?;

        let mut state = self.lock_state()?;
        state.phase = ConnectionPhase::Ready;
        state.endpoint_url = endpoint.url;
        state.last_error.clear();
        Ok(state.snapshot())
    }

    pub fn mark_reconnecting(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.phase = ConnectionPhase::Reconnecting;
        Ok(state.snapshot())
    }

    pub fn disconnect(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.phase = ConnectionPhase::Disconnected;
        Ok(state.snapshot())
    }

    pub fn resume_after(&self, after_seq: i64) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.outbox.push(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Resume,
            command_kind: ClientCommandKind::Resume,
            thread_id: String::new(),
            preset: String::new(),
            prompt: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            client_mutation_id: String::new(),
            after_seq,
        });
        Ok(state.snapshot())
    }

    pub fn set_mode(
        &self,
        thread_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&preset, ClientCoreError::EmptyPreset)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SetSessionMode,
            thread_id,
            preset,
            prompt: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    pub fn send_prompt(
        &self,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SendSessionPrompt,
            thread_id,
            preset: String::new(),
            prompt,
            assistant_surface,
            notification_id: String::new(),
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    pub fn submit_notification_reply(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&notification_id, ClientCoreError::EmptyNotificationId)?;
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SubmitNotificationReply,
            thread_id,
            preset: String::new(),
            prompt,
            assistant_surface,
            notification_id,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    pub fn apply_command_ack(
        &self,
        ack: ClientCommandAck,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.reconcile_ack(ack);
        Ok(state.snapshot())
    }

    pub fn apply_state_delta(
        &self,
        delta: ClientStateDelta,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.latest_seq = state.latest_seq.max(delta.seq);
        state.revision = delta.revision;
        state.last_error.clear();
        Ok(state.snapshot())
    }

    pub fn snapshot(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(state.snapshot())
    }

    pub fn take_outbox(&self) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let mut state = self.lock_state()?;
        Ok(std::mem::take(&mut state.outbox))
    }
}

impl LooperClientCore {
    fn lock_state(&self) -> Result<MutexGuard<'_, ClientCoreState>, ClientCoreError> {
        self.state
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }
}

impl ClientCoreState {
    fn snapshot(&self) -> ClientStateSnapshot {
        ClientStateSnapshot {
            phase: self.phase,
            endpoint_url: self.endpoint_url.clone(),
            latest_seq: self.latest_seq,
            revision: self.revision.clone(),
            pending_mutations: self.pending_mutations.clone(),
            outbox_depth: self.outbox.len() as u32,
            last_error: self.last_error.clone(),
        }
    }

    fn queue_command(&mut self, frame: OutboundSessionFrame) {
        self.pending_mutations.push(ClientPendingMutation {
            client_mutation_id: frame.client_mutation_id.clone(),
            command_kind: frame.command_kind,
            thread_id: frame.thread_id.clone(),
        });
        self.outbox.push(frame);
        self.last_error.clear();
    }

    fn reconcile_ack(&mut self, ack: ClientCommandAck) {
        let reject_message = if ack.accepted {
            String::new()
        } else {
            reject_message(&ack)
        };
        self.latest_seq = self.latest_seq.max(ack.ack_seq);
        if !ack.revision.is_empty() {
            self.revision = ack.revision;
        }
        self.pending_mutations
            .retain(|mutation| mutation.client_mutation_id != ack.client_mutation_id);

        if ack.accepted {
            self.last_error.clear();
        } else {
            self.last_error = reject_message;
        }
    }
}

fn select_endpoint(endpoints: &[ClientEndpoint]) -> Result<ClientEndpoint, ClientCoreError> {
    let endpoint = endpoints
        .iter()
        .find(|endpoint| endpoint.last_good)
        .or_else(|| endpoints.first())
        .ok_or(ClientCoreError::NoEndpoint)?;
    Ok(endpoint.clone())
}

fn reject_message(ack: &ClientCommandAck) -> String {
    match (ack.error_code.is_empty(), ack.reject_reason.is_empty()) {
        (false, false) => format!("{}: {}", ack.error_code, ack.reject_reason),
        (false, true) => ack.error_code.clone(),
        (true, false) => ack.reject_reason.clone(),
        (true, true) => "command rejected".to_owned(),
    }
}

fn require_present(value: &str, error: ClientCoreError) -> Result<(), ClientCoreError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT_PRIMARY: &str = "http://127.0.0.1:8765";
    const ENDPOINT_LAST_GOOD: &str = "http://100.64.0.2:8765";

    #[test]
    fn connect_prefers_last_good_endpoint() {
        let core = LooperClientCore::new();

        let snapshot = core
            .connect(vec![
                ClientEndpoint {
                    url: ENDPOINT_PRIMARY.to_owned(),
                    last_good: false,
                },
                ClientEndpoint {
                    url: ENDPOINT_LAST_GOOD.to_owned(),
                    last_good: true,
                },
            ])
            .expect("connect");

        assert_eq!(snapshot.phase, ConnectionPhase::Ready);
        assert_eq!(snapshot.endpoint_url, ENDPOINT_LAST_GOOD);
        assert_eq!(snapshot.latest_seq, INITIAL_SEQUENCE);
    }

    #[test]
    fn command_methods_queue_session_frames_and_pending_mutations() {
        let core = LooperClientCore::new();

        let snapshot = core
            .set_mode(
                "thread-1".to_owned(),
                "await-reply".to_owned(),
                "cmid-mode".to_owned(),
            )
            .expect("queue mode");

        assert_eq!(snapshot.outbox_depth, 1);
        assert_eq!(snapshot.pending_mutations.len(), 1);
        assert_eq!(
            snapshot.pending_mutations[0].command_kind,
            ClientCommandKind::SetSessionMode
        );

        let outbox = core.take_outbox().expect("take outbox");
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].frame_kind, OutboundSessionFrameKind::Command);
        assert_eq!(outbox[0].preset, "await-reply");

        let drained = core.snapshot().expect("snapshot");
        assert_eq!(drained.outbox_depth, 0);
        assert_eq!(drained.pending_mutations.len(), 1);
    }

    #[test]
    fn accepted_ack_reconciles_pending_mutation() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let snapshot = core
            .apply_command_ack(ClientCommandAck {
                accepted: true,
                client_mutation_id: "cmid-prompt".to_owned(),
                ack_seq: 42,
                entity_id: "thread-1".to_owned(),
                revision: "rev-42".to_owned(),
                server_time: "2026-06-25T00:00:00Z".to_owned(),
                idempotent_replay: false,
                error_code: String::new(),
                reject_reason: String::new(),
            })
            .expect("ack");

        assert_eq!(snapshot.latest_seq, 42);
        assert_eq!(snapshot.revision, "rev-42");
        assert!(snapshot.pending_mutations.is_empty());
        assert!(snapshot.last_error.is_empty());
    }

    #[test]
    fn rejected_ack_clears_pending_mutation_and_reports_reason() {
        let core = LooperClientCore::new();
        core.submit_notification_reply(
            "notification-1".to_owned(),
            "thread-1".to_owned(),
            "retry".to_owned(),
            "codex".to_owned(),
            "cmid-reply".to_owned(),
        )
        .expect("queue reply");

        let snapshot = core
            .apply_command_ack(ClientCommandAck {
                accepted: false,
                client_mutation_id: "cmid-reply".to_owned(),
                ack_seq: 43,
                entity_id: "thread-1".to_owned(),
                revision: "rev-43".to_owned(),
                server_time: "2026-06-25T00:00:01Z".to_owned(),
                idempotent_replay: false,
                error_code: "illegal_transition".to_owned(),
                reject_reason: "WAIT_REPLY required".to_owned(),
            })
            .expect("ack");

        assert_eq!(snapshot.latest_seq, 43);
        assert!(snapshot.pending_mutations.is_empty());
        assert_eq!(
            snapshot.last_error,
            "illegal_transition: WAIT_REPLY required"
        );
    }

    #[test]
    fn state_delta_advances_replicated_revision() {
        let core = LooperClientCore::new();

        let snapshot = core
            .apply_state_delta(ClientStateDelta {
                seq: 44,
                entity_id: "thread-1".to_owned(),
                kind: "session-mode".to_owned(),
                revision: "rev-44".to_owned(),
                server_time: "2026-06-25T00:00:02Z".to_owned(),
                payload_json: r#"{"mode":"await-reply"}"#.to_owned(),
            })
            .expect("state delta");

        assert_eq!(snapshot.latest_seq, 44);
        assert_eq!(snapshot.revision, "rev-44");
    }

    #[test]
    fn resume_after_queues_resume_frame() {
        let core = LooperClientCore::new();

        let snapshot = core.resume_after(44).expect("queue resume");
        assert_eq!(snapshot.outbox_depth, 1);

        let outbox = core.take_outbox().expect("take outbox");
        assert_eq!(outbox[0].frame_kind, OutboundSessionFrameKind::Resume);
        assert_eq!(outbox[0].command_kind, ClientCommandKind::Resume);
        assert_eq!(outbox[0].after_seq, 44);
    }

    #[test]
    fn empty_mutation_id_is_rejected_before_queueing() {
        let core = LooperClientCore::new();

        let error = core
            .set_mode(
                "thread-1".to_owned(),
                "await-reply".to_owned(),
                String::new(),
            )
            .expect_err("empty mutation id");

        assert_eq!(error, ClientCoreError::EmptyMutationId);
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 0);
    }
}

use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::mpsc;

use crate::error::ClientCoreError;
use crate::model::{
    ClientCommandAck, ClientCommandBatchResponse, ClientCommandKind, ClientEndpoint,
    ClientPendingMutation, ClientStateDelta, ClientStateMini, ClientStateMiniDelta,
    ClientStateMiniDeltaApplyResult, ClientStateMiniSnapshot, ClientStateMiniStreamUpdate,
    ClientStateMiniStreamUpdateReason, ClientStateSnapshot, ConnectionPhase, OutboundSessionFrame,
    OutboundSessionFrameKind,
};
use crate::session_transport::{
    StateMiniStreamEvent, fetch_state_mini_snapshot, run_state_mini_stream,
    submit_expected_session_outbox, warm_realtime_connection,
};
use crate::state_mini::{
    latest_state_mini_revision, normalize_state_minis, require_valid_sequence, same_state_mini_key,
    sort_state_minis, validate_state_mini_delta, validate_state_minis,
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
    server_time: String,
    state_minis: Vec<ClientStateMini>,
    pending_mutations: Vec<ClientPendingMutation>,
    outbox: Vec<OutboundSessionFrame>,
    last_error: String,
}

#[derive(Debug, uniffi::Object)]
pub struct LooperClientCore {
    state: Mutex<ClientCoreState>,
    stream: Mutex<Option<ClientCoreStream>>,
    runtime: tokio::runtime::Runtime,
}

#[derive(Debug)]
struct ClientCoreStream {
    task: tokio::task::JoinHandle<()>,
    receiver: Option<mpsc::Receiver<StateMiniStreamEvent>>,
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
            stream: Mutex::new(None),
            runtime: tokio::runtime::Runtime::new().expect("looper client core runtime"),
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

    pub fn apply_command_batch_response(
        &self,
        response: ClientCommandBatchResponse,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        for envelope in response.command_acks {
            state.reconcile_ack(envelope.ack);
        }
        Ok(state.snapshot())
    }

    pub fn apply_state_delta(
        &self,
        delta: ClientStateDelta,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_valid_sequence(delta.seq)?;

        let mut state = self.lock_state()?;
        state.latest_seq = state.latest_seq.max(delta.seq);
        state.revision = delta.revision;
        state.server_time = delta.server_time;
        state.last_error.clear();
        Ok(state.snapshot())
    }

    pub fn replace_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let mut state = self.lock_state()?;
        state.latest_seq = snapshot.latest_seq;
        state.server_time = snapshot.server_time;
        state.state_minis = normalize_state_minis(snapshot.sessions);
        if let Some(revision) = latest_state_mini_revision(&state.state_minis) {
            state.revision = revision;
        }
        state.last_error.clear();
        Ok(state.snapshot())
    }

    pub fn apply_state_mini_delta(
        &self,
        delta: ClientStateMiniDelta,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        Ok(self.apply_state_mini_delta_with_result(delta)?.snapshot)
    }

    pub fn apply_state_mini_delta_with_result(
        &self,
        delta: ClientStateMiniDelta,
    ) -> Result<ClientStateMiniDeltaApplyResult, ClientCoreError> {
        validate_state_mini_delta(&delta)?;
        let mut state = self.lock_state()?;
        let did_change = state.apply_state_mini_delta(delta);
        Ok(ClientStateMiniDeltaApplyResult {
            snapshot: state.snapshot(),
            did_change,
        })
    }

    pub fn snapshot(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(state.snapshot())
    }

    pub fn take_outbox(&self) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let mut state = self.lock_state()?;
        Ok(std::mem::take(&mut state.outbox))
    }

    pub fn take_expected_outbox(
        &self,
        expected_client_mutation_ids: Vec<String>,
    ) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let mut state = self.lock_state()?;
        let actual_client_mutation_ids: Vec<&str> = state
            .outbox
            .iter()
            .map(|frame| frame.client_mutation_id.as_str())
            .collect();
        let expected_client_mutation_ids: Vec<&str> = expected_client_mutation_ids
            .iter()
            .map(String::as_str)
            .collect();
        if actual_client_mutation_ids != expected_client_mutation_ids {
            return Err(ClientCoreError::UnexpectedOutboxMutations);
        }

        Ok(std::mem::take(&mut state.outbox))
    }

    pub async fn submit_expected_outbox(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
        expected_client_mutation_ids: Vec<String>,
    ) -> Result<ClientCommandBatchResponse, ClientCoreError> {
        let frames = {
            let state = self.lock_state()?;
            state.expected_outbox(&expected_client_mutation_ids)?
        };
        let response =
            submit_expected_session_outbox(endpoints, bearer_token, mobile_session_header, frames)
                .await?;

        let mut state = self.lock_state()?;
        state.drain_expected_outbox(&expected_client_mutation_ids)?;
        for envelope in &response.command_acks {
            state.reconcile_ack(envelope.ack.clone());
        }
        Ok(response)
    }

    pub async fn warm_connection(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let endpoint = select_endpoint(&endpoints)?;
        validate_endpoint_url(&endpoint.url)?;
        warm_realtime_connection(endpoints, bearer_token, mobile_session_header).await?;

        let mut state = self.lock_state()?;
        state.phase = ConnectionPhase::Ready;
        state.endpoint_url = endpoint.url;
        state.last_error.clear();
        Ok(state.snapshot())
    }

    pub async fn recover_state_mini_snapshot(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let snapshot =
            fetch_state_mini_snapshot(endpoints, bearer_token, mobile_session_header).await?;
        self.replace_state_minis(snapshot)
    }

    pub fn start_state_mini_stream(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let endpoint = select_endpoint(&endpoints)?;
        validate_endpoint_url(&endpoint.url)?;
        let after_seq = {
            let mut state = self.lock_state()?;
            state.phase = ConnectionPhase::Ready;
            state.endpoint_url = endpoint.url;
            state.last_error.clear();
            state.latest_seq
        };
        let (sender, receiver) = mpsc::channel(64);
        let task = self.runtime.spawn(run_state_mini_stream(
            endpoints,
            bearer_token,
            mobile_session_header,
            after_seq,
            sender,
        ));
        self.replace_stream(ClientCoreStream {
            task,
            receiver: Some(receiver),
        })?;
        self.snapshot()
    }

    pub async fn next_state_mini_stream_update(
        &self,
    ) -> Result<ClientStateMiniStreamUpdate, ClientCoreError> {
        let mut receiver = {
            let mut stream = self.lock_stream()?;
            stream
                .as_mut()
                .and_then(|stream| stream.receiver.take())
                .ok_or(ClientCoreError::StateMiniStreamNotRunning)?
        };
        let event = receiver
            .recv()
            .await
            .ok_or(ClientCoreError::StateMiniStreamNotRunning);
        {
            let mut stream = self.lock_stream()?;
            if let Some(stream) = stream.as_mut() {
                stream.receiver = Some(receiver);
            }
        }
        self.apply_state_mini_stream_event(event?)
    }

    pub fn stop_state_mini_stream(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.replace_stream_none()?;
        self.disconnect()
    }
}

impl LooperClientCore {
    fn lock_state(&self) -> Result<MutexGuard<'_, ClientCoreState>, ClientCoreError> {
        self.state
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }

    fn lock_stream(&self) -> Result<MutexGuard<'_, Option<ClientCoreStream>>, ClientCoreError> {
        self.stream
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }

    fn replace_stream(&self, stream: ClientCoreStream) -> Result<(), ClientCoreError> {
        self.replace_stream_none()?;
        *self.lock_stream()? = Some(stream);
        Ok(())
    }

    fn replace_stream_none(&self) -> Result<(), ClientCoreError> {
        if let Some(stream) = self.lock_stream()?.take() {
            stream.task.abort();
        }
        Ok(())
    }

    fn apply_state_mini_stream_event(
        &self,
        event: StateMiniStreamEvent,
    ) -> Result<ClientStateMiniStreamUpdate, ClientCoreError> {
        let mut state = self.lock_state()?;
        let (reason, did_change, latest_seq, error_description) = match event {
            StateMiniStreamEvent::Delta(delta) => {
                let latest_seq = delta.seq.max(delta.latest_seq);
                let did_change = state.apply_state_mini_delta(delta);
                state.phase = ConnectionPhase::Ready;
                (
                    ClientStateMiniStreamUpdateReason::Delta,
                    did_change,
                    latest_seq,
                    String::new(),
                )
            }
            StateMiniStreamEvent::Heartbeat {
                latest_seq,
                server_time,
            } => {
                state.phase = ConnectionPhase::Ready;
                if !server_time.is_empty() {
                    state.server_time = server_time;
                }
                (
                    ClientStateMiniStreamUpdateReason::Heartbeat,
                    false,
                    latest_seq,
                    String::new(),
                )
            }
            StateMiniStreamEvent::RecoveredSnapshot {
                snapshot,
                error_description,
            } => {
                require_valid_sequence(snapshot.latest_seq)?;
                validate_state_minis(&snapshot.sessions)?;
                state.latest_seq = snapshot.latest_seq;
                state.server_time = snapshot.server_time;
                state.state_minis = normalize_state_minis(snapshot.sessions);
                if let Some(revision) = latest_state_mini_revision(&state.state_minis) {
                    state.revision = revision;
                }
                state.phase = ConnectionPhase::Ready;
                state.last_error.clear();
                (
                    ClientStateMiniStreamUpdateReason::RecoveryRequired,
                    true,
                    state.latest_seq,
                    error_description,
                )
            }
            StateMiniStreamEvent::Reconnecting {
                latest_seq,
                error_description,
            } => {
                state.phase = ConnectionPhase::Reconnecting;
                state.last_error = error_description.clone();
                (
                    ClientStateMiniStreamUpdateReason::Reconnecting,
                    false,
                    latest_seq,
                    error_description,
                )
            }
            StateMiniStreamEvent::RecoveryRequired {
                latest_seq,
                error_description,
            } => {
                state.phase = ConnectionPhase::Reconnecting;
                state.last_error = error_description.clone();
                (
                    ClientStateMiniStreamUpdateReason::RecoveryRequired,
                    false,
                    latest_seq,
                    error_description,
                )
            }
        };
        Ok(ClientStateMiniStreamUpdate {
            reason,
            snapshot: state.snapshot(),
            did_change,
            latest_seq,
            error_description,
        })
    }
}

impl ClientCoreState {
    fn snapshot(&self) -> ClientStateSnapshot {
        ClientStateSnapshot {
            phase: self.phase,
            endpoint_url: self.endpoint_url.clone(),
            latest_seq: self.latest_seq,
            revision: self.revision.clone(),
            server_time: self.server_time.clone(),
            state_minis: self.state_minis.clone(),
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

    fn expected_outbox(
        &self,
        expected_client_mutation_ids: &[String],
    ) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let actual_client_mutation_ids: Vec<&str> = self
            .outbox
            .iter()
            .map(|frame| frame.client_mutation_id.as_str())
            .collect();
        let expected_client_mutation_ids: Vec<&str> = expected_client_mutation_ids
            .iter()
            .map(String::as_str)
            .collect();
        if actual_client_mutation_ids != expected_client_mutation_ids {
            return Err(ClientCoreError::UnexpectedOutboxMutations);
        }

        Ok(self.outbox.clone())
    }

    fn drain_expected_outbox(
        &mut self,
        expected_client_mutation_ids: &[String],
    ) -> Result<(), ClientCoreError> {
        let _ = self.expected_outbox(expected_client_mutation_ids)?;
        self.outbox.clear();
        Ok(())
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

    fn upsert_state_mini(&mut self, session: ClientStateMini) {
        if let Some(index) = self
            .state_minis
            .iter()
            .position(|current| same_state_mini_key(current, &session))
        {
            self.state_minis[index] = session;
        } else {
            self.state_minis.push(session);
        }
        sort_state_minis(&mut self.state_minis);
    }

    fn apply_state_mini_delta(&mut self, delta: ClientStateMiniDelta) -> bool {
        if delta.seq <= self.latest_seq {
            return false;
        }

        if delta.has_session {
            self.upsert_state_mini(delta.session);
        } else if !delta.sessions.is_empty() {
            self.state_minis = normalize_state_minis(delta.sessions);
        }

        self.latest_seq = self.latest_seq.max(delta.seq).max(delta.latest_seq);
        if !delta.revision.is_empty() {
            self.revision = delta.revision;
        } else if let Some(revision) = latest_state_mini_revision(&self.state_minis) {
            self.revision = revision;
        }
        if !delta.server_time.is_empty() {
            self.server_time = delta.server_time;
        }
        self.last_error.clear();
        true
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
    use crate::model::ClientCommandAckEnvelope;

    const ENDPOINT_PRIMARY: &str = "http://127.0.0.1:8765";
    const ENDPOINT_LAST_GOOD: &str = "http://100.64.0.2:8765";
    const SERVER_TIME: &str = "2026-06-25T00:00:02Z";

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
    fn take_expected_outbox_drains_only_matching_mutations() {
        let core = LooperClientCore::new();
        core.set_mode(
            "thread-1".to_owned(),
            "await-reply".to_owned(),
            "cmid-mode".to_owned(),
        )
        .expect("queue mode");
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let mismatch = core
            .take_expected_outbox(vec!["cmid-prompt".to_owned(), "cmid-mode".to_owned()])
            .expect_err("mutation order mismatch");
        assert_eq!(mismatch, ClientCoreError::UnexpectedOutboxMutations);
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 2);

        let outbox = core
            .take_expected_outbox(vec!["cmid-mode".to_owned(), "cmid-prompt".to_owned()])
            .expect("matching outbox");

        assert_eq!(outbox.len(), 2);
        assert_eq!(outbox[0].client_mutation_id, "cmid-mode");
        assert_eq!(outbox[1].client_mutation_id, "cmid-prompt");
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 0);
    }

    #[test]
    fn submit_expected_outbox_keeps_commands_queued_when_transport_fails() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let error = runtime
            .block_on(core.submit_expected_outbox(
                Vec::new(),
                String::new(),
                String::new(),
                vec!["cmid-prompt".to_owned()],
            ))
            .expect_err("missing endpoint rejects");

        assert_eq!(error, ClientCoreError::NoEndpoint);
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 1);
        assert_eq!(
            core.snapshot().expect("snapshot").pending_mutations.len(),
            1
        );
    }

    #[test]
    fn warm_connection_rejects_missing_endpoint_without_state_change() {
        let core = LooperClientCore::new();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let error = runtime
            .block_on(core.warm_connection(Vec::new(), String::new(), String::new()))
            .expect_err("missing endpoint rejects");

        assert_eq!(error, ClientCoreError::NoEndpoint);
        assert_eq!(
            core.snapshot().expect("snapshot").phase,
            ConnectionPhase::Disconnected
        );
    }

    #[test]
    fn recover_state_mini_snapshot_rejects_missing_endpoint_without_state_change() {
        let core = LooperClientCore::new();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        let error = runtime
            .block_on(core.recover_state_mini_snapshot(Vec::new(), String::new(), String::new()))
            .expect_err("missing endpoint rejects");

        assert_eq!(error, ClientCoreError::NoEndpoint);
        assert!(core.snapshot().expect("snapshot").state_minis.is_empty());
    }

    #[test]
    fn state_mini_stream_delta_event_updates_core_snapshot() {
        let core = LooperClientCore::new();

        let update = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::Delta(ClientStateMiniDelta {
                seq: 7,
                latest_seq: 7,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-7".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: true,
                session: ClientStateMini {
                    session_id: "thread-1".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 7,
                    revision: "rev-7".to_owned(),
                    payload_json: "{\"sessionId\":\"thread-1\",\"assistantSurface\":\"codex\"}"
                        .to_owned(),
                },
                sessions: Vec::new(),
            }))
            .expect("stream update");

        assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Delta);
        assert!(update.did_change);
        assert_eq!(update.snapshot.latest_seq, 7);
        assert_eq!(update.snapshot.state_minis.len(), 1);
        assert_eq!(update.snapshot.state_minis[0].session_id, "thread-1");
    }

    #[test]
    fn state_mini_stream_ignores_stale_delta_then_applies_fresh_delta() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 5,
            sessions: vec![state_mini("thread-1", "codex", 5, "rev-5", "cached")],
            server_time: String::new(),
        })
        .expect("seed minis");

        let stale = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::Delta(ClientStateMiniDelta {
                seq: 4,
                latest_seq: 4,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-4".to_owned(),
                server_time: String::new(),
                has_session: true,
                session: state_mini("thread-1", "codex", 4, "rev-4", "stale"),
                sessions: Vec::new(),
            }))
            .expect("stale stream update");

        assert_eq!(stale.reason, ClientStateMiniStreamUpdateReason::Delta);
        assert!(!stale.did_change);
        assert_eq!(stale.snapshot.latest_seq, 5);
        assert_eq!(
            stale.snapshot.state_minis[0].payload_json,
            r#"{"title":"cached"}"#
        );

        let fresh = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::Delta(ClientStateMiniDelta {
                seq: 6,
                latest_seq: 6,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-6".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: true,
                session: state_mini("thread-1", "codex", 6, "rev-6", "streamed"),
                sessions: Vec::new(),
            }))
            .expect("fresh stream update");

        assert_eq!(fresh.reason, ClientStateMiniStreamUpdateReason::Delta);
        assert!(fresh.did_change);
        assert_eq!(fresh.snapshot.latest_seq, 6);
        assert_eq!(fresh.snapshot.revision, "rev-6");
        assert_eq!(
            fresh.snapshot.state_minis[0].payload_json,
            r#"{"title":"streamed"}"#
        );
    }

    #[test]
    fn state_mini_stream_recovery_event_marks_reconnecting_when_snapshot_recovery_fails() {
        let core = LooperClientCore::new();

        let update = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::RecoveryRequired {
                latest_seq: 9,
                error_description: "seq_gap".to_owned(),
            })
            .expect("stream update");

        assert_eq!(
            update.reason,
            ClientStateMiniStreamUpdateReason::RecoveryRequired
        );
        assert!(!update.did_change);
        assert_eq!(update.snapshot.phase, ConnectionPhase::Reconnecting);
        assert_eq!(update.snapshot.last_error, "seq_gap");
    }

    #[test]
    fn state_mini_stream_recovery_snapshot_replaces_state_in_rust_core() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 3,
            sessions: vec![state_mini("thread-1", "codex", 3, "rev-3", "old")],
            server_time: "2026-06-25T00:00:01Z".to_owned(),
        })
        .expect("seed state mini");

        let update = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::RecoveredSnapshot {
                snapshot: ClientStateMiniSnapshot {
                    latest_seq: 9,
                    sessions: vec![state_mini("thread-1", "codex", 9, "rev-9", "recovered")],
                    server_time: SERVER_TIME.to_owned(),
                },
                error_description: "seq_gap".to_owned(),
            })
            .expect("stream recovery snapshot");

        assert_eq!(
            update.reason,
            ClientStateMiniStreamUpdateReason::RecoveryRequired
        );
        assert!(update.did_change);
        assert_eq!(update.snapshot.phase, ConnectionPhase::Ready);
        assert_eq!(update.snapshot.latest_seq, 9);
        assert_eq!(update.snapshot.revision, "rev-9");
        assert!(update.snapshot.last_error.is_empty());
        assert_eq!(
            update.snapshot.state_minis[0].payload_json,
            r#"{"title":"recovered"}"#
        );
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
                current_state: String::new(),
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
                current_state: "awaiting_mode".to_owned(),
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
    fn command_batch_response_reconciles_all_acks_in_rust_core() {
        let core = LooperClientCore::new();
        core.set_mode(
            "thread-1".to_owned(),
            "await-reply".to_owned(),
            "cmid-mode".to_owned(),
        )
        .expect("queue mode");
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let snapshot = core
            .apply_command_batch_response(ClientCommandBatchResponse {
                accepted: false,
                command_acks: vec![
                    ClientCommandAckEnvelope {
                        command_kind: ClientCommandKind::SetSessionMode,
                        ack: ClientCommandAck {
                            accepted: true,
                            client_mutation_id: "cmid-mode".to_owned(),
                            ack_seq: 41,
                            entity_id: "thread-1".to_owned(),
                            revision: "rev-41".to_owned(),
                            server_time: "2026-06-25T00:00:00Z".to_owned(),
                            idempotent_replay: false,
                            error_code: String::new(),
                            reject_reason: String::new(),
                            current_state: String::new(),
                        },
                        preset: "await-reply".to_owned(),
                        dispatch_kind: String::new(),
                        prompt_id: String::new(),
                        notification_id: String::new(),
                    },
                    ClientCommandAckEnvelope {
                        command_kind: ClientCommandKind::SendSessionPrompt,
                        ack: ClientCommandAck {
                            accepted: false,
                            client_mutation_id: "cmid-prompt".to_owned(),
                            ack_seq: 42,
                            entity_id: "thread-1".to_owned(),
                            revision: "rev-42".to_owned(),
                            server_time: "2026-06-25T00:00:01Z".to_owned(),
                            idempotent_replay: false,
                            error_code: "mode_required".to_owned(),
                            reject_reason: "session is waiting for mode".to_owned(),
                            current_state: "mode_armed".to_owned(),
                        },
                        preset: String::new(),
                        dispatch_kind: "rejected".to_owned(),
                        prompt_id: String::new(),
                        notification_id: String::new(),
                    },
                ],
            })
            .expect("batch ack");

        assert_eq!(snapshot.latest_seq, 42);
        assert_eq!(snapshot.revision, "rev-42");
        assert!(snapshot.pending_mutations.is_empty());
        assert_eq!(
            snapshot.last_error,
            "mode_required: session is waiting for mode"
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
                server_time: SERVER_TIME.to_owned(),
                payload_json: r#"{"mode":"await-reply"}"#.to_owned(),
            })
            .expect("state delta");

        assert_eq!(snapshot.latest_seq, 44);
        assert_eq!(snapshot.revision, "rev-44");
        assert_eq!(snapshot.server_time, SERVER_TIME);
    }

    #[test]
    fn state_mini_snapshot_replaces_and_normalizes_records() {
        let core = LooperClientCore::new();

        let snapshot = core
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 10,
                sessions: vec![
                    state_mini("thread-2", "codex", 7, "rev-7", "queued"),
                    state_mini("thread-1", "codex", 5, "rev-5", "old"),
                    state_mini("thread-1", "codex", 9, "rev-9", "current"),
                ],
                server_time: SERVER_TIME.to_owned(),
            })
            .expect("replace minis");

        assert_eq!(snapshot.latest_seq, 10);
        assert_eq!(snapshot.revision, "rev-9");
        assert_eq!(snapshot.server_time, SERVER_TIME);
        assert_eq!(
            snapshot
                .state_minis
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["thread-2", "thread-1"]
        );
        assert_eq!(
            snapshot.state_minis[1].payload_json,
            r#"{"title":"current"}"#
        );
    }

    #[test]
    fn state_mini_delta_upserts_and_ignores_stale_sequences() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 2,
            sessions: vec![state_mini("thread-1", "codex", 2, "rev-2", "old")],
            server_time: String::new(),
        })
        .expect("seed minis");

        let result = core
            .apply_state_mini_delta_with_result(ClientStateMiniDelta {
                seq: 3,
                latest_seq: 3,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-3".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: true,
                session: state_mini("thread-1", "codex", 3, "rev-3", "new"),
                sessions: vec![],
            })
            .expect("apply mini delta");

        assert!(result.did_change);
        let snapshot = result.snapshot;
        assert_eq!(snapshot.latest_seq, 3);
        assert_eq!(snapshot.state_minis.len(), 1);
        assert_eq!(snapshot.state_minis[0].payload_json, r#"{"title":"new"}"#);

        let stale = core
            .apply_state_mini_delta_with_result(ClientStateMiniDelta {
                seq: 2,
                latest_seq: 2,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-stale".to_owned(),
                server_time: String::new(),
                has_session: true,
                session: state_mini("thread-1", "codex", 2, "rev-stale", "stale"),
                sessions: vec![],
            })
            .expect("ignore stale mini delta");

        assert!(!stale.did_change);
        assert_eq!(stale.snapshot.latest_seq, 3);
        assert_eq!(stale.snapshot.revision, "rev-3");
        assert_eq!(
            stale.snapshot.state_minis[0].payload_json,
            r#"{"title":"new"}"#
        );
    }

    #[test]
    fn invalid_state_mini_input_is_rejected() {
        let core = LooperClientCore::new();

        let error = core
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 1,
                sessions: vec![state_mini("", "codex", 1, "rev-1", "bad")],
                server_time: String::new(),
            })
            .expect_err("empty session id");

        assert_eq!(error, ClientCoreError::EmptySessionId);
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

    fn state_mini(
        session_id: &str,
        assistant_surface: &str,
        seq: i64,
        revision: &str,
        title: &str,
    ) -> ClientStateMini {
        ClientStateMini {
            session_id: session_id.to_owned(),
            assistant_surface: assistant_surface.to_owned(),
            seq,
            revision: revision.to_owned(),
            payload_json: format!(r#"{{"title":"{}"}}"#, title),
        }
    }
}

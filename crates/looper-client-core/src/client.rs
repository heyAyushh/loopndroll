use std::sync::{Arc, Mutex, MutexGuard, mpsc as std_mpsc};

use tokio::{
    sync::mpsc,
    time::{Duration, sleep},
};

use serde_json::Value;

use crate::command_batch::{build_command_batch_response, reduce_expected_command_ack};
use crate::error::ClientCoreError;
use crate::local_store::LooperClientCoreLocalStore;
#[cfg(test)]
use crate::model::ClientStateDelta;
#[cfg(test)]
use crate::model::ClientStateMiniDeltaApplyResult;
use crate::model::{
    ClientCommandAck, ClientCommandAckEnvelope, ClientCommandBatchResponse, ClientCommandKind,
    ClientEndpoint, ClientLocalStateSnapshot, ClientPendingCommand, ClientPendingCommandKind,
    ClientPendingMutation, ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot,
    ClientStateMiniStreamUpdate, ClientStateMiniStreamUpdateReason, ClientStateSnapshot,
    ConnectionPhase, OutboundSessionFrame, OutboundSessionFrameKind, STATE_MINI_REPLACEMENT_KIND,
};
use crate::session_transport::fetch_state_mini_snapshot;
use crate::session_transport::{StateMiniStreamEvent, command_metadata, run_state_mini_stream};
#[cfg(test)]
use crate::state_mini::validate_state_mini_delta;
use crate::state_mini::{
    latest_state_mini_revision, normalize_state_minis, require_valid_sequence, same_state_mini_key,
    sort_state_minis, validate_state_minis,
};
#[cfg(test)]
use crate::transport::validate_endpoint_url;

const INITIAL_SEQUENCE: i64 = 0;
const EMPTY_SEQUENCE: i64 = 0;
const COMMAND_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const COMMAND_FLUSH_RETRY_ATTEMPTS: usize = 5;
const COMMAND_FLUSH_RETRY_DELAY: Duration = Duration::from_millis(250);
const COMMAND_ACK_BACKLOG_LIMIT: usize = 64;
const MOBILE_SETTINGS_ENTITY_ID: &str = "mobile-settings";

#[derive(Debug, Default)]
struct ClientCoreState {
    phase: ConnectionPhase,
    endpoint_url: String,
    latest_seq: i64,
    revision: String,
    server_time: String,
    state_minis: Vec<ClientStateMini>,
    pending_mutations: Vec<ClientPendingMutation>,
    mode_rollbacks: Vec<ClientModeRollback>,
    outbox: Vec<OutboundSessionFrame>,
    command_ack_backlog: Vec<ClientCommandAck>,
    last_error: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClientModeRollback {
    client_mutation_id: String,
    thread_id: String,
    sessions: Vec<ClientStateMini>,
}

#[derive(Debug)]
pub(crate) struct LooperClientCore {
    state: Mutex<ClientCoreState>,
    stream: Mutex<Option<ClientCoreStream>>,
    local_updates: Mutex<Option<mpsc::UnboundedReceiver<ClientStateMiniStreamUpdate>>>,
    local_update_sender: mpsc::UnboundedSender<ClientStateMiniStreamUpdate>,
    observe_updates: tokio::sync::Mutex<()>,
    command_flush: tokio::sync::Mutex<()>,
    notification_reply_drain: tokio::sync::Mutex<()>,
    runtime: tokio::runtime::Runtime,
}

#[derive(Debug)]
struct ClientCoreStream {
    task: tokio::task::JoinHandle<()>,
    receiver: Option<mpsc::Receiver<StateMiniStreamEvent>>,
    command_sender: mpsc::Sender<OutboundSessionFrame>,
    command_ack_receiver: Option<mpsc::Receiver<ClientCommandAck>>,
    endpoints_identity: String,
}

impl ClientCoreStream {
    fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

struct LocalUpdateReceiverLease<'a> {
    core: &'a LooperClientCore,
    receiver: Option<mpsc::UnboundedReceiver<ClientStateMiniStreamUpdate>>,
}

impl LocalUpdateReceiverLease<'_> {
    async fn recv(&mut self) -> Option<ClientStateMiniStreamUpdate> {
        let receiver = self.receiver.as_mut()?;
        receiver.recv().await
    }

    fn restore(&mut self) -> Result<(), ClientCoreError> {
        if let Some(receiver) = self.receiver.take() {
            self.core
                .restore_local_update_receiver_if_absent(receiver)?;
        }
        Ok(())
    }
}

impl Drop for LocalUpdateReceiverLease<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

struct StreamEventReceiverLease<'a> {
    core: &'a LooperClientCore,
    receiver: Option<mpsc::Receiver<StateMiniStreamEvent>>,
}

impl StreamEventReceiverLease<'_> {
    async fn recv(&mut self) -> Option<StateMiniStreamEvent> {
        let receiver = self.receiver.as_mut()?;
        receiver.recv().await
    }

    fn restore(&mut self) -> Result<(), ClientCoreError> {
        if let Some(receiver) = self.receiver.take() {
            self.core
                .restore_stream_event_receiver_if_absent(receiver)?;
        }
        Ok(())
    }
}

impl Drop for StreamEventReceiverLease<'_> {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

impl LooperClientCore {
    pub(crate) fn new() -> Arc<Self> {
        let (local_update_sender, local_updates) = mpsc::unbounded_channel();
        Arc::new(Self {
            state: Mutex::new(ClientCoreState {
                latest_seq: INITIAL_SEQUENCE,
                ..ClientCoreState::default()
            }),
            stream: Mutex::new(None),
            local_updates: Mutex::new(Some(local_updates)),
            local_update_sender,
            observe_updates: tokio::sync::Mutex::new(()),
            command_flush: tokio::sync::Mutex::new(()),
            notification_reply_drain: tokio::sync::Mutex::new(()),
            runtime: tokio::runtime::Runtime::new().expect("looper client core runtime"),
        })
    }

    pub(crate) fn start(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.start_state_mini_stream(endpoints, bearer_token, mobile_session_header)
    }

    pub(crate) fn stop(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.replace_stream_none()?;
        self.disconnect()
    }

    pub(crate) async fn observe(&self) -> Result<ClientStateMiniStreamUpdate, ClientCoreError> {
        let _observe = self.observe_updates.lock().await;
        self.next_state_mini_stream_update().await
    }
}

impl LooperClientCore {
    #[cfg(test)]
    fn connect(
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

    fn disconnect(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.phase = ConnectionPhase::Disconnected;
        Ok(state.snapshot())
    }

    fn set_mode(
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
            thread_id: thread_id.clone(),
            preset: preset.clone(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            archived: false,
            client_mutation_id: client_mutation_id.clone(),
            after_seq: EMPTY_SEQUENCE,
        });
        state.apply_optimistic_mode(&thread_id, &preset, &client_mutation_id);
        Ok(state.snapshot())
    }

    fn send_prompt(
        &self,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        prompt_intent: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;
        let prompt_intent = normalized_prompt_intent(prompt_intent)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SendSessionPrompt,
            thread_id,
            preset: String::new(),
            prompt,
            prompt_intent,
            assistant_surface,
            notification_id: String::new(),
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn submit_notification_reply(
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
            prompt_intent: String::new(),
            assistant_surface,
            notification_id,
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn set_assistant_surface(
        &self,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&assistant_surface, ClientCoreError::EmptySessionId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SetAssistantSurface,
            thread_id: MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            preset: String::new(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface,
            notification_id: String::new(),
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn set_siri_current_session(
        &self,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.queue_siri_session_command(
            ClientCommandKind::SetSiriCurrentSession,
            thread_id,
            assistant_surface,
            client_mutation_id,
        )
    }

    fn set_siri_default_session(
        &self,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.queue_siri_session_command(
            ClientCommandKind::SetSiriDefaultSession,
            thread_id,
            assistant_surface,
            client_mutation_id,
        )
    }

    fn save_default_prompt(
        &self,
        prompt: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SaveDefaultPrompt,
            thread_id: MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            preset: String::new(),
            prompt,
            prompt_intent: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn queue_siri_session_command(
        &self,
        command_kind: ClientCommandKind,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind,
            thread_id,
            preset: String::new(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface,
            notification_id: String::new(),
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn set_session_archived(
        &self,
        thread_id: String,
        archived: bool,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::SetSessionArchived,
            thread_id,
            preset: String::new(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            archived,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn delete_session(
        &self,
        thread_id: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::DeleteSession,
            thread_id,
            preset: String::new(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    fn mute_session(
        &self,
        thread_id: String,
        client_mutation_id: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_present(&thread_id, ClientCoreError::EmptyThreadId)?;
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state.queue_command(OutboundSessionFrame {
            frame_kind: OutboundSessionFrameKind::Command,
            command_kind: ClientCommandKind::MuteSession,
            thread_id,
            preset: String::new(),
            prompt: String::new(),
            prompt_intent: String::new(),
            assistant_surface: String::new(),
            notification_id: String::new(),
            archived: false,
            client_mutation_id,
            after_seq: EMPTY_SEQUENCE,
        });
        Ok(state.snapshot())
    }

    #[cfg(test)]
    fn apply_command_ack(
        &self,
        ack: ClientCommandAck,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        state.reconcile_ack(ack);
        Ok(state.snapshot())
    }

    #[cfg(test)]
    fn apply_command_batch_response(
        &self,
        response: ClientCommandBatchResponse,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let mut state = self.lock_state()?;
        for envelope in response.command_acks {
            state.reconcile_ack(envelope.ack);
        }
        Ok(state.snapshot())
    }

    #[cfg(test)]
    fn apply_state_delta(
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
}

impl LooperClientCore {
    pub(crate) fn replace_state_minis(
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

    #[cfg(test)]
    pub(crate) fn apply_state_mini_delta_with_result(
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

    pub(crate) fn snapshot(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(state.snapshot())
    }
}

impl LooperClientCore {
    fn pending_outbox_client_mutation_ids(&self) -> Result<Vec<String>, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(state.pending_outbox_client_mutation_ids())
    }

    pub(crate) fn restore_pending_commands(
        &self,
        pending_commands: Vec<ClientPendingCommand>,
    ) -> Result<Vec<String>, ClientCoreError> {
        let mut restored_client_mutation_ids = Vec::new();
        let mut state = self.lock_state()?;
        for command in pending_commands {
            let frame = restored_outbound_frame(command)?;
            restored_client_mutation_ids.push(frame.client_mutation_id.clone());
            state.queue_command(frame);
        }
        Ok(restored_client_mutation_ids)
    }

    #[cfg(test)]
    fn take_outbox(&self) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let mut state = self.lock_state()?;
        Ok(std::mem::take(&mut state.outbox))
    }

    #[cfg(test)]
    fn take_expected_outbox(
        &self,
        expected_client_mutation_ids: Vec<String>,
    ) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let mut state = self.lock_state()?;
        let outbox = state.expected_outbox(&expected_client_mutation_ids)?;
        state.drain_expected_outbox(&expected_client_mutation_ids)?;
        Ok(outbox)
    }

    async fn submit_expected_outbox(
        &self,
        expected_client_mutation_ids: Vec<String>,
    ) -> Result<ClientCommandBatchResponse, ClientCoreError> {
        let frames = {
            let state = self.lock_state()?;
            state.expected_outbox(&expected_client_mutation_ids)?
        };
        let command_metadata = frames
            .iter()
            .map(command_metadata)
            .collect::<Result<Vec<_>, _>>()?;
        let submitted_frames = frames.clone();
        let expected_ack_count = expected_client_mutation_ids.len();
        self.send_session_commands(frames).await?;
        let acks = self
            .recv_command_acks(expected_client_mutation_ids, expected_ack_count)
            .await?;
        let response = build_command_batch_response(command_metadata, acks)?;

        let mut state = self.lock_state()?;
        state.drain_submitted_outbox(&submitted_frames)?;
        for envelope in &response.command_acks {
            state.reconcile_ack(envelope.ack.clone());
        }
        Ok(response)
    }

    async fn submit_pending_outbox(
        &self,
        expected_client_mutation_ids: Vec<String>,
    ) -> Result<ClientCommandBatchResponse, ClientCoreError> {
        self.submit_expected_outbox(expected_client_mutation_ids)
            .await
    }
}

impl LooperClientCore {
    pub(crate) fn queue_set_mode_durable(
        &self,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        let snapshot = self.set_mode(
            thread_id.clone(),
            preset.clone(),
            client_mutation_id.clone(),
        )?;
        local_store.enqueue_set_mode_command(thread_id, preset, client_mutation_id)?;
        let local_snapshot = persist_state_minis_to_local_store(&local_store, snapshot.clone())?;
        self.emit_local_state_update(snapshot);
        Ok(local_snapshot)
    }

    pub(crate) fn accept_set_mode_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.queue_set_mode_durable(
            local_store.clone(),
            thread_id,
            preset,
            client_mutation_id.clone(),
        )?;
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_send_prompt_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        prompt_intent: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.send_prompt(
            thread_id.clone(),
            prompt.clone(),
            assistant_surface.clone(),
            prompt_intent.clone(),
            client_mutation_id.clone(),
        )?;
        local_store.enqueue_send_prompt_command(
            thread_id,
            prompt,
            assistant_surface,
            prompt_intent,
            client_mutation_id.clone(),
        )?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_set_assistant_surface_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.set_assistant_surface(assistant_surface.clone(), client_mutation_id.clone())?;
        local_store
            .enqueue_set_assistant_surface_command(assistant_surface, client_mutation_id.clone())?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_set_siri_current_session_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.set_siri_current_session(
            thread_id.clone(),
            assistant_surface.clone(),
            client_mutation_id.clone(),
        )?;
        local_store.enqueue_set_siri_current_session_command(
            thread_id,
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_set_siri_default_session_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.set_siri_default_session(
            thread_id.clone(),
            assistant_surface.clone(),
            client_mutation_id.clone(),
        )?;
        local_store.enqueue_set_siri_default_session_command(
            thread_id,
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_save_default_prompt_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        prompt: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.save_default_prompt(prompt.clone(), client_mutation_id.clone())?;
        local_store.enqueue_save_default_prompt_command(prompt, client_mutation_id.clone())?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_set_session_archived_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        archived: bool,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.set_session_archived(thread_id.clone(), archived, client_mutation_id.clone())?;
        local_store.enqueue_set_session_archived_command(
            thread_id,
            archived,
            client_mutation_id.clone(),
        )?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_delete_session_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.delete_session(thread_id.clone(), client_mutation_id.clone())?;
        local_store.enqueue_delete_session_command(thread_id, client_mutation_id.clone())?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) fn accept_mute_session_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        thread_id: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.mute_session(thread_id.clone(), client_mutation_id.clone())?;
        local_store.enqueue_mute_session_command(thread_id, client_mutation_id.clone())?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    async fn submit_notification_reply_durable_without_flush_lock(
        &self,
        local_store: Arc<LooperClientCoreLocalStore>,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        self.submit_notification_reply(
            notification_id.clone(),
            thread_id.clone(),
            prompt.clone(),
            assistant_surface.clone(),
            client_mutation_id.clone(),
        )?;
        local_store.enqueue_notification_reply_command(
            notification_id,
            thread_id,
            prompt,
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        let envelope = self
            .submit_pending_command_ack(
                ClientCommandKind::SubmitNotificationReply,
                client_mutation_id,
            )
            .await?;
        self.mark_durable_command_final(&local_store, &envelope)?;
        self.emit_local_state_update(self.snapshot()?);
        Ok(envelope)
    }

    pub(crate) fn accept_notification_reply_durable(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<(), ClientCoreError> {
        self.submit_notification_reply(
            notification_id.clone(),
            thread_id.clone(),
            prompt.clone(),
            assistant_surface.clone(),
            client_mutation_id.clone(),
        )?;
        local_store.enqueue_notification_reply_command(
            notification_id,
            thread_id,
            prompt,
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        self.emit_local_state_update(self.snapshot()?);
        local_store.mark_attempted(client_mutation_id.clone())?;
        self.spawn_command_ack_flush(local_store, client_mutation_id);
        Ok(())
    }

    pub(crate) async fn drain_notification_reply_outbox_durable(
        &self,
        local_store: Arc<LooperClientCoreLocalStore>,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        let _drain = self.notification_reply_drain.lock().await;
        let mut last_envelope = None;
        loop {
            let plan = local_store.notification_reply_retry_plan()?;
            if !plan.has_pending {
                return last_envelope.ok_or(ClientCoreError::NoPendingNotificationReply);
            }

            if plan.delay_nanoseconds > 0 {
                sleep(Duration::from_nanos(plan.delay_nanoseconds)).await;
            }

            match self
                .submit_next_notification_reply_durable_without_drain_lock(local_store.clone())
                .await
            {
                Ok(envelope) if envelope.ack.accepted => {
                    last_envelope = Some(envelope);
                }
                Ok(envelope) => return Ok(envelope),
                Err(error) if should_retry_notification_reply_drain(&error) => {}
                Err(error) => return Err(error),
            }
        }
    }
}

impl LooperClientCore {
    pub(crate) fn recover_state_mini_snapshot(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let (sender, receiver) = std_mpsc::sync_channel(1);
        self.runtime.spawn(async move {
            let result =
                fetch_state_mini_snapshot(endpoints, bearer_token, mobile_session_header).await;
            let _ = sender.send(result);
        });
        let snapshot = receiver
            .recv()
            .map_err(|_| ClientCoreError::StateMiniSnapshotTransportFailed)??;
        self.replace_state_minis(snapshot)
    }

    fn start_state_mini_stream(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        require_endpoints(&endpoints)?;
        let endpoints_identity = endpoints_identity(&endpoints);
        let (sender, receiver) = mpsc::channel(64);
        let (command_sender, command_receiver) = mpsc::channel(64);
        let (command_ack_sender, command_ack_receiver) = mpsc::channel(64);
        let mut state = self.lock_state()?;
        let mut stream = self.lock_stream()?;
        let has_running_stream = stream
            .as_ref()
            .map(ClientCoreStream::is_running)
            .unwrap_or(false);
        let is_same_stream_configuration = stream
            .as_ref()
            .map(|stream| stream.endpoints_identity == endpoints_identity)
            .unwrap_or(false);
        if is_same_stream_configuration && has_running_stream {
            return Ok(state.snapshot());
        }

        state.phase = ConnectionPhase::Connecting;
        state.endpoint_url.clear();
        state.last_error.clear();
        let after_seq = state.latest_seq;
        let task = self.runtime.spawn(run_state_mini_stream(
            endpoints,
            bearer_token,
            mobile_session_header,
            after_seq,
            command_receiver,
            sender,
            command_ack_sender,
        ));
        if let Some(existing_stream) = stream.take() {
            existing_stream.task.abort();
        }
        *stream = Some(ClientCoreStream {
            task,
            receiver: Some(receiver),
            command_sender,
            command_ack_receiver: Some(command_ack_receiver),
            endpoints_identity,
        });
        Ok(state.snapshot())
    }
}

impl LooperClientCore {
    async fn next_state_mini_stream_update(
        &self,
    ) -> Result<ClientStateMiniStreamUpdate, ClientCoreError> {
        if let Some(update) = self.try_recv_local_update()? {
            return Ok(update);
        }

        let mut local_receiver = self.take_local_update_receiver_lease()?;
        let maybe_stream_receiver = self.take_stream_event_receiver_lease()?;

        enum NextUpdate {
            Local(ClientStateMiniStreamUpdate),
            Stream(StateMiniStreamEvent),
        }

        let next = match maybe_stream_receiver {
            Some(mut stream_receiver) => {
                let next = tokio::select! {
                    local = local_receiver.recv() => local
                        .map(NextUpdate::Local)
                        .ok_or(ClientCoreError::StateMiniStreamNotRunning),
                    stream = stream_receiver.recv() => stream
                        .map(NextUpdate::Stream)
                        .ok_or(ClientCoreError::StateMiniStreamNotRunning),
                };
                stream_receiver.restore()?;
                next
            }
            None => local_receiver
                .recv()
                .await
                .map(NextUpdate::Local)
                .ok_or(ClientCoreError::StateMiniStreamNotRunning),
        };
        local_receiver.restore()?;

        match next? {
            NextUpdate::Local(update) => Ok(update),
            NextUpdate::Stream(event) => self.apply_state_mini_stream_event(event),
        }
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

    async fn send_session_commands(
        &self,
        frames: Vec<OutboundSessionFrame>,
    ) -> Result<(), ClientCoreError> {
        if frames.is_empty() {
            return Ok(());
        }
        let sender = self.command_sender()?;
        for frame in frames {
            sender
                .send(frame)
                .await
                .map_err(|_| ClientCoreError::SessionCommandTransportFailed)?;
        }
        Ok(())
    }

    fn command_sender(&self) -> Result<mpsc::Sender<OutboundSessionFrame>, ClientCoreError> {
        self.lock_stream()?
            .as_ref()
            .map(|stream| stream.command_sender.clone())
            .ok_or(ClientCoreError::NoEndpoint)
    }

    async fn recv_command_acks(
        &self,
        expected_client_mutation_ids: Vec<String>,
        expected_ack_count: usize,
    ) -> Result<Vec<ClientCommandAck>, ClientCoreError> {
        if expected_ack_count == 0 {
            return Ok(Vec::new());
        }
        let expected_client_mutation_ids =
            std::collections::HashSet::<String>::from_iter(expected_client_mutation_ids);
        let mut acks =
            self.take_buffered_command_acks(&expected_client_mutation_ids, expected_ack_count)?;
        if acks.len() >= expected_ack_count {
            return Ok(acks);
        }

        let mut receiver = self.take_command_ack_receiver()?;
        let result = tokio::time::timeout(COMMAND_ACK_TIMEOUT, async {
            while acks.len() < expected_ack_count {
                let ack = receiver
                    .recv()
                    .await
                    .ok_or(ClientCoreError::SessionCommandTransportFailed)?;
                if expected_client_mutation_ids.contains(&ack.client_mutation_id)
                    && !acks.iter().any(|seen: &ClientCommandAck| {
                        seen.client_mutation_id == ack.client_mutation_id
                    })
                {
                    acks.push(ack);
                } else {
                    self.buffer_command_ack(ack)?;
                }
            }
            Ok::<_, ClientCoreError>(acks)
        })
        .await
        .map_err(|_| ClientCoreError::SessionCommandAckTimedOut)
        .and_then(|result| result);
        self.restore_command_ack_receiver(receiver)?;
        result
    }

    fn take_command_ack_receiver(
        &self,
    ) -> Result<mpsc::Receiver<ClientCommandAck>, ClientCoreError> {
        self.lock_stream()?
            .as_mut()
            .and_then(|stream| stream.command_ack_receiver.take())
            .ok_or(ClientCoreError::NoEndpoint)
    }

    fn restore_command_ack_receiver(
        &self,
        receiver: mpsc::Receiver<ClientCommandAck>,
    ) -> Result<(), ClientCoreError> {
        let mut stream = self.lock_stream()?;
        let stream = stream.as_mut().ok_or(ClientCoreError::NoEndpoint)?;
        stream.command_ack_receiver = Some(receiver);
        Ok(())
    }

    fn take_buffered_command_acks(
        &self,
        expected_client_mutation_ids: &std::collections::HashSet<String>,
        expected_ack_count: usize,
    ) -> Result<Vec<ClientCommandAck>, ClientCoreError> {
        let mut state = self.lock_state()?;
        let mut matching_acks = Vec::with_capacity(expected_ack_count);
        let mut remaining_acks = Vec::with_capacity(state.command_ack_backlog.len());
        for ack in state.command_ack_backlog.drain(..) {
            if expected_client_mutation_ids.contains(&ack.client_mutation_id)
                && !matching_acks.iter().any(|seen: &ClientCommandAck| {
                    seen.client_mutation_id == ack.client_mutation_id
                })
                && matching_acks.len() < expected_ack_count
            {
                matching_acks.push(ack);
            } else {
                remaining_acks.push(ack);
            }
        }
        state.command_ack_backlog = remaining_acks;
        Ok(matching_acks)
    }

    fn buffer_command_ack(&self, ack: ClientCommandAck) -> Result<(), ClientCoreError> {
        let mut state = self.lock_state()?;
        if state
            .command_ack_backlog
            .iter()
            .any(|seen| seen.client_mutation_id == ack.client_mutation_id)
        {
            return Ok(());
        }
        state.command_ack_backlog.push(ack);
        if state.command_ack_backlog.len() > COMMAND_ACK_BACKLOG_LIMIT {
            state.command_ack_backlog.remove(0);
        }
        Ok(())
    }

    fn lock_local_updates(
        &self,
    ) -> Result<
        MutexGuard<'_, Option<mpsc::UnboundedReceiver<ClientStateMiniStreamUpdate>>>,
        ClientCoreError,
    > {
        self.local_updates
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }

    fn take_local_update_receiver(
        &self,
    ) -> Result<mpsc::UnboundedReceiver<ClientStateMiniStreamUpdate>, ClientCoreError> {
        self.lock_local_updates()?
            .take()
            .ok_or(ClientCoreError::StateMiniStreamNotRunning)
    }

    fn take_local_update_receiver_lease(
        &self,
    ) -> Result<LocalUpdateReceiverLease<'_>, ClientCoreError> {
        Ok(LocalUpdateReceiverLease {
            core: self,
            receiver: Some(self.take_local_update_receiver()?),
        })
    }

    fn take_stream_event_receiver_lease(
        &self,
    ) -> Result<Option<StreamEventReceiverLease<'_>>, ClientCoreError> {
        let receiver = {
            let mut stream = self.lock_stream()?;
            stream.as_mut().and_then(|stream| stream.receiver.take())
        };
        Ok(receiver.map(|receiver| StreamEventReceiverLease {
            core: self,
            receiver: Some(receiver),
        }))
    }

    fn restore_local_update_receiver(
        &self,
        receiver: mpsc::UnboundedReceiver<ClientStateMiniStreamUpdate>,
    ) -> Result<(), ClientCoreError> {
        *self.lock_local_updates()? = Some(receiver);
        Ok(())
    }

    fn restore_local_update_receiver_if_absent(
        &self,
        receiver: mpsc::UnboundedReceiver<ClientStateMiniStreamUpdate>,
    ) -> Result<(), ClientCoreError> {
        let mut updates = self.lock_local_updates()?;
        if updates.is_none() {
            *updates = Some(receiver);
        }
        Ok(())
    }

    fn restore_stream_event_receiver_if_absent(
        &self,
        receiver: mpsc::Receiver<StateMiniStreamEvent>,
    ) -> Result<(), ClientCoreError> {
        let mut stream = self.lock_stream()?;
        if let Some(stream) = stream.as_mut() {
            if stream.receiver.is_none() {
                stream.receiver = Some(receiver);
            }
        }
        Ok(())
    }

    fn try_recv_local_update(
        &self,
    ) -> Result<Option<ClientStateMiniStreamUpdate>, ClientCoreError> {
        let mut receiver = self.take_local_update_receiver()?;
        let result = match receiver.try_recv() {
            Ok(update) => Ok(Some(update)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err(ClientCoreError::StateMiniStreamNotRunning)
            }
        };
        self.restore_local_update_receiver(receiver)?;
        result
    }

    fn emit_local_state_update(&self, snapshot: ClientStateSnapshot) {
        let _ = self.local_update_sender.send(ClientStateMiniStreamUpdate {
            reason: ClientStateMiniStreamUpdateReason::Delta,
            did_change: true,
            latest_seq: snapshot.latest_seq,
            error_description: String::new(),
            snapshot,
        });
    }

    fn spawn_command_ack_flush(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        client_mutation_id: String,
    ) {
        let client_core = self.clone();
        let handle = self.runtime.handle().clone();
        self.runtime.spawn_blocking(move || {
            let flush_result = handle.block_on(client_core.flush_pending_outbox_with_retries(
                local_store,
                vec![client_mutation_id],
                false,
            ));
            if let Err(error) = flush_result {
                let _ = client_core.emit_command_flush_error(error);
            }
        });
    }

    pub(crate) fn spawn_restored_command_ack_flush(
        self: &Arc<Self>,
        local_store: Arc<LooperClientCoreLocalStore>,
        restored_client_mutation_ids: Vec<String>,
    ) {
        let restored_client_mutation_ids = non_empty_unique(restored_client_mutation_ids);
        if restored_client_mutation_ids.is_empty() {
            return;
        }

        let client_core = self.clone();
        let handle = self.runtime.handle().clone();
        self.runtime.spawn_blocking(move || {
            let flush_result = handle.block_on(client_core.flush_pending_outbox_with_retries(
                local_store,
                restored_client_mutation_ids,
                true,
            ));
            if let Err(error) = flush_result {
                let _ = client_core.emit_command_flush_error(error);
            }
        });
    }

    async fn flush_pending_outbox_with_retries(
        &self,
        local_store: Arc<LooperClientCoreLocalStore>,
        required_client_mutation_ids: Vec<String>,
        mark_required_attempted: bool,
    ) -> Result<(), ClientCoreError> {
        let required_client_mutation_ids = non_empty_unique(required_client_mutation_ids);
        if required_client_mutation_ids.is_empty() {
            return Ok(());
        }

        let mut last_error = None;
        for attempt_index in 0..COMMAND_FLUSH_RETRY_ATTEMPTS {
            let result = self
                .flush_pending_outbox_once(
                    local_store.clone(),
                    &required_client_mutation_ids,
                    mark_required_attempted,
                )
                .await;
            match result {
                Ok(()) => return Ok(()),
                Err(error)
                    if should_retry_command_flush(&error)
                        && attempt_index + 1 < COMMAND_FLUSH_RETRY_ATTEMPTS =>
                {
                    last_error = Some(error);
                    sleep(COMMAND_FLUSH_RETRY_DELAY).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_error.unwrap_or(ClientCoreError::MissingCommandAcknowledgement))
    }

    async fn flush_pending_outbox_once(
        &self,
        local_store: Arc<LooperClientCoreLocalStore>,
        required_client_mutation_ids: &[String],
        mark_required_attempted: bool,
    ) -> Result<(), ClientCoreError> {
        let _flush = self.command_flush.lock().await;
        let expected_client_mutation_ids = self.pending_outbox_client_mutation_ids()?;
        let required_ids_still_pending = required_client_mutation_ids
            .iter()
            .filter(|id| expected_client_mutation_ids.contains(id))
            .cloned()
            .collect::<Vec<_>>();
        if required_ids_still_pending.is_empty() {
            return Ok(());
        }

        if mark_required_attempted {
            for client_mutation_id in &required_ids_still_pending {
                local_store.mark_attempted(client_mutation_id.clone())?;
            }
        }

        let response = self
            .submit_pending_outbox(expected_client_mutation_ids)
            .await?;
        for envelope in &response.command_acks {
            self.mark_durable_command_final(&local_store, envelope)?;
        }

        let acknowledged_ids = response
            .command_acks
            .iter()
            .map(|envelope| envelope.ack.client_mutation_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        if required_ids_still_pending
            .iter()
            .any(|id| !acknowledged_ids.contains(id.as_str()))
        {
            return Err(ClientCoreError::MissingCommandAcknowledgement);
        }

        let snapshot = self.snapshot()?;
        persist_state_minis_to_local_store(&local_store, snapshot.clone())?;
        self.emit_local_state_update(snapshot);
        Ok(())
    }

    fn emit_command_flush_error(&self, error: ClientCoreError) -> Result<(), ClientCoreError> {
        let snapshot = {
            let mut state = self.lock_state()?;
            state.last_error = error.to_string();
            state.snapshot()
        };
        self.emit_local_state_update(snapshot);
        Ok(())
    }

    async fn submit_next_notification_reply_durable_without_drain_lock(
        &self,
        local_store: Arc<LooperClientCoreLocalStore>,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        let _flush = self.command_flush.lock().await;
        let command = local_store
            .pending_notification_reply_command()?
            .ok_or(ClientCoreError::NoPendingNotificationReply)?;
        self.submit_notification_reply_durable_without_flush_lock(
            local_store,
            command.notification_id,
            command.thread_id,
            command.prompt,
            command.assistant_surface,
            command.client_mutation_id,
        )
        .await
    }

    async fn submit_pending_command_ack(
        &self,
        command_kind: ClientCommandKind,
        client_mutation_id: String,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        let expected_client_mutation_ids = self.pending_outbox_client_mutation_ids()?;
        let response = self
            .submit_pending_outbox(expected_client_mutation_ids)
            .await?;
        reduce_expected_command_ack(response, command_kind, client_mutation_id)
    }

    fn mark_durable_command_final(
        &self,
        local_store: &LooperClientCoreLocalStore,
        envelope: &ClientCommandAckEnvelope,
    ) -> Result<(), ClientCoreError> {
        local_store.mark_delivered(envelope.ack.client_mutation_id.clone())
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
                endpoint_url,
            } => {
                state.phase = ConnectionPhase::Ready;
                update_server_time_if_newer(&mut state.server_time, server_time);
                if !endpoint_url.is_empty() {
                    state.endpoint_url = endpoint_url;
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
        if is_latest_wins_outbox_command(frame.command_kind) {
            self.pending_mutations.retain(|mutation| {
                mutation.client_mutation_id == frame.client_mutation_id
                    || mutation.command_kind != frame.command_kind
                    || mutation.thread_id != frame.thread_id
            });
            self.outbox.retain(|queued| {
                queued.client_mutation_id == frame.client_mutation_id
                    || queued.command_kind != frame.command_kind
                    || queued.thread_id != frame.thread_id
            });
        }

        let pending_mutation = ClientPendingMutation {
            client_mutation_id: frame.client_mutation_id.clone(),
            command_kind: frame.command_kind,
            thread_id: frame.thread_id.clone(),
        };
        if let Some(existing) = self
            .pending_mutations
            .iter_mut()
            .find(|mutation| mutation.client_mutation_id == pending_mutation.client_mutation_id)
        {
            *existing = pending_mutation;
        } else {
            self.pending_mutations.push(pending_mutation);
        }
        if let Some(existing) = self
            .outbox
            .iter_mut()
            .find(|queued| queued.client_mutation_id == frame.client_mutation_id)
        {
            *existing = frame;
        } else {
            self.outbox.push(frame);
        }
        self.last_error.clear();
    }

    fn pending_outbox_client_mutation_ids(&self) -> Vec<String> {
        self.outbox
            .iter()
            .map(|frame| frame.client_mutation_id.clone())
            .collect()
    }

    fn expected_outbox(
        &self,
        expected_client_mutation_ids: &[String],
    ) -> Result<Vec<OutboundSessionFrame>, ClientCoreError> {
        let mut frames = Vec::with_capacity(expected_client_mutation_ids.len());
        for client_mutation_id in expected_client_mutation_ids {
            let Some(frame) = self
                .outbox
                .iter()
                .find(|frame| frame.client_mutation_id == *client_mutation_id)
            else {
                return Err(ClientCoreError::UnexpectedOutboxMutations);
            };
            frames.push(frame.clone());
        }
        Ok(frames)
    }

    #[cfg(test)]
    fn drain_expected_outbox(
        &mut self,
        expected_client_mutation_ids: &[String],
    ) -> Result<(), ClientCoreError> {
        let _ = self.expected_outbox(expected_client_mutation_ids)?;
        self.outbox.retain(|frame| {
            !expected_client_mutation_ids
                .iter()
                .any(|client_mutation_id| frame.client_mutation_id == *client_mutation_id)
        });
        Ok(())
    }

    fn drain_submitted_outbox(
        &mut self,
        submitted_frames: &[OutboundSessionFrame],
    ) -> Result<(), ClientCoreError> {
        for frame in submitted_frames {
            let still_pending = self
                .outbox
                .iter()
                .any(|queued| queued.client_mutation_id == frame.client_mutation_id);
            if !still_pending && !is_latest_wins_outbox_command(frame.command_kind) {
                return Err(ClientCoreError::UnexpectedOutboxMutations);
            }
        }
        self.outbox.retain(|queued| {
            !submitted_frames
                .iter()
                .any(|frame| frame.client_mutation_id == queued.client_mutation_id)
        });
        Ok(())
    }

    fn reconcile_ack(&mut self, ack: ClientCommandAck) {
        let reject_message = if ack.accepted {
            String::new()
        } else {
            reject_message(&ack)
        };
        if ack.ack_seq > self.latest_seq && !ack.revision.is_empty() {
            self.revision = ack.revision.clone();
        }
        self.latest_seq = self.latest_seq.max(ack.ack_seq);
        let command_kind = self
            .pending_mutations
            .iter()
            .find(|mutation| mutation.client_mutation_id == ack.client_mutation_id)
            .map(|mutation| mutation.command_kind);
        update_server_time_if_newer(&mut self.server_time, ack.server_time.clone());
        self.pending_mutations
            .retain(|mutation| mutation.client_mutation_id != ack.client_mutation_id);
        if command_kind == Some(ClientCommandKind::SetSessionMode) && !ack.accepted {
            self.restore_mode_rollback(&ack.client_mutation_id);
        } else {
            self.discard_mode_rollback(&ack.client_mutation_id);
        }

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
            self.state_minis[index] = merged_state_mini(&self.state_minis[index], session);
        } else {
            self.state_minis.push(session);
        }
        sort_state_minis(&mut self.state_minis);
    }

    fn apply_optimistic_mode(&mut self, thread_id: &str, preset: &str, client_mutation_id: &str) {
        self.remember_mode_rollback(thread_id, client_mutation_id);
        let mut did_update = false;
        for session in self
            .state_minis
            .iter_mut()
            .filter(|session| session.session_id == thread_id)
        {
            if let Some(payload_json) = optimistic_mode_payload_json(&session.payload_json, preset)
            {
                session.payload_json = payload_json;
                did_update = true;
            }
        }
        if did_update {
            self.last_error.clear();
        }
    }

    fn remember_mode_rollback(&mut self, thread_id: &str, client_mutation_id: &str) {
        if self
            .mode_rollbacks
            .iter()
            .any(|rollback| rollback.client_mutation_id == client_mutation_id)
        {
            return;
        }

        self.mode_rollbacks.push(ClientModeRollback {
            client_mutation_id: client_mutation_id.to_owned(),
            thread_id: thread_id.to_owned(),
            sessions: self
                .state_minis
                .iter()
                .filter(|session| session.session_id == thread_id)
                .cloned()
                .collect(),
        });
    }

    fn restore_mode_rollback(&mut self, client_mutation_id: &str) {
        let Some(index) = self
            .mode_rollbacks
            .iter()
            .position(|rollback| rollback.client_mutation_id == client_mutation_id)
        else {
            return;
        };
        let rollback = self.mode_rollbacks.remove(index);
        self.state_minis
            .retain(|session| session.session_id != rollback.thread_id);
        for session in rollback.sessions {
            self.upsert_state_mini(session);
        }
        sort_state_minis(&mut self.state_minis);
    }

    fn discard_mode_rollback(&mut self, client_mutation_id: &str) {
        self.mode_rollbacks
            .retain(|rollback| rollback.client_mutation_id != client_mutation_id);
    }

    fn apply_state_mini_delta(&mut self, delta: ClientStateMiniDelta) -> bool {
        if delta.seq <= self.latest_seq {
            return false;
        }

        if is_state_mini_replacement_delta(&delta) {
            self.state_minis = normalize_state_minis(delta.sessions);
        } else {
            if delta.has_session {
                self.upsert_state_mini(delta.session);
            }
            for session in delta.sessions {
                self.upsert_state_mini(session);
            }
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

fn is_state_mini_replacement_delta(delta: &ClientStateMiniDelta) -> bool {
    !delta.has_session && delta.kind == STATE_MINI_REPLACEMENT_KIND
}

#[cfg(test)]
fn select_endpoint(endpoints: &[ClientEndpoint]) -> Result<ClientEndpoint, ClientCoreError> {
    let endpoint = endpoints
        .iter()
        .find(|endpoint| endpoint.last_good)
        .or_else(|| endpoints.first())
        .ok_or(ClientCoreError::NoEndpoint)?;
    Ok(endpoint.clone())
}

fn require_endpoints(endpoints: &[ClientEndpoint]) -> Result<(), ClientCoreError> {
    if endpoints.is_empty() {
        return Err(ClientCoreError::NoEndpoint);
    }
    Ok(())
}

fn endpoints_identity(endpoints: &[ClientEndpoint]) -> String {
    let mut endpoint_urls = endpoints
        .iter()
        .map(|endpoint| endpoint.url.trim().trim_end_matches('/').to_owned())
        .filter(|endpoint_url| !endpoint_url.is_empty())
        .collect::<Vec<_>>();
    endpoint_urls.sort();
    endpoint_urls.dedup();
    endpoint_urls.join("\n")
}

fn restored_outbound_frame(
    command: ClientPendingCommand,
) -> Result<OutboundSessionFrame, ClientCoreError> {
    require_present(
        &command.client_mutation_id,
        ClientCoreError::EmptyMutationId,
    )?;
    validate_restored_command(&command)?;
    let command_kind = restored_command_kind(command.kind);
    let thread_id = restored_thread_id(&command)?;
    let prompt_intent = if command.kind == ClientPendingCommandKind::SendSessionPrompt {
        normalized_prompt_intent(command.prompt_intent)?
    } else {
        String::new()
    };

    Ok(OutboundSessionFrame {
        frame_kind: OutboundSessionFrameKind::Command,
        command_kind,
        thread_id,
        preset: command.preset,
        prompt: command.prompt,
        prompt_intent,
        assistant_surface: command.assistant_surface,
        notification_id: command.notification_id,
        archived: command.archived,
        client_mutation_id: command.client_mutation_id,
        after_seq: EMPTY_SEQUENCE,
    })
}

fn validate_restored_command(command: &ClientPendingCommand) -> Result<(), ClientCoreError> {
    match command.kind {
        ClientPendingCommandKind::SendSessionPrompt => {
            require_present(&command.prompt, ClientCoreError::EmptyPrompt)?;
        }
        ClientPendingCommandKind::SubmitNotificationReply => {
            require_present(
                &command.notification_id,
                ClientCoreError::EmptyNotificationId,
            )?;
            require_present(&command.prompt, ClientCoreError::EmptyPrompt)?;
        }
        ClientPendingCommandKind::SetAssistantSurface => {
            require_present(&command.assistant_surface, ClientCoreError::EmptySessionId)?;
        }
        ClientPendingCommandKind::SaveDefaultPrompt => {
            require_present(&command.prompt, ClientCoreError::EmptyPrompt)?;
        }
        ClientPendingCommandKind::SetSessionMode
        | ClientPendingCommandKind::SetSiriCurrentSession
        | ClientPendingCommandKind::SetSiriDefaultSession
        | ClientPendingCommandKind::SetSessionArchived
        | ClientPendingCommandKind::DeleteSession
        | ClientPendingCommandKind::MuteSession => {}
    }
    Ok(())
}

fn restored_command_kind(kind: ClientPendingCommandKind) -> ClientCommandKind {
    match kind {
        ClientPendingCommandKind::SetSessionMode => ClientCommandKind::SetSessionMode,
        ClientPendingCommandKind::SendSessionPrompt => ClientCommandKind::SendSessionPrompt,
        ClientPendingCommandKind::SubmitNotificationReply => {
            ClientCommandKind::SubmitNotificationReply
        }
        ClientPendingCommandKind::SetAssistantSurface => ClientCommandKind::SetAssistantSurface,
        ClientPendingCommandKind::SetSiriCurrentSession => ClientCommandKind::SetSiriCurrentSession,
        ClientPendingCommandKind::SetSiriDefaultSession => ClientCommandKind::SetSiriDefaultSession,
        ClientPendingCommandKind::SaveDefaultPrompt => ClientCommandKind::SaveDefaultPrompt,
        ClientPendingCommandKind::SetSessionArchived => ClientCommandKind::SetSessionArchived,
        ClientPendingCommandKind::DeleteSession => ClientCommandKind::DeleteSession,
        ClientPendingCommandKind::MuteSession => ClientCommandKind::MuteSession,
    }
}

fn restored_thread_id(command: &ClientPendingCommand) -> Result<String, ClientCoreError> {
    match command.kind {
        ClientPendingCommandKind::SetAssistantSurface
        | ClientPendingCommandKind::SaveDefaultPrompt => Ok(MOBILE_SETTINGS_ENTITY_ID.to_owned()),
        ClientPendingCommandKind::SetSiriCurrentSession
        | ClientPendingCommandKind::SetSiriDefaultSession => Ok(command.thread_id.clone()),
        ClientPendingCommandKind::SetSessionMode
        | ClientPendingCommandKind::SendSessionPrompt
        | ClientPendingCommandKind::SubmitNotificationReply
        | ClientPendingCommandKind::SetSessionArchived
        | ClientPendingCommandKind::DeleteSession
        | ClientPendingCommandKind::MuteSession => {
            require_present(&command.thread_id, ClientCoreError::EmptyThreadId)?;
            Ok(command.thread_id.clone())
        }
    }
}

fn non_empty_unique(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn merged_state_mini(current: &ClientStateMini, incoming: ClientStateMini) -> ClientStateMini {
    ClientStateMini {
        session_id: incoming.session_id,
        assistant_surface: non_empty_or_current(
            incoming.assistant_surface,
            &current.assistant_surface,
        ),
        seq: incoming.seq.max(current.seq),
        revision: non_empty_or_current(incoming.revision, &current.revision),
        payload_json: merged_payload_json(&current.payload_json, &incoming.payload_json),
    }
}

fn non_empty_or_current(incoming: String, current: &str) -> String {
    if incoming.trim().is_empty() {
        current.to_owned()
    } else {
        incoming
    }
}

fn merged_payload_json(current: &str, incoming: &str) -> String {
    let Ok(mut current_value) = serde_json::from_str::<Value>(current) else {
        return incoming.to_owned();
    };
    let Ok(incoming_value) = serde_json::from_str::<Value>(incoming) else {
        return incoming.to_owned();
    };
    let (Some(current_object), Some(incoming_object)) =
        (current_value.as_object_mut(), incoming_value.as_object())
    else {
        return incoming.to_owned();
    };
    for (key, value) in incoming_object {
        current_object.insert(key.clone(), value.clone());
    }
    serde_json::to_string(&current_value).unwrap_or_else(|_| incoming.to_owned())
}

fn update_server_time_if_newer(current: &mut String, candidate: String) {
    if candidate.trim().is_empty() {
        return;
    }
    if current.is_empty() || candidate > *current {
        *current = candidate;
    }
}

fn reject_message(ack: &ClientCommandAck) -> String {
    match (ack.error_code.is_empty(), ack.reject_reason.is_empty()) {
        (false, false) => format!("{}: {}", ack.error_code, ack.reject_reason),
        (false, true) => ack.error_code.clone(),
        (true, false) => ack.reject_reason.clone(),
        (true, true) => "command rejected".to_owned(),
    }
}

fn optimistic_mode_payload_json(payload_json: &str, preset: &str) -> Option<String> {
    let mut payload = serde_json::from_str::<Value>(payload_json).ok()?;
    let payload_object = payload.as_object_mut()?;
    payload_object.insert("effectiveMode".to_owned(), optimistic_mode_value(preset));
    serde_json::to_string(&payload).ok()
}

fn optimistic_mode_value(preset: &str) -> Value {
    let trimmed = preset.trim();
    if trimmed.is_empty() {
        Value::Null
    } else {
        Value::from(trimmed)
    }
}

fn persist_state_minis_to_local_store(
    local_store: &LooperClientCoreLocalStore,
    snapshot: ClientStateSnapshot,
) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
    local_store.replace_state_minis(ClientStateMiniSnapshot {
        latest_seq: snapshot.latest_seq,
        sessions: snapshot.state_minis,
        server_time: snapshot.server_time,
    })
}

fn should_retry_notification_reply_drain(error: &ClientCoreError) -> bool {
    matches!(
        error,
        ClientCoreError::NoEndpoint
            | ClientCoreError::MissingCommandAcknowledgement
            | ClientCoreError::SessionCommandTransportFailed
            | ClientCoreError::SessionCommandAckTimedOut
    )
}

fn should_retry_command_flush(error: &ClientCoreError) -> bool {
    matches!(
        error,
        ClientCoreError::NoEndpoint
            | ClientCoreError::MissingCommandAcknowledgement
            | ClientCoreError::SessionCommandTransportFailed
            | ClientCoreError::SessionCommandAckTimedOut
            | ClientCoreError::StateMiniStreamNotRunning
    )
}

fn is_latest_wins_outbox_command(command_kind: ClientCommandKind) -> bool {
    matches!(command_kind, ClientCommandKind::SetAssistantSurface)
}

fn require_present(value: &str, error: ClientCoreError) -> Result<(), ClientCoreError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

fn normalized_prompt_intent(value: String) -> Result<String, ClientCoreError> {
    match value.trim() {
        "" | "queue" => Ok(default_prompt_intent()),
        "steer" => Ok("steer".to_owned()),
        _ => Err(ClientCoreError::InvalidPromptIntent),
    }
}

fn default_prompt_intent() -> String {
    "queue".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClientCommandAckEnvelope;
    use std::{path::PathBuf, sync::Arc};

    const ENDPOINT_PRIMARY: &str = "http://127.0.0.1:8765";
    const ENDPOINT_LAST_GOOD: &str = "http://100.64.0.2:8765";
    const SERVER_TIME: &str = "2026-06-25T00:00:02Z";
    const OBSERVE_TEST_TIMEOUT: Duration = Duration::from_secs(1);

    fn install_test_session_stream(
        core: &Arc<LooperClientCore>,
    ) -> (
        mpsc::Receiver<OutboundSessionFrame>,
        mpsc::Sender<ClientCommandAck>,
    ) {
        let endpoints_identity = endpoints_identity(&[ClientEndpoint {
            url: ENDPOINT_PRIMARY.to_owned(),
            last_good: false,
        }]);
        let (events_sender, events_receiver) = mpsc::channel(1);
        let (commands_sender, commands_receiver) = mpsc::channel(2);
        let (acks_sender, acks_receiver) = mpsc::channel(2);
        let task = core.runtime.spawn(async {
            std::future::pending::<()>().await;
        });
        *core.lock_stream().expect("stream lock") = Some(ClientCoreStream {
            task,
            receiver: Some(events_receiver),
            command_sender: commands_sender,
            command_ack_receiver: Some(acks_receiver),
            endpoints_identity,
        });
        drop(events_sender);
        (commands_receiver, acks_sender)
    }

    fn install_finished_test_session_stream(core: &Arc<LooperClientCore>) {
        let endpoints_identity = endpoints_identity(&[ClientEndpoint {
            url: ENDPOINT_PRIMARY.to_owned(),
            last_good: false,
        }]);
        let (_events_sender, events_receiver) = mpsc::channel(1);
        let (commands_sender, _commands_receiver) = mpsc::channel(2);
        let (_acks_sender, acks_receiver) = mpsc::channel(2);
        let (finished_sender, finished_receiver) = std::sync::mpsc::channel();
        let task = core.runtime.spawn(async move {
            let _ = finished_sender.send(());
        });
        core.runtime.block_on(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
        });
        finished_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("finished stream task");
        assert!(task.is_finished());
        *core.lock_stream().expect("stream lock") = Some(ClientCoreStream {
            task,
            receiver: Some(events_receiver),
            command_sender: commands_sender,
            command_ack_receiver: Some(acks_receiver),
            endpoints_identity,
        });
    }

    fn accepted_ack(client_mutation_id: &str, ack_seq: i64, revision: &str) -> ClientCommandAck {
        ClientCommandAck {
            accepted: true,
            client_mutation_id: client_mutation_id.to_owned(),
            ack_seq,
            entity_id: "thread-1".to_owned(),
            revision: revision.to_owned(),
            server_time: format!("2026-06-25T00:00:{ack_seq:02}Z"),
            idempotent_replay: false,
            error_code: String::new(),
            reject_reason: String::new(),
            current_state: String::new(),
        }
    }

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
    fn start_state_mini_stream_keeps_running_stream_for_same_endpoint() {
        let core = LooperClientCore::new();
        let (mut commands_receiver, _acks_sender) = install_test_session_stream(&core);
        {
            let mut state = core.lock_state().expect("state lock");
            state.phase = ConnectionPhase::Ready;
            state.endpoint_url = ENDPOINT_PRIMARY.to_owned();
        }

        let snapshot = core
            .start_state_mini_stream(
                vec![ClientEndpoint {
                    url: ENDPOINT_PRIMARY.to_owned(),
                    last_good: true,
                }],
                "token".to_owned(),
                "mobile-session".to_owned(),
            )
            .expect("idempotent start");

        assert_eq!(snapshot.phase, ConnectionPhase::Ready);
        assert_eq!(snapshot.endpoint_url, ENDPOINT_PRIMARY);

        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let outbox = core.take_outbox().expect("outbox");
            core.send_session_commands(outbox)
                .await
                .expect("send over retained stream");
            let frame = commands_receiver
                .recv()
                .await
                .expect("retained command frame");
            assert_eq!(frame.client_mutation_id, "cmid-prompt");
        });
    }

    #[test]
    fn start_keeps_live_endpoint_for_retained_stream_configuration() {
        let core = LooperClientCore::new();
        let (_commands_receiver, _acks_sender) = install_test_session_stream(&core);
        {
            let mut state = core.lock_state().expect("state lock");
            state.phase = ConnectionPhase::Ready;
            state.endpoint_url = ENDPOINT_LAST_GOOD.to_owned();
        }

        let snapshot = core
            .start(
                vec![ClientEndpoint {
                    url: ENDPOINT_PRIMARY.to_owned(),
                    last_good: false,
                }],
                "token".to_owned(),
                "mobile-session".to_owned(),
            )
            .expect("idempotent start");

        assert_eq!(snapshot.phase, ConnectionPhase::Ready);
        assert_eq!(snapshot.endpoint_url, ENDPOINT_LAST_GOOD);
    }

    #[test]
    fn start_state_mini_stream_replaces_finished_stream_for_same_endpoint() {
        let core = LooperClientCore::new();
        install_finished_test_session_stream(&core);
        {
            let mut state = core.lock_state().expect("state lock");
            state.phase = ConnectionPhase::Ready;
            state.endpoint_url = ENDPOINT_PRIMARY.to_owned();
        }

        let snapshot = core
            .start_state_mini_stream(
                vec![ClientEndpoint {
                    url: ENDPOINT_PRIMARY.to_owned(),
                    last_good: false,
                }],
                "token".to_owned(),
                "mobile-session".to_owned(),
            )
            .expect("replace finished stream");

        assert_eq!(snapshot.phase, ConnectionPhase::Connecting);
        assert_eq!(snapshot.endpoint_url, "");
        assert!(
            core.lock_stream()
                .expect("stream lock")
                .as_ref()
                .expect("stream")
                .is_running()
        );
    }

    #[test]
    fn heartbeat_updates_displayed_endpoint_to_live_session_route() {
        let core = LooperClientCore::new();
        core.connect(vec![ClientEndpoint {
            url: ENDPOINT_PRIMARY.to_owned(),
            last_good: true,
        }])
        .expect("connect primary");

        let update = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::Heartbeat {
                latest_seq: 12,
                server_time: SERVER_TIME.to_owned(),
                endpoint_url: ENDPOINT_LAST_GOOD.to_owned(),
            })
            .expect("heartbeat");

        assert_eq!(update.snapshot.phase, ConnectionPhase::Ready);
        assert_eq!(update.snapshot.endpoint_url, ENDPOINT_LAST_GOOD);
        assert_eq!(update.latest_seq, 12);
        assert_eq!(update.snapshot.latest_seq, 0);
        assert_eq!(update.snapshot.server_time, SERVER_TIME);
    }

    #[test]
    fn heartbeat_latest_seq_does_not_skip_replay_delta() {
        let core = LooperClientCore::new();
        core.connect(vec![ClientEndpoint {
            url: ENDPOINT_PRIMARY.to_owned(),
            last_good: true,
        }])
        .expect("connect primary");

        core.apply_state_mini_stream_event(StateMiniStreamEvent::Heartbeat {
            latest_seq: 12,
            server_time: SERVER_TIME.to_owned(),
            endpoint_url: ENDPOINT_LAST_GOOD.to_owned(),
        })
        .expect("heartbeat");

        let update = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::Delta(ClientStateMiniDelta {
                seq: 11,
                latest_seq: 11,
                entity_id: "thread-1".to_owned(),
                kind: "session.changed".to_owned(),
                revision: "rev-11".to_owned(),
                server_time: "2026-06-26T00:00:11Z".to_owned(),
                has_session: true,
                session: ClientStateMini {
                    session_id: "thread-1".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 11,
                    revision: "rev-11".to_owned(),
                    payload_json: r#"{"title":"Replay applied"}"#.to_owned(),
                },
                sessions: Vec::new(),
            }))
            .expect("replay delta");

        assert!(update.did_change);
        assert_eq!(update.snapshot.latest_seq, 11);
        assert_eq!(update.snapshot.state_minis.len(), 1);
        assert_eq!(update.snapshot.state_minis[0].session_id, "thread-1");
    }

    #[test]
    fn concurrent_state_mini_observers_are_serialized() {
        let core = LooperClientCore::new();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        runtime.block_on(async {
            let first_core = core.clone();
            let second_core = core.clone();
            let first = tokio::spawn(async move { first_core.observe().await });
            let second = tokio::spawn(async move { second_core.observe().await });

            let first_snapshot = core.snapshot().expect("first snapshot");
            core.emit_local_state_update(first_snapshot);
            let second_snapshot = core.snapshot().expect("second snapshot");
            core.emit_local_state_update(second_snapshot);

            let (first_result, second_result) =
                tokio::time::timeout(OBSERVE_TEST_TIMEOUT, async { tokio::join!(first, second) })
                    .await
                    .expect("serialized observers should both complete");

            assert_eq!(
                first_result
                    .expect("first observer task")
                    .expect("first observer")
                    .reason,
                ClientStateMiniStreamUpdateReason::Delta
            );
            assert_eq!(
                second_result
                    .expect("second observer task")
                    .expect("second observer")
                    .reason,
                ClientStateMiniStreamUpdateReason::Delta
            );
        });
    }

    #[test]
    fn cancelled_state_mini_observer_restores_receivers() {
        let core = LooperClientCore::new();
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let (_commands_receiver, _acks_sender) = install_test_session_stream(&core);

        runtime.block_on(async {
            let observer_core = core.clone();
            let observer = tokio::spawn(async move { observer_core.observe().await });
            tokio::time::sleep(Duration::from_millis(10)).await;
            observer.abort();
            let _ = observer.await;

            let snapshot = core.snapshot().expect("snapshot");
            core.emit_local_state_update(snapshot);
            let update = tokio::time::timeout(OBSERVE_TEST_TIMEOUT, core.observe())
                .await
                .expect("observer should recover after cancellation")
                .expect("observe should not lose receivers");

            assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Delta);
        });
    }

    #[test]
    fn notification_reply_drain_retries_only_transient_failures() {
        assert!(should_retry_notification_reply_drain(
            &ClientCoreError::NoEndpoint
        ));
        assert!(should_retry_notification_reply_drain(
            &ClientCoreError::SessionCommandTransportFailed
        ));
        assert!(should_retry_notification_reply_drain(
            &ClientCoreError::SessionCommandAckTimedOut
        ));
        assert!(should_retry_notification_reply_drain(
            &ClientCoreError::MissingCommandAcknowledgement
        ));
        assert!(!should_retry_notification_reply_drain(
            &ClientCoreError::EmptyPrompt
        ));
        assert!(!should_retry_notification_reply_drain(
            &ClientCoreError::EmptyNotificationId
        ));
    }

    #[test]
    fn command_flush_retries_only_transient_failures() {
        assert!(should_retry_command_flush(&ClientCoreError::NoEndpoint));
        assert!(should_retry_command_flush(
            &ClientCoreError::SessionCommandTransportFailed
        ));
        assert!(should_retry_command_flush(
            &ClientCoreError::SessionCommandAckTimedOut
        ));
        assert!(should_retry_command_flush(
            &ClientCoreError::MissingCommandAcknowledgement
        ));
        assert!(should_retry_command_flush(
            &ClientCoreError::StateMiniStreamNotRunning
        ));
        assert!(!should_retry_command_flush(&ClientCoreError::EmptyPrompt));
        assert!(!should_retry_command_flush(&ClientCoreError::EmptyThreadId));
    }

    #[test]
    fn command_ack_receiver_buffers_unexpected_acks() {
        let core = LooperClientCore::new();
        let (_commands_receiver, acks_sender) = install_test_session_stream(&core);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        runtime.block_on(async {
            acks_sender
                .send(accepted_ack("cmid-later", 43, "rev-43"))
                .await
                .expect("send later ack");
            acks_sender
                .send(accepted_ack("cmid-current", 42, "rev-42"))
                .await
                .expect("send current ack");

            let current = core
                .recv_command_acks(vec!["cmid-current".to_owned()], 1)
                .await
                .expect("current ack");
            assert_eq!(current.len(), 1);
            assert_eq!(current[0].client_mutation_id, "cmid-current");

            let later = core
                .recv_command_acks(vec!["cmid-later".to_owned()], 1)
                .await
                .expect("buffered later ack");
            assert_eq!(later.len(), 1);
            assert_eq!(later[0].client_mutation_id, "cmid-later");
        });
    }

    #[test]
    fn notification_reply_drain_lifecycle_is_serialized_in_rust_core() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("notification-drain-serialized");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        runtime.block_on(async {
            let drain_guard = core.notification_reply_drain.lock().await;
            let drain = core.drain_notification_reply_outbox_durable(store.clone());
            tokio::pin!(drain);

            tokio::select! {
                result = &mut drain => {
                    panic!("drain completed while Rust drain lock was held: {result:?}");
                }
                _ = sleep(Duration::from_millis(10)) => {}
            }

            drop(drain_guard);
            let result = drain
                .await
                .expect_err("empty outbox rejects after lock release");
            assert_eq!(result, ClientCoreError::NoPendingNotificationReply);
        });
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
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let prompt_outbox = core
            .take_expected_outbox(vec!["cmid-prompt".to_owned()])
            .expect("matching prompt outbox");
        assert_eq!(prompt_outbox.len(), 1);
        assert_eq!(prompt_outbox[0].client_mutation_id, "cmid-prompt");
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 1);

        let missing = core
            .take_expected_outbox(vec!["cmid-prompt".to_owned()])
            .expect_err("delivered mutation is no longer pending");
        assert_eq!(missing, ClientCoreError::UnexpectedOutboxMutations);

        let outbox = core
            .take_expected_outbox(vec!["cmid-mode".to_owned()])
            .expect("remaining mode outbox");

        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].client_mutation_id, "cmid-mode");
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 0);
    }

    #[test]
    fn pending_outbox_client_mutation_ids_preserve_order() {
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
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        assert_eq!(
            core.pending_outbox_client_mutation_ids()
                .expect("pending ids"),
            vec!["cmid-mode".to_owned(), "cmid-prompt".to_owned()]
        );
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 2);
    }

    #[test]
    fn duplicate_client_mutation_replaces_queued_command() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "first".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue first");
        core.send_prompt(
            "thread-1".to_owned(),
            "second".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("replace retry");

        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 1);
        assert_eq!(snapshot.pending_mutations.len(), 1);

        let outbox = core.take_outbox().expect("outbox");
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].prompt, "second");
        assert_eq!(outbox[0].client_mutation_id, "cmid-prompt");
    }

    #[test]
    fn assistant_surface_outbox_is_latest_wins() {
        let core = LooperClientCore::new();
        core.set_assistant_surface("claude-code".to_owned(), "cmid-claude".to_owned())
            .expect("queue claude");
        core.set_assistant_surface("devin".to_owned(), "cmid-devin".to_owned())
            .expect("queue devin");
        core.set_assistant_surface("grok-build".to_owned(), "cmid-grok".to_owned())
            .expect("queue grok");

        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 1);
        assert_eq!(snapshot.pending_mutations.len(), 1);
        assert_eq!(
            snapshot.pending_mutations[0].client_mutation_id,
            "cmid-grok"
        );

        let outbox = core.take_outbox().expect("outbox");
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].client_mutation_id, "cmid-grok");
        assert_eq!(outbox[0].assistant_surface, "grok-build");
    }

    #[test]
    fn superseded_assistant_surface_ack_preserves_newer_outbox() {
        let core = LooperClientCore::new();
        core.set_assistant_surface("claude-code".to_owned(), "cmid-claude".to_owned())
            .expect("queue claude");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let (mut commands_receiver, acks_sender) = install_test_session_stream(&core);

        runtime.block_on(async {
            let submit_core = core.clone();
            let submit_task = tokio::spawn(async move {
                submit_core
                    .submit_expected_outbox(vec!["cmid-claude".to_owned()])
                    .await
            });
            let frame = commands_receiver.recv().await.expect("command frame");
            assert_eq!(frame.client_mutation_id, "cmid-claude");
            assert_eq!(frame.command_kind, ClientCommandKind::SetAssistantSurface);

            core.set_assistant_surface("grok-build".to_owned(), "cmid-grok".to_owned())
                .expect("queue newer surface while old ack is pending");
            acks_sender
                .send(accepted_ack("cmid-claude", 42, "rev-42"))
                .await
                .expect("send old ack");

            let response = submit_task
                .await
                .expect("submit task")
                .expect("superseded latest-wins ack is still valid");
            assert!(response.accepted);
        });

        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 1);
        assert_eq!(snapshot.pending_mutations.len(), 1);
        assert_eq!(
            snapshot.pending_mutations[0].client_mutation_id,
            "cmid-grok"
        );

        let outbox = core
            .take_expected_outbox(vec!["cmid-grok".to_owned()])
            .expect("newer surface remains pending");
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].assistant_surface, "grok-build");
    }

    #[test]
    fn submit_expected_outbox_keeps_commands_queued_when_transport_fails() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let error = runtime
            .block_on(core.submit_expected_outbox(vec!["cmid-prompt".to_owned()]))
            .expect_err("missing endpoint rejects");

        assert_eq!(error, ClientCoreError::NoEndpoint);
        assert_eq!(core.snapshot().expect("snapshot").outbox_depth, 1);
        assert_eq!(
            core.snapshot().expect("snapshot").pending_mutations.len(),
            1
        );
    }

    #[test]
    fn flush_pending_outbox_retries_until_stream_is_available() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("flush-retry-stream");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");
        core.set_assistant_surface("grok-build".to_owned(), "cmid-surface".to_owned())
            .expect("queue assistant surface");
        store
            .enqueue_set_assistant_surface_command(
                "grok-build".to_owned(),
                "cmid-surface".to_owned(),
            )
            .expect("persist assistant surface");
        store
            .mark_attempted("cmid-surface".to_owned())
            .expect("mark initial attempt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let install_core = core.clone();
        runtime.block_on(async {
            let ack_task = tokio::spawn(async move {
                sleep(Duration::from_millis(50)).await;
                let (mut commands_receiver, acks_sender) =
                    install_test_session_stream(&install_core);
                let frame = commands_receiver.recv().await.expect("retried command");
                assert_eq!(frame.client_mutation_id, "cmid-surface");
                assert_eq!(frame.command_kind, ClientCommandKind::SetAssistantSurface);
                assert_eq!(frame.assistant_surface, "grok-build");
                acks_sender
                    .send(accepted_ack("cmid-surface", 44, "rev-44"))
                    .await
                    .expect("send ack");
            });

            core.flush_pending_outbox_with_retries(
                store.clone(),
                vec!["cmid-surface".to_owned()],
                false,
            )
            .await
            .expect("flush retries after endpoint appears");
            ack_task.await.expect("ack task");
        });

        let local_snapshot = store.snapshot().expect("local snapshot");
        assert!(local_snapshot.pending_commands.is_empty());
        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 0);
        assert_eq!(snapshot.latest_seq, 44);
        assert_eq!(snapshot.revision, "rev-44");
    }

    #[test]
    fn submit_expected_outbox_uses_existing_session_stream() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let (mut commands_receiver, acks_sender) = install_test_session_stream(&core);

        runtime.block_on(async {
            let ack_task = tokio::spawn(async move {
                let frame = commands_receiver.recv().await.expect("command frame");
                assert_eq!(frame.client_mutation_id, "cmid-prompt");
                assert_eq!(frame.command_kind, ClientCommandKind::SendSessionPrompt);
                acks_sender
                    .send(accepted_ack("cmid-prompt", 42, "rev-42"))
                    .await
                    .expect("send ack");
            });

            let response = core
                .submit_expected_outbox(vec!["cmid-prompt".to_owned()])
                .await
                .expect("submit over existing stream");
            ack_task.await.expect("ack task");
            assert!(response.accepted);
        });

        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 0);
        assert!(snapshot.pending_mutations.is_empty());
        assert_eq!(snapshot.latest_seq, 42);
        assert_eq!(snapshot.revision, "rev-42");
        assert_eq!(snapshot.server_time, "2026-06-25T00:00:42Z");
    }

    #[test]
    fn submit_expected_outbox_keeps_commands_queued_while_ack_is_pending() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let (mut commands_receiver, acks_sender) = install_test_session_stream(&core);

        runtime.block_on(async {
            let submit_core = core.clone();
            let submit_task = tokio::spawn(async move {
                submit_core
                    .submit_expected_outbox(vec!["cmid-prompt".to_owned()])
                    .await
            });
            let frame = commands_receiver.recv().await.expect("command frame");
            assert_eq!(frame.client_mutation_id, "cmid-prompt");
            assert_eq!(frame.command_kind, ClientCommandKind::SendSessionPrompt);

            core.set_mode(
                "thread-1".to_owned(),
                "await-reply".to_owned(),
                "cmid-mode".to_owned(),
            )
            .expect("queue newer mode while prompt ack is pending");
            acks_sender
                .send(accepted_ack("cmid-prompt", 42, "rev-42"))
                .await
                .expect("send prompt ack");

            let response = submit_task
                .await
                .expect("submit task")
                .expect("submit over existing stream");
            assert!(response.accepted);
            assert_eq!(response.command_acks.len(), 1);
        });

        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 1);
        assert_eq!(snapshot.pending_mutations.len(), 1);
        assert_eq!(
            snapshot.pending_mutations[0].client_mutation_id,
            "cmid-mode"
        );
        assert_eq!(snapshot.latest_seq, 42);
        assert_eq!(snapshot.revision, "rev-42");
    }

    #[test]
    fn submit_expected_outbox_drains_batch_when_acks_arrive_out_of_order() {
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
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("queue prompt");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let (mut commands_receiver, acks_sender) = install_test_session_stream(&core);

        runtime.block_on(async {
            let ack_task = tokio::spawn(async move {
                let mode_frame = commands_receiver.recv().await.expect("mode frame");
                let prompt_frame = commands_receiver.recv().await.expect("prompt frame");
                assert_eq!(mode_frame.client_mutation_id, "cmid-mode");
                assert_eq!(mode_frame.command_kind, ClientCommandKind::SetSessionMode);
                assert_eq!(prompt_frame.client_mutation_id, "cmid-prompt");
                assert_eq!(
                    prompt_frame.command_kind,
                    ClientCommandKind::SendSessionPrompt
                );

                acks_sender
                    .send(accepted_ack("cmid-prompt", 43, "rev-43"))
                    .await
                    .expect("send prompt ack");
                acks_sender
                    .send(accepted_ack("cmid-mode", 42, "rev-42"))
                    .await
                    .expect("send mode ack");
            });

            let response = core
                .submit_expected_outbox(vec!["cmid-mode".to_owned(), "cmid-prompt".to_owned()])
                .await
                .expect("submit over existing stream");
            ack_task.await.expect("ack task");
            assert!(response.accepted);
            assert_eq!(response.command_acks.len(), 2);
        });

        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 0);
        assert!(snapshot.pending_mutations.is_empty());
        assert_eq!(snapshot.latest_seq, 43);
        assert_eq!(snapshot.revision, "rev-43");
        assert_eq!(snapshot.server_time, "2026-06-25T00:00:43Z");
    }

    #[test]
    fn durable_prompt_retry_dedupes_local_store_and_core_outbox() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("durable-prompt-retry");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");

        for _ in 0..2 {
            core.accept_send_prompt_durable(
                store.clone(),
                "thread-1".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "queue".to_owned(),
                "cmid-prompt".to_owned(),
            )
            .expect("local prompt accepted");
        }

        let snapshot = store.snapshot().expect("store snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "cmid-prompt"
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 2);
        assert_eq!(core.snapshot().expect("core snapshot").outbox_depth, 1);
    }

    #[test]
    fn submit_intent_queues_before_missing_runtime_error() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("durable-mode-missing-runtime");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");

        core.accept_set_mode_durable(
            store.clone(),
            "thread-1".to_owned(),
            "await-reply".to_owned(),
            "cmid-mode".to_owned(),
        )
        .expect("local mode accepted");
        let snapshot = core.snapshot().expect("snapshot");
        assert_eq!(snapshot.outbox_depth, 1);
        assert_eq!(snapshot.pending_mutations.len(), 1);
        assert_eq!(
            snapshot.pending_mutations[0].command_kind,
            ClientCommandKind::SetSessionMode
        );
        let local_snapshot = store.snapshot().expect("store snapshot");
        assert_eq!(local_snapshot.pending_commands.len(), 1);
        assert_eq!(local_snapshot.pending_commands[0].attempt_count, 1);
    }

    #[test]
    fn durable_mode_command_emits_local_state_update_before_transport() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("durable-mode-local-update");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 11,
            sessions: vec![state_mini("thread-1", "codex", 11, "rev-11", "await-reply")],
            server_time: SERVER_TIME.to_owned(),
        })
        .expect("seed minis");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        core.accept_set_mode_durable(
            store,
            "thread-1".to_owned(),
            "max-turns-2".to_owned(),
            "cmid-mode".to_owned(),
        )
        .expect("local mode accepted");

        let update = runtime
            .block_on(core.observe())
            .expect("local update before transport ack");
        assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Delta);
        assert!(update.did_change);
        assert_eq!(update.snapshot.outbox_depth, 1);
        let updated_payload: Value =
            serde_json::from_str(&update.snapshot.state_minis[0].payload_json)
                .expect("payload json");
        assert_eq!(updated_payload["effectiveMode"], "max-turns-2");
    }

    #[test]
    fn durable_prompt_command_emits_local_outbox_update_before_transport() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("durable-prompt-local-update");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        core.accept_send_prompt_durable(
            store,
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
            "cmid-prompt".to_owned(),
        )
        .expect("local prompt accepted");

        let update = runtime
            .block_on(core.observe())
            .expect("local update before transport ack");
        assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Delta);
        assert!(update.did_change);
        assert_eq!(update.snapshot.outbox_depth, 1);
        assert_eq!(update.snapshot.pending_mutations.len(), 1);
        assert_eq!(
            update.snapshot.pending_mutations[0].command_kind,
            ClientCommandKind::SendSessionPrompt
        );
    }

    #[test]
    fn durable_notification_reply_emits_local_outbox_update_before_transport() {
        let core = LooperClientCore::new();
        let store_path = temp_store_path("durable-reply-local-update");
        let store = LooperClientCoreLocalStore::new(store_path.to_string_lossy().into_owned())
            .expect("store");
        let runtime = tokio::runtime::Runtime::new().expect("runtime");

        core.accept_notification_reply_durable(
            store,
            "notification-1".to_owned(),
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "cmid-reply".to_owned(),
        )
        .expect("local reply accepted");

        let update = runtime
            .block_on(core.observe())
            .expect("local update before transport ack");
        assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Delta);
        assert!(update.did_change);
        assert_eq!(update.snapshot.outbox_depth, 1);
        assert_eq!(update.snapshot.pending_mutations.len(), 1);
        assert_eq!(
            update.snapshot.pending_mutations[0].command_kind,
            ClientCommandKind::SubmitNotificationReply
        );
    }

    #[test]
    fn recover_state_mini_snapshot_rejects_missing_endpoint_without_state_change() {
        let core = LooperClientCore::new();

        let error = core
            .recover_state_mini_snapshot(Vec::new(), String::new(), String::new())
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
    fn state_mini_stream_merges_compact_delta_into_cached_mini() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 5,
            sessions: vec![ClientStateMini {
                session_id: "thread-1".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 5,
                revision: "rev-5".to_owned(),
                payload_json: r#"{"id":"thread-1","sessionId":"thread-1","assistantSurface":"codex","ref":"T1","title":"Reduce latency","status":"waiting","lastUpdatedAt":"2026-06-25T00:00:00Z"}"#.to_owned(),
            }],
            server_time: String::new(),
        })
        .expect("seed minis");

        let update = core
            .apply_state_mini_stream_event(StateMiniStreamEvent::Delta(ClientStateMiniDelta {
                seq: 6,
                latest_seq: 6,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-6".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: true,
                session: ClientStateMini {
                    session_id: "thread-1".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 6,
                    revision: "rev-6".to_owned(),
                    payload_json: r#"{"sessionId":"thread-1","assistantSurface":"codex","effectiveMode":"max-turns-1"}"#.to_owned(),
                },
                sessions: Vec::new(),
            }))
            .expect("stream update");

        let payload: Value =
            serde_json::from_str(&update.snapshot.state_minis[0].payload_json).expect("payload");
        assert!(update.did_change);
        assert_eq!(payload["title"], "Reduce latency");
        assert_eq!(payload["ref"], "T1");
        assert_eq!(payload["status"], "waiting");
        assert_eq!(payload["effectiveMode"], "max-turns-1");
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
    fn state_mini_replacement_batch_replaces_missing_sessions() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 10,
            sessions: vec![
                state_mini("thread-codex", "codex", 8, "rev-8", "old codex"),
                state_mini("thread-devin", "devin", 9, "rev-9", "devin"),
            ],
            server_time: SERVER_TIME.to_owned(),
        })
        .expect("seed minis");

        let result = core
            .apply_state_mini_delta_with_result(ClientStateMiniDelta {
                seq: 11,
                latest_seq: 11,
                entity_id: "mobile".to_owned(),
                kind: STATE_MINI_REPLACEMENT_KIND.to_owned(),
                revision: "rev-11".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: false,
                session: state_mini("", "", 0, "", ""),
                sessions: vec![state_mini(
                    "thread-codex",
                    "codex",
                    11,
                    "rev-11",
                    "new codex",
                )],
            })
            .expect("apply stream batch");

        assert!(result.did_change);
        assert_eq!(result.snapshot.state_minis.len(), 1);
        assert_eq!(result.snapshot.state_minis[0].session_id, "thread-codex");
        assert_eq!(result.snapshot.state_minis[0].assistant_surface, "codex");
        assert!(
            result.snapshot.state_minis[0]
                .payload_json
                .contains("new codex")
        );
    }

    #[test]
    fn accepted_ack_reconciles_pending_mutation() {
        let core = LooperClientCore::new();
        core.send_prompt(
            "thread-1".to_owned(),
            "continue".to_owned(),
            "codex".to_owned(),
            "queue".to_owned(),
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
        assert_eq!(snapshot.server_time, "2026-06-25T00:00:00Z");
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
        assert_eq!(snapshot.revision, "rev-43");
        assert!(snapshot.pending_mutations.is_empty());
        assert_eq!(
            snapshot.last_error,
            "illegal_transition: WAIT_REPLY required"
        );
    }

    #[test]
    fn rejected_mode_ack_restores_optimistic_state_mini() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 7,
            sessions: vec![ClientStateMini {
                session_id: "thread-1".to_owned(),
                assistant_surface: "codex".to_owned(),
                seq: 7,
                revision: "rev-7".to_owned(),
                payload_json: r#"{"sessionId":"thread-1","assistantSurface":"codex","effectiveMode":"await-reply"}"#.to_owned(),
            }],
            server_time: SERVER_TIME.to_owned(),
        })
        .expect("seed mini");

        let optimistic = core
            .set_mode(
                "thread-1".to_owned(),
                "max-turns-2".to_owned(),
                "cmid-mode".to_owned(),
            )
            .expect("queue mode");
        assert!(
            optimistic.state_minis[0]
                .payload_json
                .contains(r#""effectiveMode":"max-turns-2""#)
        );

        let rejected = core
            .apply_command_ack(ClientCommandAck {
                accepted: false,
                client_mutation_id: "cmid-mode".to_owned(),
                ack_seq: 8,
                entity_id: "thread-1".to_owned(),
                revision: "rev-8".to_owned(),
                server_time: "2026-06-25T00:00:01Z".to_owned(),
                idempotent_replay: false,
                error_code: "illegal_transition".to_owned(),
                reject_reason: "mode rejected".to_owned(),
                current_state: "done".to_owned(),
            })
            .expect("ack");

        assert!(
            rejected.state_minis[0]
                .payload_json
                .contains(r#""effectiveMode":"await-reply""#)
        );
        assert!(rejected.pending_mutations.is_empty());
        assert_eq!(rejected.last_error, "illegal_transition: mode rejected");
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
            "queue".to_owned(),
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
        assert_eq!(snapshot.server_time, "2026-06-25T00:00:01Z");
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
    fn state_mini_replacement_delta_can_clear_local_projection() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 5,
            sessions: vec![state_mini("thread-1", "codex", 5, "rev-5", "old")],
            server_time: String::new(),
        })
        .expect("seed minis");

        let result = core
            .apply_state_mini_delta_with_result(ClientStateMiniDelta {
                seq: 6,
                latest_seq: 6,
                entity_id: "mobile".to_owned(),
                kind: STATE_MINI_REPLACEMENT_KIND.to_owned(),
                revision: "rev-6".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: false,
                session: state_mini("", "", 0, "", ""),
                sessions: vec![],
            })
            .expect("apply replacement");

        assert!(result.did_change);
        assert_eq!(result.snapshot.latest_seq, 6);
        assert!(result.snapshot.state_minis.is_empty());
        assert_eq!(result.snapshot.revision, "rev-6");
    }

    #[test]
    fn state_mini_batch_delta_merges_without_clearing_existing_sessions() {
        let core = LooperClientCore::new();
        core.replace_state_minis(ClientStateMiniSnapshot {
            latest_seq: 5,
            sessions: vec![
                state_mini("thread-1", "codex", 5, "rev-5", "old"),
                state_mini("thread-2", "codex", 4, "rev-4", "kept"),
            ],
            server_time: String::new(),
        })
        .expect("seed minis");

        let result = core
            .apply_state_mini_delta_with_result(ClientStateMiniDelta {
                seq: 6,
                latest_seq: 6,
                entity_id: "thread-1".to_owned(),
                kind: "session_mini".to_owned(),
                revision: "rev-6".to_owned(),
                server_time: SERVER_TIME.to_owned(),
                has_session: false,
                session: state_mini("", "", 0, "", ""),
                sessions: vec![state_mini("thread-1", "codex", 6, "rev-6", "new")],
            })
            .expect("apply batch delta");

        assert!(result.did_change);
        assert_eq!(result.snapshot.latest_seq, 6);
        assert_eq!(
            result
                .snapshot
                .state_minis
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["thread-2", "thread-1"]
        );
        let thread_1 = result
            .snapshot
            .state_minis
            .iter()
            .find(|session| session.session_id == "thread-1")
            .expect("updated thread");
        let thread_2 = result
            .snapshot
            .state_minis
            .iter()
            .find(|session| session.session_id == "thread-2")
            .expect("preserved thread");
        assert_eq!(thread_1.payload_json, r#"{"title":"new"}"#);
        assert_eq!(thread_2.payload_json, r#"{"title":"kept"}"#);
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

    fn temp_store_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join("looper-client-core-client-tests")
            .join(format!("{name}-{}", std::process::id()))
            .join(crate::local_store::DEFAULT_LOCAL_STORE_FILE_NAME);
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
        path
    }
}

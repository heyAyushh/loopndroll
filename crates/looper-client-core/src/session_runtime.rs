use std::sync::Arc;

use crate::{
    client::LooperClientCore,
    error::ClientCoreError,
    local_store::LooperClientCoreLocalStore,
    menu_snapshot::{
        ClientMenuBarSessionMiniLocalSnapshot, ClientMenuSnapshotStreamUpdate,
        reduce_state_minis_menu_snapshot,
    },
    mobile_snapshot::{
        ClientMobileSnapshot, reduce_state_minis_mobile_snapshot_with_pending_commands,
    },
    model::{
        ClientCommandAckEnvelope, ClientEndpoint, ClientLocalStateSnapshot,
        ClientLocalStateStreamUpdate, ClientMobileSnapshotStreamUpdate,
        ClientNotificationReplyIntentResult, ClientNotificationReplyPersistResult,
        ClientSessionCommandIntentResult, ClientSessionDetailProjection,
        ClientSessionModeIntentResult, ClientSessionPromptIntentResult, ClientStateMiniSnapshot,
        ClientStateMiniStreamUpdate, ClientStateMiniStreamUpdateReason, ClientStateSnapshot,
    },
};

const MOBILE_SYNC_REASON_DELTA: &str = "delta";
const MOBILE_SYNC_REASON_TEXT_CHUNK: &str = "text_chunk";
const MOBILE_SYNC_REASON_HEARTBEAT: &str = "heartbeat";
const MOBILE_SYNC_REASON_RECOVERY: &str = "recovery";
const MOBILE_SYNC_REASON_RECONNECTING: &str = "reconnecting";
const MODE_MUTATION_PREFIX: &str = "mode";
const PROMPT_MUTATION_PREFIX: &str = "prompt";
const NOTIFICATION_REPLY_MUTATION_PREFIX: &str = "notification-reply";
const SIRI_CURRENT_MUTATION_PREFIX: &str = "siri-current";
const SIRI_DEFAULT_MUTATION_PREFIX: &str = "siri-default";
const DEFAULT_PROMPT_MUTATION_PREFIX: &str = "default-prompt";
const DEFAULT_NOTIFICATION_TARGETS_MUTATION_PREFIX: &str = "default-notification-targets";
const ARCHIVE_MUTATION_PREFIX: &str = "archive";
const DELETE_MUTATION_PREFIX: &str = "delete";
const MUTE_MUTATION_PREFIX: &str = "mute";
const LOCAL_ACCEPTED_DISPATCH_KIND: &str = "accepted";
const EMPTY_LOCAL_REPLAY_SEQUENCE: i64 = 0;
#[derive(Debug, uniffi::Object)]
pub struct LooperClientCoreSessionRuntime {
    client_core: Arc<LooperClientCore>,
    local_store: Arc<LooperClientCoreLocalStore>,
}

#[uniffi::export]
impl LooperClientCoreSessionRuntime {
    #[uniffi::constructor]
    pub fn new(file_path: String) -> Result<Arc<Self>, ClientCoreError> {
        let runtime = Arc::new(Self {
            client_core: LooperClientCore::new(),
            local_store: LooperClientCoreLocalStore::new(file_path)?,
        });
        runtime.seed_core_from_local_store()?;
        Ok(runtime)
    }

    pub fn start(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientStateSnapshot, ClientCoreError> {
        let endpoints = self.local_store.endpoints_with_last_good(endpoints)?;
        let has_warm_stream = self.client_core.has_warm_stream_for_configuration(
            &endpoints,
            &bearer_token,
            &mobile_session_header,
        )?;
        let restored_client_mutation_ids = if has_warm_stream {
            self.restore_pending_commands_from_local_store()?
        } else {
            self.seed_core_from_local_store()?
        };
        let snapshot = self
            .client_core
            .start(endpoints, bearer_token, mobile_session_header)?;
        if !has_warm_stream {
            self.emit_cached_local_state(&snapshot)?;
        }
        self.client_core.spawn_restored_command_ack_flush(
            self.local_store.clone(),
            restored_client_mutation_ids,
        );
        Ok(snapshot)
    }

    pub fn stop(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        // No flush here: the persister already writes continuously with a
        // 250ms debounce / 2s max-staleness bound, so stream teardown needs
        // no durability barrier — and a flush waits FIFO behind every queued
        // write on the single persister thread, which measured 37s on a real
        // device during command-ack bursts and stalled every stream restart
        // behind it.
        self.client_core.stop()
    }

    /// Migrates the h3 transport's UDP socket after a device network-path
    /// change (Wi-Fi <-> LTE). QUIC connections survive the rebind, so a
    /// still-reachable stream keeps flowing seamlessly instead of paying a
    /// teardown + redial + `after_seq` catch-up. Returns `false` when there
    /// is no h3 endpoint to migrate. Cheap and non-blocking: a socket swap,
    /// no network round-trip.
    pub fn rebind_transport(&self) -> Result<bool, ClientCoreError> {
        crate::session_transport::rebind_h3_transport()
    }

    pub async fn observe(&self) -> Result<ClientStateMiniStreamUpdate, ClientCoreError> {
        let update = self.client_core.observe().await?;
        self.persist_last_good_endpoint(&update.snapshot)?;
        if update.has_text_chunk {
            self.local_store
                .apply_text_chunk(update.text_chunk.clone())?;
        }
        if update.did_change && !update.has_text_chunk {
            self.persist_core_snapshot(&update.snapshot)?;
        }
        self.client_core
            .spawn_pending_outbox_flush_after_live_transition(self.local_store.clone())?;
        Ok(update)
    }

    pub async fn observe_local_state_change(
        &self,
    ) -> Result<ClientLocalStateStreamUpdate, ClientCoreError> {
        loop {
            let update = self.observe().await?;
            match update.reason {
                ClientStateMiniStreamUpdateReason::Delta if update.did_change => {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::RecoveryRequired
                    if update.did_change || !update.error_description.is_empty() =>
                {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::Reconnecting
                    if !update.error_description.is_empty() =>
                {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::Heartbeat
                    if !update.snapshot.server_time.is_empty() =>
                {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::Stopped => {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::TextChunk => {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::Delta
                | ClientStateMiniStreamUpdateReason::Reconnecting
                | ClientStateMiniStreamUpdateReason::Heartbeat
                | ClientStateMiniStreamUpdateReason::RecoveryRequired => {}
            }
        }
    }

    pub async fn observe_mobile_snapshot_change(
        &self,
    ) -> Result<ClientMobileSnapshotStreamUpdate, ClientCoreError> {
        let update = self.observe_local_state_change().await?;
        self.mobile_snapshot_stream_update(update)
    }

    pub async fn observe_menu_snapshot_change(
        &self,
    ) -> Result<ClientMenuSnapshotStreamUpdate, ClientCoreError> {
        let update = self.observe_local_state_change().await?;
        self.menu_snapshot_stream_update(update)
    }

    pub fn state_snapshot(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.client_core.snapshot()
    }

    pub fn local_snapshot(&self) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.local_store.snapshot()
    }

    pub fn session_detail(
        &self,
        session_id: String,
    ) -> Result<ClientSessionDetailProjection, ClientCoreError> {
        self.local_store.session_detail(session_id)
    }

    pub async fn recover_state_mini_snapshot(
        &self,
        endpoints: Vec<ClientEndpoint>,
        bearer_token: String,
        mobile_session_header: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        let endpoints = self.local_store.endpoints_with_last_good(endpoints)?;
        let recovered = self
            .client_core
            .recover_state_mini_snapshot(endpoints, bearer_token, mobile_session_header)
            .await?;
        if recovered.did_change {
            self.local_store.mark_last_good_endpoint(
                recovered.endpoint_url.clone(),
                recovered.endpoint_transport,
            )?;
        }
        self.local_store.replace_state_minis(recovered.snapshot)
    }

    pub async fn set_mode(
        &self,
        thread_id: String,
        preset: String,
    ) -> Result<ClientSessionModeIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(MODE_MUTATION_PREFIX);
        self.client_core.accept_set_mode_durable(
            self.local_store.clone(),
            thread_id,
            preset.clone(),
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionModeIntentResult {
            accepted: true,
            preset,
            client_mutation_id,
        })
    }

    pub async fn send_prompt(
        &self,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        prompt_intent: String,
    ) -> Result<ClientSessionPromptIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(PROMPT_MUTATION_PREFIX);
        self.client_core.accept_send_prompt_durable(
            self.local_store.clone(),
            thread_id,
            prompt,
            assistant_surface,
            prompt_intent,
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionPromptIntentResult {
            accepted: true,
            dispatch_kind: LOCAL_ACCEPTED_DISPATCH_KIND.to_owned(),
            prompt_id: String::new(),
            client_mutation_id,
        })
    }

    pub async fn set_siri_current_session(
        &self,
        thread_id: String,
        assistant_surface: String,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(SIRI_CURRENT_MUTATION_PREFIX);
        self.client_core.accept_set_siri_current_session_durable(
            self.local_store.clone(),
            thread_id.clone(),
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: thread_id,
        })
    }

    pub async fn set_siri_default_session(
        &self,
        thread_id: String,
        assistant_surface: String,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(SIRI_DEFAULT_MUTATION_PREFIX);
        self.client_core.accept_set_siri_default_session_durable(
            self.local_store.clone(),
            thread_id.clone(),
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: thread_id,
        })
    }

    pub async fn save_default_prompt(
        &self,
        prompt: String,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(DEFAULT_PROMPT_MUTATION_PREFIX);
        self.client_core.accept_save_default_prompt_durable(
            self.local_store.clone(),
            prompt,
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: String::new(),
        })
    }

    pub async fn set_default_notification_targets(
        &self,
        notification_target_ids: Vec<String>,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id =
            generated_client_mutation_id(DEFAULT_NOTIFICATION_TARGETS_MUTATION_PREFIX);
        self.client_core
            .accept_set_default_notification_targets_durable(
                self.local_store.clone(),
                notification_target_ids,
                client_mutation_id.clone(),
            )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: String::new(),
        })
    }

    pub async fn set_session_archived(
        &self,
        thread_id: String,
        archived: bool,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(ARCHIVE_MUTATION_PREFIX);
        self.client_core.accept_set_session_archived_durable(
            self.local_store.clone(),
            thread_id.clone(),
            archived,
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: thread_id,
        })
    }

    pub async fn delete_session(
        &self,
        thread_id: String,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(DELETE_MUTATION_PREFIX);
        self.client_core.accept_delete_session_durable(
            self.local_store.clone(),
            thread_id.clone(),
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: thread_id,
        })
    }

    pub async fn mute_session(
        &self,
        thread_id: String,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(MUTE_MUTATION_PREFIX);
        self.client_core.accept_mute_session_durable(
            self.local_store.clone(),
            thread_id.clone(),
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: thread_id,
        })
    }

    pub async fn submit_notification_reply(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientNotificationReplyIntentResult, ClientCoreError> {
        self.client_core.accept_notification_reply_durable(
            self.local_store.clone(),
            notification_id.clone(),
            thread_id.clone(),
            prompt,
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        Ok(ClientNotificationReplyIntentResult {
            accepted: true,
            dispatch_kind: LOCAL_ACCEPTED_DISPATCH_KIND.to_owned(),
            prompt_id: String::new(),
            server_time: String::new(),
            client_mutation_id,
            ack_seq: 0,
            entity_id: thread_id,
            revision: String::new(),
            idempotent_replay: false,
            notification_id,
        })
    }

    pub async fn submit_notification_reply_with_generated_mutation(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
    ) -> Result<ClientNotificationReplyIntentResult, ClientCoreError> {
        let notification_id = notification_id.trim().to_owned();
        let client_mutation_id = notification_reply_client_mutation_id(&notification_id);
        self.submit_notification_reply(
            notification_id,
            thread_id,
            prompt,
            assistant_surface,
            client_mutation_id,
        )
        .await
    }

    pub async fn drain_notification_reply_outbox(
        &self,
    ) -> Result<ClientNotificationReplyIntentResult, ClientCoreError> {
        let envelope = self.drain_notification_reply_outbox_envelope().await?;
        Ok(ClientNotificationReplyIntentResult::from(envelope))
    }

    pub fn persist_notification_reply(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.local_store.enqueue_notification_reply_command(
            notification_id,
            thread_id,
            prompt,
            assistant_surface,
            client_mutation_id,
        )
    }

    pub fn persist_notification_reply_with_generated_mutation(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
    ) -> Result<ClientNotificationReplyPersistResult, ClientCoreError> {
        let notification_id = notification_id.trim().to_owned();
        let client_mutation_id = notification_reply_client_mutation_id(&notification_id);
        let snapshot = self.persist_notification_reply(
            notification_id,
            thread_id,
            prompt,
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        Ok(ClientNotificationReplyPersistResult {
            client_mutation_id,
            snapshot,
        })
    }

    pub fn outbox_depth(&self) -> Result<u32, ClientCoreError> {
        Ok(self.client_core.snapshot()?.outbox_depth)
    }
}

impl LooperClientCoreSessionRuntime {
    async fn drain_notification_reply_outbox_envelope(
        &self,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        self.client_core
            .drain_notification_reply_outbox_durable(self.local_store.clone())
            .await
    }

    fn seed_core_from_local_store(&self) -> Result<Vec<String>, ClientCoreError> {
        let snapshot = self.local_store.snapshot()?;
        self.client_core
            .replace_state_minis(resume_seed_snapshot(snapshot.clone()))?;
        self.client_core
            .restore_pending_commands(snapshot.pending_commands)
    }

    fn restore_pending_commands_from_local_store(&self) -> Result<Vec<String>, ClientCoreError> {
        self.client_core
            .restore_pending_commands(self.local_store.pending_commands()?)
    }

    fn persist_core_snapshot(
        &self,
        snapshot: &ClientStateSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.local_store
            .replace_state_minis(ClientStateMiniSnapshot::from(snapshot.clone()))
    }

    fn persist_last_good_endpoint(
        &self,
        snapshot: &ClientStateSnapshot,
    ) -> Result<(), ClientCoreError> {
        if snapshot.phase != crate::model::ConnectionPhase::Ready
            || snapshot.endpoint_url.trim().is_empty()
        {
            return Ok(());
        }
        self.local_store
            .mark_last_good_endpoint(snapshot.endpoint_url.clone(), snapshot.endpoint_transport)
    }

    fn emit_cached_local_state(
        &self,
        snapshot: &ClientStateSnapshot,
    ) -> Result<(), ClientCoreError> {
        let local_snapshot = self.local_store.snapshot()?;
        if local_snapshot.sessions.is_empty() && local_snapshot.pending_commands.is_empty() {
            return Ok(());
        }
        self.client_core.emit_local_state_update(snapshot.clone());
        Ok(())
    }

    fn local_state_stream_update(
        &self,
        update: ClientStateMiniStreamUpdate,
    ) -> Result<ClientLocalStateStreamUpdate, ClientCoreError> {
        let pending_commands = self.local_store.pending_commands()?;
        Ok(ClientLocalStateStreamUpdate {
            reason: update.reason,
            snapshot: ClientLocalStateSnapshot {
                latest_seq: update.snapshot.latest_seq,
                sessions: update.snapshot.state_minis,
                pending_commands,
                server_time: update.snapshot.server_time,
            },
            did_change: update.did_change,
            error_description: update.error_description,
            has_text_chunk: update.has_text_chunk,
            text_chunk: update.text_chunk,
        })
    }

    fn mobile_snapshot_stream_update(
        &self,
        update: ClientLocalStateStreamUpdate,
    ) -> Result<ClientMobileSnapshotStreamUpdate, ClientCoreError> {
        let sync_reason = sync_reason(update.reason);
        let latest_seq = update.snapshot.latest_seq;
        let server_time = update.snapshot.server_time.clone();
        let should_stop = update.reason == ClientStateMiniStreamUpdateReason::Stopped;
        let debug_message = recovery_wait_debug_message(&update);

        if update.has_text_chunk || !update.did_change {
            return Ok(ClientMobileSnapshotStreamUpdate {
                has_snapshot: false,
                snapshot: ClientMobileSnapshot::empty(),
                sync_reason,
                should_stop,
                latest_seq,
                server_time,
                error_description: update.error_description,
                debug_message,
                has_text_chunk: update.has_text_chunk,
                text_chunk: update.text_chunk,
            });
        }

        let projection = reduce_state_minis_mobile_snapshot_with_pending_commands(
            latest_seq,
            update.snapshot.sessions,
            update.snapshot.pending_commands,
            server_time.clone(),
        )?;

        Ok(ClientMobileSnapshotStreamUpdate {
            has_snapshot: projection.has_snapshot,
            snapshot: projection.snapshot,
            sync_reason,
            should_stop,
            latest_seq,
            server_time,
            error_description: update.error_description,
            debug_message,
            has_text_chunk: update.has_text_chunk,
            text_chunk: update.text_chunk,
        })
    }

    fn menu_snapshot_stream_update(
        &self,
        update: ClientLocalStateStreamUpdate,
    ) -> Result<ClientMenuSnapshotStreamUpdate, ClientCoreError> {
        let sync_reason = sync_reason(update.reason);
        let should_stop = update.reason == ClientStateMiniStreamUpdateReason::Stopped;
        let debug_message = recovery_wait_debug_message(&update);

        if update.has_text_chunk || !update.did_change {
            return Ok(ClientMenuSnapshotStreamUpdate {
                has_snapshot: false,
                snapshot: empty_menu_snapshot(),
                sync_reason,
                should_stop,
                error_description: update.error_description,
                debug_message,
            });
        }

        let snapshot = reduce_state_minis_menu_snapshot(update.snapshot)?;
        Ok(ClientMenuSnapshotStreamUpdate {
            has_snapshot: !snapshot.sessions.is_empty(),
            snapshot,
            sync_reason,
            should_stop,
            error_description: update.error_description,
            debug_message,
        })
    }
}

fn resume_seed_snapshot(snapshot: ClientLocalStateSnapshot) -> ClientStateMiniSnapshot {
    let latest_seq = if snapshot.sessions.is_empty() {
        EMPTY_LOCAL_REPLAY_SEQUENCE
    } else {
        snapshot.latest_seq
    };
    ClientStateMiniSnapshot {
        latest_seq,
        sessions: snapshot.sessions,
        server_time: snapshot.server_time,
    }
}

fn generated_client_mutation_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

fn notification_reply_client_mutation_id(notification_id: &str) -> String {
    format!(
        "{}:{}",
        NOTIFICATION_REPLY_MUTATION_PREFIX,
        notification_id.trim()
    )
}

fn sync_reason(reason: ClientStateMiniStreamUpdateReason) -> String {
    match reason {
        ClientStateMiniStreamUpdateReason::RecoveryRequired => MOBILE_SYNC_REASON_RECOVERY,
        ClientStateMiniStreamUpdateReason::TextChunk => MOBILE_SYNC_REASON_TEXT_CHUNK,
        ClientStateMiniStreamUpdateReason::Heartbeat => MOBILE_SYNC_REASON_HEARTBEAT,
        ClientStateMiniStreamUpdateReason::Reconnecting => MOBILE_SYNC_REASON_RECONNECTING,
        ClientStateMiniStreamUpdateReason::Delta | ClientStateMiniStreamUpdateReason::Stopped => {
            MOBILE_SYNC_REASON_DELTA
        }
    }
    .to_owned()
}

fn recovery_wait_debug_message(update: &ClientLocalStateStreamUpdate) -> String {
    if update.did_change || update.error_description.is_empty() {
        return String::new();
    }

    match update.reason {
        ClientStateMiniStreamUpdateReason::RecoveryRequired => {
            format!(
                "session-mini:client-core-stream-recovery-waiting error={}",
                update.error_description
            )
        }
        ClientStateMiniStreamUpdateReason::Reconnecting => {
            format!(
                "session-mini:client-core-stream-reconnecting error={}",
                update.error_description
            )
        }
        ClientStateMiniStreamUpdateReason::Delta
        | ClientStateMiniStreamUpdateReason::TextChunk
        | ClientStateMiniStreamUpdateReason::Heartbeat
        | ClientStateMiniStreamUpdateReason::Stopped => String::new(),
    }
}

fn empty_menu_snapshot() -> ClientMenuBarSessionMiniLocalSnapshot {
    ClientMenuBarSessionMiniLocalSnapshot {
        latest_seq: 0,
        sessions: Vec::new(),
        pending_commands: Vec::new(),
    }
}

impl From<ClientLocalStateSnapshot> for ClientStateMiniSnapshot {
    fn from(snapshot: ClientLocalStateSnapshot) -> Self {
        Self {
            latest_seq: snapshot.latest_seq,
            sessions: snapshot.sessions,
            server_time: snapshot.server_time,
        }
    }
}

impl From<ClientCommandAckEnvelope> for ClientNotificationReplyIntentResult {
    fn from(envelope: ClientCommandAckEnvelope) -> Self {
        Self {
            accepted: envelope.ack.accepted,
            dispatch_kind: envelope.dispatch_kind,
            prompt_id: envelope.prompt_id,
            server_time: envelope.ack.server_time,
            client_mutation_id: envelope.ack.client_mutation_id,
            ack_seq: envelope.ack.ack_seq,
            entity_id: envelope.ack.entity_id,
            revision: envelope.ack.revision,
            idempotent_replay: envelope.ack.idempotent_replay,
            notification_id: envelope.notification_id,
        }
    }
}

impl From<ClientStateSnapshot> for ClientStateMiniSnapshot {
    fn from(snapshot: ClientStateSnapshot) -> Self {
        Self {
            latest_seq: snapshot.latest_seq,
            sessions: snapshot.state_minis,
            server_time: snapshot.server_time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_store::LOCAL_STORE_DEBOUNCE_INTERVAL;
    use crate::model::{
        ClientEndpoint, ClientPendingCommandKind, ClientStateMini, ClientTextChunk,
    };
    use crate::session_transport::proto;
    use std::{
        future::Future,
        io::{Read, Write},
        net::TcpListener,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll, Wake},
        thread,
        time::Duration,
    };
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    #[test]
    fn runtime_persists_prompt_before_transport() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("prompt")).expect("runtime");

        let result = test_runtime
            .block_on(runtime.send_prompt(
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "queue".to_owned(),
            ))
            .expect("local prompt accepted before transport");
        assert!(result.accepted);
        assert_eq!(result.dispatch_kind, LOCAL_ACCEPTED_DISPATCH_KIND);

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SendSessionPrompt
        );
        assert_eq!(snapshot.pending_commands[0].thread_id, "thread-main");
        assert_eq!(snapshot.pending_commands[0].prompt, "continue");
        assert!(
            snapshot.pending_commands[0]
                .client_mutation_id
                .starts_with("prompt-")
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
        assert_eq!(runtime.outbox_depth().expect("outbox depth"), 1);
        drop(runtime);
        drop(test_runtime);
    }

    #[test]
    fn runtime_restores_durable_pending_commands_into_core_outbox() {
        let path = temp_store_path("restore-pending-outbox");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");
        runtime
            .local_store
            .enqueue_save_default_prompt_command(
                "Continue".to_owned(),
                "default-prompt-restore".to_owned(),
            )
            .expect("enqueue default prompt command");
        runtime
            .local_store
            .mark_attempted("default-prompt-restore".to_owned())
            .expect("mark attempted");
        drop(runtime);

        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let snapshot = reopened.local_snapshot().expect("local snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SaveDefaultPrompt
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
        assert_eq!(reopened.outbox_depth().expect("outbox depth"), 1);
    }

    #[test]
    fn runtime_generates_mode_mutation_id_before_transport() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("generated-mode"))
            .expect("runtime");

        let result = test_runtime
            .block_on(runtime.set_mode("thread-main".to_owned(), "max-turns-2".to_owned()))
            .expect("local mode accepted before transport");
        assert!(result.accepted);
        assert_eq!(result.preset, "max-turns-2");

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SetSessionMode
        );
        assert!(
            snapshot.pending_commands[0]
                .client_mutation_id
                .starts_with("mode-")
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
    }

    #[test]
    fn runtime_generates_prompt_mutation_id_before_transport() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("generated-prompt"))
            .expect("runtime");

        let result = test_runtime
            .block_on(runtime.send_prompt(
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "queue".to_owned(),
            ))
            .expect("local prompt accepted before transport");
        assert!(result.accepted);
        assert_eq!(result.dispatch_kind, LOCAL_ACCEPTED_DISPATCH_KIND);

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SendSessionPrompt
        );
        assert!(
            snapshot.pending_commands[0]
                .client_mutation_id
                .starts_with("prompt-")
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
    }

    #[test]
    fn runtime_generates_default_notification_targets_mutation_id_before_transport() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("generated-notification-targets"))
                .expect("runtime");

        let result =
            test_runtime
                .block_on(runtime.set_default_notification_targets(vec![
                    "macos".to_owned(),
                    "iphone".to_owned(),
                ]))
                .expect("local target change accepted before transport");
        assert!(result.accepted);

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SetDefaultNotificationTargets
        );
        assert_eq!(
            snapshot.pending_commands[0].notification_target_ids,
            vec!["macos".to_owned(), "iphone".to_owned()]
        );
        assert!(
            snapshot.pending_commands[0]
                .client_mutation_id
                .starts_with("default-notification-targets-")
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
    }

    #[test]
    fn runtime_paints_mode_before_transport() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("mode-paint")).expect("runtime");
        seed_runtime_state_minis(
            &runtime,
            ClientStateMiniSnapshot {
                latest_seq: 7,
                sessions: vec![ClientStateMini {
                    session_id: "thread-main".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 7,
                    revision: "rev-7".to_owned(),
                    payload_json: r#"{"sessionId":"thread-main","assistantSurface":"codex","effectiveMode":"await-reply"}"#.to_owned(),
                }],
                server_time: "2026-06-25T00:00:00Z".to_owned(),
            },
        );

        let result = test_runtime
            .block_on(runtime.set_mode("thread-main".to_owned(), "max-turns-2".to_owned()))
            .expect("local mode accepted before transport");
        assert!(result.accepted);
        assert_eq!(result.preset, "max-turns-2");

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert!(
            snapshot.pending_commands[0]
                .client_mutation_id
                .starts_with("mode-")
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
        assert!(
            snapshot.sessions[0]
                .payload_json
                .contains(r#""effectiveMode":"max-turns-2""#)
        );
        drop(runtime);
        drop(test_runtime);
    }

    #[test]
    fn runtime_generated_notification_reply_persist_is_deterministic() {
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("reply-generated-persist"))
                .expect("runtime");

        let queued = runtime
            .persist_notification_reply_with_generated_mutation(
                " notification-main ".to_owned(),
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
            )
            .expect("queue generated reply");

        assert_eq!(
            queued.client_mutation_id,
            "notification-reply:notification-main"
        );
        assert_eq!(queued.snapshot.pending_commands.len(), 1);
        assert_eq!(
            queued.snapshot.pending_commands[0].client_mutation_id,
            queued.client_mutation_id
        );
    }

    #[test]
    fn runtime_seeds_state_minis_from_durable_store() {
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("state-mini")).expect("runtime");

        let snapshot = seed_runtime_state_minis(
            &runtime,
            ClientStateMiniSnapshot {
                latest_seq: 7,
                sessions: vec![ClientStateMini {
                    session_id: "thread-main".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 7,
                    revision: "rev-7".to_owned(),
                    payload_json: r#"{"title":"Ready"}"#.to_owned(),
                }],
                server_time: "2026-06-26T00:00:00Z".to_owned(),
            },
        );

        assert_eq!(snapshot.latest_seq, 7);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(
            runtime.state_snapshot().expect("core snapshot").latest_seq,
            7
        );
        assert_eq!(
            runtime.local_snapshot().expect("local snapshot").latest_seq,
            7
        );
    }

    #[test]
    fn runtime_start_emits_cached_mobile_snapshot_before_recovery() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("cached-mobile-start"))
            .expect("runtime");
        seed_runtime_state_minis(
            &runtime,
            ClientStateMiniSnapshot {
                latest_seq: 7,
                sessions: vec![state_mini("thread-main", "codex", 7, "rev-7", "Cached")],
                server_time: "2026-06-26T00:00:00Z".to_owned(),
            },
        );

        let started = runtime
            .start(
                vec![ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: "http://127.0.0.1:1".to_owned(),
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                }],
                String::new(),
                String::new(),
            )
            .expect("start cached runtime");
        assert_eq!(started.phase, crate::model::ConnectionPhase::Connecting);
        assert_eq!(started.latest_seq, 7);
        assert_eq!(started.state_minis.len(), 1);

        let update = test_runtime
            .block_on(runtime.observe_mobile_snapshot_change())
            .expect("cached mobile update");
        assert!(update.has_snapshot);
        assert_eq!(update.sync_reason, "delta");
        assert_eq!(update.latest_seq, 7);
        assert_eq!(update.snapshot.sessions[0].id, "thread-main");
        assert_eq!(update.snapshot.sessions[0].title, "Cached");
    }

    #[test]
    fn runtime_same_endpoint_start_keeps_warm_stream_and_live_core_state() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("warm-same-endpoint-start"))
                .expect("runtime");

        test_runtime.block_on(async {
            let (stream_url, session_count, server) =
                spawn_counting_realtime_session_server().await;
            runtime
                .start(
                    vec![ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: stream_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    }],
                    String::new(),
                    String::new(),
                )
                .expect("start runtime");

            let update = tokio::time::timeout(Duration::from_secs(1), runtime.observe())
                .await
                .expect("observe first warm stream heartbeat")
                .expect("runtime update");
            assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Heartbeat);
            assert_eq!(update.snapshot.phase, crate::model::ConnectionPhase::Ready);
            assert_eq!(session_count.load(Ordering::SeqCst), 1);

            runtime
                .client_core
                .replace_state_minis(ClientStateMiniSnapshot {
                    latest_seq: 21,
                    sessions: vec![state_mini("thread-live", "codex", 21, "rev-21", "Live")],
                    server_time: "2026-06-26T00:00:21Z".to_owned(),
                })
                .expect("seed live core state");
            runtime
                .local_store
                .replace_state_minis(ClientStateMiniSnapshot {
                    latest_seq: 7,
                    sessions: vec![state_mini("thread-stale", "codex", 7, "rev-7", "Stale")],
                    server_time: "2026-06-26T00:00:07Z".to_owned(),
                })
                .expect("seed stale disk cache");

            let snapshot = runtime
                .start(
                    vec![ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: format!("{}/", stream_url.trim_end_matches('/')),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    }],
                    String::new(),
                    String::new(),
                )
                .expect("same identity start");

            assert_eq!(snapshot.phase, crate::model::ConnectionPhase::Ready);
            assert_eq!(snapshot.latest_seq, 21);
            assert_eq!(snapshot.state_minis.len(), 1);
            assert_eq!(snapshot.state_minis[0].session_id, "thread-live");
            assert!(
                snapshot.state_minis[0]
                    .payload_json
                    .contains(r#""title":"Live""#)
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(session_count.load(Ordering::SeqCst), 1);

            server.abort();
            let _ = server.await;
        });
    }

    #[test]
    fn runtime_start_replays_cached_pending_commands_for_menu_overlay() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("cached-menu-pending"))
            .expect("runtime");
        seed_runtime_state_minis(
            &runtime,
            ClientStateMiniSnapshot {
                latest_seq: 11,
                sessions: vec![state_mini(
                    "thread-menu",
                    "codex",
                    11,
                    "rev-11",
                    "Menu Cached",
                )],
                server_time: "2026-06-26T00:00:11Z".to_owned(),
            },
        );
        runtime
            .local_store
            .enqueue_notification_reply_command(
                "notification-menu".to_owned(),
                "thread-menu".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "notification-reply:notification-menu".to_owned(),
            )
            .expect("enqueue pending reply");

        runtime
            .start(
                vec![ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: "http://127.0.0.1:1".to_owned(),
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                }],
                String::new(),
                String::new(),
            )
            .expect("start cached runtime");

        let update = test_runtime
            .block_on(runtime.observe_menu_snapshot_change())
            .expect("cached menu update");
        assert!(update.has_snapshot);
        assert_eq!(update.snapshot.latest_seq, 11);
        assert_eq!(update.snapshot.sessions[0].session_id, "thread-menu");
        assert_eq!(update.snapshot.pending_commands.len(), 1);
        assert_eq!(
            update.snapshot.pending_commands[0].client_mutation_id,
            "notification-reply:notification-menu"
        );
    }

    #[test]
    fn runtime_persists_first_ready_fallback_endpoint_as_last_good() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let path = temp_store_path("first-ready-fallback");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");
        let stale_url = unused_local_url();
        runtime
            .local_store
            .mark_last_good_endpoint(stale_url.clone(), crate::model::ClientEndpointTransport::H2)
            .expect("seed stale last-good endpoint");

        let fallback_url = test_runtime.block_on(async {
            let (fallback_url, server) = spawn_realtime_session_server().await;
            let started = runtime
                .start(
                    vec![
                        ClientEndpoint {
                            transport: crate::model::ClientEndpointTransport::H2,
                            url: stale_url.clone(),
                            recovery_base_url: String::new(),
                            h3_certificate_sha256: String::new(),
                            h3_certificate_spki_sha256: String::new(),
                            last_good: false,
                        },
                        ClientEndpoint {
                            transport: crate::model::ClientEndpointTransport::H2,
                            url: fallback_url.clone(),
                            recovery_base_url: String::new(),
                            h3_certificate_sha256: String::new(),
                            h3_certificate_spki_sha256: String::new(),
                            last_good: false,
                        },
                    ],
                    String::new(),
                    String::new(),
                )
                .expect("start runtime");

            assert_eq!(started.phase, crate::model::ConnectionPhase::Connecting);
            assert!(started.endpoint_url.is_empty());

            let update = tokio::time::timeout(Duration::from_secs(1), runtime.observe())
                .await
                .expect("observe first ready endpoint")
                .expect("runtime update");
            assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Heartbeat);
            assert_eq!(update.snapshot.phase, crate::model::ConnectionPhase::Ready);
            assert_eq!(
                update.snapshot.endpoint_url,
                fallback_url.trim_end_matches('/')
            );

            server.abort();
            let _ = server.await;
            fallback_url
        });
        drop(runtime);

        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let endpoints = reopened
            .local_store
            .endpoints_with_last_good(vec![
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: stale_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: fallback_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
            ])
            .expect("stored endpoints");

        assert!(!endpoints[0].last_good);
        assert!(endpoints[1].last_good);
    }

    #[test]
    fn runtime_does_not_rewrite_local_store_for_duplicate_heartbeat() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let path = temp_store_path("duplicate-heartbeat-no-store-write");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");

        let heartbeat_url = test_runtime.block_on(async {
            let (heartbeat_url, server) = spawn_realtime_session_server().await;
            runtime
                .local_store
                .mark_last_good_endpoint(
                    heartbeat_url.clone(),
                    crate::model::ClientEndpointTransport::H2,
                )
                .expect("seed last-good endpoint");
            runtime.local_store.flush().expect("flush seeded endpoint");
            let before = std::fs::read(&path).expect("read initial local store");

            runtime
                .start(
                    vec![ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: heartbeat_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    }],
                    String::new(),
                    String::new(),
                )
                .expect("start runtime");

            let update = tokio::time::timeout(Duration::from_secs(1), runtime.observe())
                .await
                .expect("observe heartbeat")
                .expect("runtime update");
            assert_eq!(update.reason, ClientStateMiniStreamUpdateReason::Heartbeat);
            assert!(!update.did_change);

            let after = std::fs::read(&path).expect("read local store after heartbeat");
            assert_eq!(after, before);

            server.abort();
            let _ = server.await;
            heartbeat_url
        });

        drop(runtime);
        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let endpoints = reopened
            .local_store
            .endpoints_with_last_good(vec![ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: heartbeat_url,
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            }])
            .expect("stored endpoints");

        assert!(endpoints[0].last_good);
    }

    #[test]
    fn runtime_recovery_persists_first_ready_fallback_endpoint_as_last_good() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let path = temp_store_path("recovery-persists-first-ready-fallback");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");
        let stale_url = unused_local_url();
        runtime
            .local_store
            .mark_last_good_endpoint(stale_url.clone(), crate::model::ClientEndpointTransport::H2)
            .expect("seed stale endpoint");
        let (recovery_url, recovery_server) = spawn_snapshot_server(31, "thread-recovered");

        let local_snapshot = test_runtime
            .block_on(runtime.recover_state_mini_snapshot(
                vec![
                    ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: stale_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    },
                    ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: recovery_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    },
                ],
                String::new(),
                String::new(),
            ))
            .expect("recover snapshot");

        assert_eq!(local_snapshot.latest_seq, 31);
        assert_eq!(local_snapshot.sessions[0].session_id, "thread-recovered");
        assert_ne!(recovery_url.trim_end_matches('/'), stale_url);
        drop(runtime);
        let _ = recovery_server.join();

        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let endpoints = reopened
            .local_store
            .endpoints_with_last_good(vec![
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: stale_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: recovery_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
            ])
            .expect("stored endpoints");

        assert!(!endpoints[0].last_good);
        assert!(endpoints[1].last_good);
    }

    #[test]
    fn runtime_recovery_stale_snapshot_preserves_live_local_state_and_endpoint() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let path = temp_store_path("recovery-stale-preserves-live-local");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");
        let stale_url = unused_local_url();
        runtime
            .local_store
            .mark_last_good_endpoint(stale_url.clone(), crate::model::ClientEndpointTransport::H2)
            .expect("seed stale endpoint");
        seed_runtime_state_minis(
            &runtime,
            ClientStateMiniSnapshot {
                latest_seq: 20,
                sessions: vec![state_mini("thread-live", "zed", 20, "rev-20", "Live")],
                server_time: "2026-06-26T00:00:20Z".to_owned(),
            },
        );
        let (recovery_url, recovery_server) = spawn_snapshot_server(10, "thread-stale");

        let local_snapshot = test_runtime
            .block_on(runtime.recover_state_mini_snapshot(
                vec![
                    ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: stale_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    },
                    ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: recovery_url.clone(),
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    },
                ],
                String::new(),
                String::new(),
            ))
            .expect("recover stale snapshot");

        assert_eq!(local_snapshot.latest_seq, 20);
        assert_eq!(local_snapshot.sessions.len(), 1);
        assert_eq!(local_snapshot.sessions[0].session_id, "thread-live");
        drop(runtime);
        let _ = recovery_server.join();

        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let endpoints = reopened
            .local_store
            .endpoints_with_last_good(vec![
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: stale_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: recovery_url,
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
            ])
            .expect("stored endpoints");

        assert!(endpoints[0].last_good);
        assert!(!endpoints[1].last_good);
    }

    #[test]
    fn runtime_recovery_uses_owned_reactor_when_called_without_tokio_context() {
        let path = temp_store_path("recovery-without-caller-reactor");
        let runtime = LooperClientCoreSessionRuntime::new(path).expect("runtime");
        let (recovery_url, recovery_server) = spawn_snapshot_server(37, "thread-no-reactor");

        let local_snapshot = block_on_without_tokio(runtime.recover_state_mini_snapshot(
            vec![ClientEndpoint {
                transport: crate::model::ClientEndpointTransport::H2,
                url: recovery_url.clone(),
                recovery_base_url: String::new(),
                h3_certificate_sha256: String::new(),
                h3_certificate_spki_sha256: String::new(),
                last_good: false,
            }],
            String::new(),
            String::new(),
        ))
        .expect("recover snapshot without caller reactor");

        assert_eq!(local_snapshot.latest_seq, 37);
        assert_eq!(local_snapshot.sessions[0].session_id, "thread-no-reactor");
        let _ = recovery_server.join();
    }

    #[test]
    fn runtime_text_chunk_persists_detail_projection_from_session_stream() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let path = temp_store_path("text-chunk-session-stream");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");

        test_runtime.block_on(async {
            let (stream_url, server) = spawn_text_chunk_realtime_session_server().await;
            runtime
                .start(
                    vec![ClientEndpoint {
                        transport: crate::model::ClientEndpointTransport::H2,
                        url: stream_url,
                        recovery_base_url: String::new(),
                        h3_certificate_sha256: String::new(),
                        h3_certificate_spki_sha256: String::new(),
                        last_good: false,
                    }],
                    String::new(),
                    String::new(),
                )
                .expect("start runtime");

            let update = tokio::time::timeout(
                Duration::from_secs(1),
                runtime.observe_mobile_snapshot_change(),
            )
            .await
            .expect("observe text chunk update")
            .expect("text chunk update");
            assert!(!update.has_snapshot);
            assert_eq!(update.sync_reason, MOBILE_SYNC_REASON_TEXT_CHUNK);
            assert!(update.has_text_chunk);
            assert_eq!(update.text_chunk.thread_id, "thread-live");
            assert_eq!(update.text_chunk.content, "Hello live");

            let detail = runtime
                .session_detail("thread-live".to_owned())
                .expect("detail projection");
            assert!(detail.has_latest_reply);
            assert_eq!(detail.latest_reply.message_id, "message-live");
            assert_eq!(detail.latest_reply.text, "Hello live");
            assert_eq!(detail.latest_reply.latest_seq, 44);
            assert!(!detail.latest_reply.is_final);

            server.abort();
            let _ = server.await;
        });
        drop(runtime);

        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let detail = reopened
            .session_detail("thread-live".to_owned())
            .expect("reopened detail projection");
        assert_eq!(detail.latest_reply.text, "Hello live");
        assert_eq!(detail.latest_reply.latest_seq, 44);
    }

    #[test]
    fn runtime_stop_does_not_block_on_local_store_flush() {
        // stop() used to flush the local store, which waited FIFO behind the
        // persister queue (measured 37s on device during command-ack bursts)
        // and stalled every stream restart. Pending writes still land via the
        // persister's own debounce/staleness deadlines — stop() just must not
        // wait for them.
        let path = temp_store_path("stop-does-not-block-on-flush");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");
        runtime
            .local_store
            .apply_text_chunk(ClientTextChunk {
                seq: 9,
                thread_id: "thread-live".to_owned(),
                message_id: "message-live".to_owned(),
                content: "stop flush".to_owned(),
                is_final: true,
                server_time: "2026-06-24T00:00:09Z".to_owned(),
            })
            .expect("text chunk");

        let stop_started_at = std::time::Instant::now();
        runtime.stop().expect("stop runtime");
        assert!(
            stop_started_at.elapsed() < LOCAL_STORE_DEBOUNCE_INTERVAL,
            "stop() must return without waiting on the persister queue"
        );

        // Dropping the runtime shuts the persister down, which flushes
        // pending writes — durability comes from the persister lifecycle,
        // not from stop().
        drop(runtime);
        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        let detail = reopened
            .session_detail("thread-live".to_owned())
            .expect("detail projection");
        assert_eq!(detail.latest_reply.text, "stop flush");
        assert_eq!(detail.latest_reply.latest_seq, 9);
    }

    #[test]
    fn runtime_does_not_seed_empty_durable_state_as_replay_cursor() {
        let path = temp_store_path("empty-state-mini-cursor");
        let runtime = LooperClientCoreSessionRuntime::new(path.clone()).expect("runtime");
        runtime
            .local_store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 42,
                sessions: Vec::new(),
                server_time: "2026-06-26T00:00:42Z".to_owned(),
            })
            .expect("poison empty local cursor");
        drop(runtime);

        let reopened = LooperClientCoreSessionRuntime::new(path).expect("reopened runtime");
        assert_eq!(
            reopened
                .local_snapshot()
                .expect("local snapshot")
                .latest_seq,
            42
        );
        assert_eq!(
            reopened.state_snapshot().expect("core snapshot").latest_seq,
            EMPTY_LOCAL_REPLAY_SEQUENCE
        );
        assert!(
            reopened
                .state_snapshot()
                .expect("core snapshot")
                .state_minis
                .is_empty()
        );
    }

    #[test]
    fn runtime_projects_mobile_snapshot_stream_update_in_rust() {
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("mobile-stream")).expect("runtime");
        let update = runtime
            .mobile_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::Delta,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 7,
                    sessions: vec![state_mini("thread-main", "codex", 7, "rev-7", "Ready")],
                    pending_commands: Vec::new(),
                    server_time: "2026-06-26T00:00:00Z".to_owned(),
                },
                did_change: true,
                error_description: String::new(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("mobile projection");

        assert!(update.has_snapshot);
        assert_eq!(update.sync_reason, "delta");
        assert!(!update.should_stop);
        assert_eq!(update.latest_seq, 7);
        assert_eq!(update.snapshot.revision, "rev-7");
        assert_eq!(update.snapshot.sessions[0].id, "thread-main");
        assert!(update.debug_message.is_empty());
    }

    #[test]
    fn runtime_reports_recovery_wait_without_swift_reason_logic() {
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("mobile-recovery"))
            .expect("runtime");
        let update = runtime
            .mobile_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::RecoveryRequired,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 9,
                    sessions: Vec::new(),
                    pending_commands: Vec::new(),
                    server_time: String::new(),
                },
                did_change: false,
                error_description: "seq_gap".to_owned(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("mobile recovery update");

        assert!(!update.has_snapshot);
        assert_eq!(update.sync_reason, "recovery");
        assert!(!update.should_stop);
        assert_eq!(
            update.debug_message,
            "session-mini:client-core-stream-recovery-waiting error=seq_gap"
        );
    }

    #[test]
    fn runtime_reports_reconnecting_wait_without_swift_reason_logic() {
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("mobile-reconnecting"))
            .expect("runtime");
        let update = runtime
            .mobile_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::Reconnecting,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 9,
                    sessions: Vec::new(),
                    pending_commands: Vec::new(),
                    server_time: String::new(),
                },
                did_change: false,
                error_description: "transport unavailable".to_owned(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("mobile reconnecting update");

        assert!(!update.has_snapshot);
        assert_eq!(update.sync_reason, "reconnecting");
        assert!(!update.should_stop);
        assert_eq!(
            update.debug_message,
            "session-mini:client-core-stream-reconnecting error=transport unavailable"
        );
    }

    #[test]
    fn runtime_reports_heartbeat_liveness_without_projecting_snapshot() {
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("mobile-heartbeat"))
            .expect("runtime");
        let update = runtime
            .mobile_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::Heartbeat,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 11,
                    sessions: Vec::new(),
                    pending_commands: Vec::new(),
                    server_time: "2026-06-26T00:00:11Z".to_owned(),
                },
                did_change: false,
                error_description: String::new(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("mobile heartbeat update");

        assert!(!update.has_snapshot);
        assert_eq!(update.sync_reason, "heartbeat");
        assert_eq!(update.latest_seq, 11);
        assert_eq!(update.server_time, "2026-06-26T00:00:11Z");
        assert!(update.debug_message.is_empty());
    }

    #[test]
    fn runtime_marks_stopped_mobile_stream_update() {
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("mobile-stopped"))
            .expect("runtime");
        let update = runtime
            .mobile_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::Stopped,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 0,
                    sessions: Vec::new(),
                    pending_commands: Vec::new(),
                    server_time: String::new(),
                },
                did_change: false,
                error_description: String::new(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("mobile stopped update");

        assert!(!update.has_snapshot);
        assert!(update.should_stop);
    }

    #[test]
    fn runtime_projects_menu_snapshot_stream_update_in_rust() {
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("menu-stream")).expect("runtime");
        let update = runtime
            .menu_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::Delta,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 11,
                    sessions: vec![state_mini(
                        "thread-menu",
                        "codex",
                        11,
                        "rev-11",
                        "Menu Ready",
                    )],
                    pending_commands: Vec::new(),
                    server_time: "2026-06-26T00:00:00Z".to_owned(),
                },
                did_change: true,
                error_description: String::new(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("menu projection");

        assert!(update.has_snapshot);
        assert_eq!(update.sync_reason, "delta");
        assert!(!update.should_stop);
        assert_eq!(update.snapshot.latest_seq, 11);
        assert_eq!(update.snapshot.sessions[0].session_id, "thread-menu");
        assert_eq!(update.snapshot.sessions[0].title, "Menu Ready");
    }

    #[test]
    fn runtime_reports_menu_recovery_wait_without_swift_reason_logic() {
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("menu-recovery")).expect("runtime");
        let update = runtime
            .menu_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::RecoveryRequired,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 13,
                    sessions: Vec::new(),
                    pending_commands: Vec::new(),
                    server_time: String::new(),
                },
                did_change: false,
                error_description: "seq_gap".to_owned(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("menu recovery update");

        assert!(!update.has_snapshot);
        assert_eq!(update.sync_reason, "recovery");
        assert_eq!(
            update.debug_message,
            "session-mini:client-core-stream-recovery-waiting error=seq_gap"
        );
    }

    #[test]
    fn runtime_reports_menu_reconnecting_wait_without_swift_reason_logic() {
        let runtime = LooperClientCoreSessionRuntime::new(temp_store_path("menu-reconnecting"))
            .expect("runtime");
        let update = runtime
            .menu_snapshot_stream_update(ClientLocalStateStreamUpdate {
                reason: ClientStateMiniStreamUpdateReason::Reconnecting,
                snapshot: ClientLocalStateSnapshot {
                    latest_seq: 13,
                    sessions: Vec::new(),
                    pending_commands: Vec::new(),
                    server_time: String::new(),
                },
                did_change: false,
                error_description: "transport unavailable".to_owned(),
                has_text_chunk: false,
                text_chunk: ClientTextChunk::empty(),
            })
            .expect("menu reconnecting update");

        assert!(!update.has_snapshot);
        assert_eq!(update.sync_reason, "reconnecting");
        assert_eq!(
            update.debug_message,
            "session-mini:client-core-stream-reconnecting error=transport unavailable"
        );
    }

    fn seed_runtime_state_minis(
        runtime: &LooperClientCoreSessionRuntime,
        snapshot: ClientStateMiniSnapshot,
    ) -> ClientLocalStateSnapshot {
        let local_snapshot = runtime
            .local_store
            .replace_state_minis(snapshot)
            .expect("seed local state minis");
        runtime
            .seed_core_from_local_store()
            .expect("seed core from local state minis");
        local_snapshot
    }

    fn temp_store_path(name: &str) -> String {
        let path = std::env::temp_dir()
            .join(format!(
                "looper-client-core-runtime-{name}-{}",
                std::process::id()
            ))
            .join("state-minis.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
        path.to_string_lossy().into_owned()
    }

    fn unused_local_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind unused port");
        let address = listener.local_addr().expect("unused addr");
        drop(listener);
        format!("http://{address}")
    }

    fn spawn_snapshot_server(
        latest_seq: i64,
        session_id: &'static str,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind snapshot server");
        let url = format!("http://{}", listener.local_addr().expect("server addr"));
        let handle = thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = format!(
                r#"{{"latestSeq":{latest_seq},"serverTime":"2026-06-28T00:00:00Z","sessions":[{{"sessionId":"{session_id}","assistantSurface":"codex","seq":{latest_seq},"revision":"rev-{latest_seq}","title":"{session_id}"}}]}}"#
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (url, handle)
    }

    struct ThreadWaker(thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on_without_tokio<F: Future>(future: F) -> F::Output {
        let waker = std::task::Waker::from(Arc::new(ThreadWaker(thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(output) => return output,
                Poll::Pending => thread::park_timeout(Duration::from_millis(10)),
            }
        }
    }

    async fn spawn_realtime_session_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind realtime server port");
        let address = listener.local_addr().expect("realtime server addr");
        drop(listener);
        let handle = tokio::spawn(async move {
            let service = proto::looper_realtime_server::LooperRealtimeServer::new(
                TestRealtimeSessionService,
            );
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(address)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        (format!("http://{address}"), handle)
    }

    async fn spawn_counting_realtime_session_server()
    -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind realtime server port");
        let address = listener.local_addr().expect("realtime server addr");
        drop(listener);
        let session_count = Arc::new(AtomicUsize::new(0));
        let service = CountingRealtimeSessionService {
            session_count: session_count.clone(),
        };
        let handle = tokio::spawn(async move {
            let service = proto::looper_realtime_server::LooperRealtimeServer::new(service);
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(address)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        (format!("http://{address}"), session_count, handle)
    }

    async fn spawn_text_chunk_realtime_session_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind text chunk server port");
        let address = listener.local_addr().expect("text chunk server addr");
        drop(listener);
        let handle = tokio::spawn(async move {
            let service = proto::looper_realtime_server::LooperRealtimeServer::new(
                TextChunkRealtimeSessionService,
            );
            let _ = tonic::transport::Server::builder()
                .add_service(service)
                .serve(address)
                .await;
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        (format!("http://{address}"), handle)
    }

    struct TestRealtimeSessionService;

    struct CountingRealtimeSessionService {
        session_count: Arc<AtomicUsize>,
    }

    struct TextChunkRealtimeSessionService;

    macro_rules! unimplemented_unary_command {
        ($name:ident, $request:ty) => {
            fn $name<'life0, 'async_trait>(
                &'life0 self,
                _request: tonic::Request<$request>,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<
                            Output = Result<tonic::Response<proto::CommandAck>, tonic::Status>,
                        > + Send
                        + 'async_trait,
                >,
            >
            where
                'life0: 'async_trait,
                Self: Sync + 'async_trait,
            {
                Box::pin(async { Err(tonic::Status::unimplemented("test service stream-only")) })
            }
        };
    }

    macro_rules! impl_unimplemented_unary_commands {
        () => {
            unimplemented_unary_command!(set_session_mode, proto::SetSessionModeRequest);
            unimplemented_unary_command!(send_session_prompt, proto::SendSessionPromptRequest);
            unimplemented_unary_command!(
                submit_notification_reply,
                proto::SubmitNotificationReplyRequest
            );
            unimplemented_unary_command!(
                set_siri_current_session,
                proto::SetSiriCurrentSessionRequest
            );
            unimplemented_unary_command!(
                set_siri_default_session,
                proto::SetSiriDefaultSessionRequest
            );
            unimplemented_unary_command!(save_default_prompt, proto::SaveDefaultPromptRequest);
            unimplemented_unary_command!(set_session_archived, proto::SetSessionArchivedRequest);
            unimplemented_unary_command!(delete_session, proto::DeleteSessionRequest);
            unimplemented_unary_command!(mute_session, proto::MuteSessionRequest);
            unimplemented_unary_command!(set_scope, proto::SetScopeRequest);
            unimplemented_unary_command!(set_global_preset, proto::SetGlobalPresetRequest);
            unimplemented_unary_command!(
                set_global_notification,
                proto::SetGlobalNotificationRequest
            );
            unimplemented_unary_command!(
                set_default_notification_targets,
                proto::SetDefaultNotificationTargetsRequest
            );
            unimplemented_unary_command!(
                set_global_completion_check,
                proto::SetGlobalCompletionCheckRequest
            );
            unimplemented_unary_command!(
                upsert_notification_route,
                proto::UpsertNotificationRouteRequest
            );
            unimplemented_unary_command!(
                delete_notification_route,
                proto::DeleteNotificationRouteRequest
            );
            unimplemented_unary_command!(
                upsert_completion_check,
                proto::UpsertCompletionCheckRequest
            );
            unimplemented_unary_command!(
                delete_completion_check,
                proto::DeleteCompletionCheckRequest
            );
            unimplemented_unary_command!(
                set_session_notifications,
                proto::SetSessionNotificationsRequest
            );
            unimplemented_unary_command!(
                set_session_completion_check,
                proto::SetSessionCompletionCheckRequest
            );
            unimplemented_unary_command!(set_assistant_surface, proto::SetAssistantSurfaceRequest);
        };
    }

    #[tonic::async_trait]
    impl proto::looper_realtime_server::LooperRealtime for TestRealtimeSessionService {
        type SessionStream = ReceiverStream<Result<proto::ServerFrame, tonic::Status>>;

        async fn health(
            &self,
            _request: tonic::Request<proto::HealthRequest>,
        ) -> Result<tonic::Response<proto::HealthResponse>, tonic::Status> {
            Ok(tonic::Response::new(proto::HealthResponse {
                ok: true,
                service: "test".to_owned(),
                server_time: String::new(),
            }))
        }

        async fn session(
            &self,
            _request: tonic::Request<tonic::Streaming<proto::ClientFrame>>,
        ) -> Result<tonic::Response<Self::SessionStream>, tonic::Status> {
            let (_sender, receiver) = mpsc::channel(1);
            Ok(tonic::Response::new(ReceiverStream::new(receiver)))
        }

        impl_unimplemented_unary_commands!();
    }

    #[tonic::async_trait]
    impl proto::looper_realtime_server::LooperRealtime for CountingRealtimeSessionService {
        type SessionStream = ReceiverStream<Result<proto::ServerFrame, tonic::Status>>;

        async fn health(
            &self,
            _request: tonic::Request<proto::HealthRequest>,
        ) -> Result<tonic::Response<proto::HealthResponse>, tonic::Status> {
            Ok(tonic::Response::new(proto::HealthResponse {
                ok: true,
                service: "test".to_owned(),
                server_time: String::new(),
            }))
        }

        async fn session(
            &self,
            _request: tonic::Request<tonic::Streaming<proto::ClientFrame>>,
        ) -> Result<tonic::Response<Self::SessionStream>, tonic::Status> {
            self.session_count.fetch_add(1, Ordering::SeqCst);
            let (sender, receiver) = mpsc::channel(1);
            tokio::spawn(async move {
                let _sender = sender;
                std::future::pending::<()>().await;
            });
            Ok(tonic::Response::new(ReceiverStream::new(receiver)))
        }

        impl_unimplemented_unary_commands!();
    }

    #[tonic::async_trait]
    impl proto::looper_realtime_server::LooperRealtime for TextChunkRealtimeSessionService {
        type SessionStream = ReceiverStream<Result<proto::ServerFrame, tonic::Status>>;

        async fn health(
            &self,
            _request: tonic::Request<proto::HealthRequest>,
        ) -> Result<tonic::Response<proto::HealthResponse>, tonic::Status> {
            Ok(tonic::Response::new(proto::HealthResponse {
                ok: true,
                service: "test".to_owned(),
                server_time: String::new(),
            }))
        }

        async fn session(
            &self,
            _request: tonic::Request<tonic::Streaming<proto::ClientFrame>>,
        ) -> Result<tonic::Response<Self::SessionStream>, tonic::Status> {
            let (sender, receiver) = mpsc::channel(1);
            let _ = sender
                .send(Ok(proto::ServerFrame {
                    frame: Some(proto::server_frame::Frame::TextChunk(proto::TextChunk {
                        seq: 44,
                        thread_id: "thread-live".to_owned(),
                        message_id: "message-live".to_owned(),
                        content: "Hello live".to_owned(),
                        is_final: false,
                        server_time: "2026-06-30T00:00:44Z".to_owned(),
                    })),
                }))
                .await;
            Ok(tonic::Response::new(ReceiverStream::new(receiver)))
        }

        impl_unimplemented_unary_commands!();
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
            payload_json: format!(
                r#"{{"id":"{session_id}","sessionId":"{session_id}","assistantSurface":"{assistant_surface}","title":"{title}"}}"#
            ),
        }
    }
}

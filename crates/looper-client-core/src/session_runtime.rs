use std::sync::Arc;

use crate::{
    client::LooperClientCore,
    error::ClientCoreError,
    local_store::LooperClientCoreLocalStore,
    menu_snapshot::{
        ClientMenuBarSessionMiniLocalSnapshot, ClientMenuSnapshotStreamUpdate,
        reduce_state_minis_menu_snapshot,
    },
    mobile_snapshot::reduce_state_minis_mobile_snapshot,
    model::{
        ClientCommandAckEnvelope, ClientEndpoint, ClientLocalStateSnapshot,
        ClientLocalStateStreamUpdate, ClientMobileSnapshotStreamUpdate,
        ClientNotificationReplyIntentResult, ClientNotificationReplyPersistResult,
        ClientSessionCommandIntentResult, ClientSessionModeIntentResult,
        ClientSessionPromptIntentResult, ClientStateMiniSnapshot, ClientStateMiniStreamUpdate,
        ClientStateMiniStreamUpdateReason, ClientStateSnapshot,
    },
};

const MOBILE_SYNC_REASON_DELTA: &str = "delta";
const MOBILE_SYNC_REASON_HEARTBEAT: &str = "heartbeat";
const MOBILE_SYNC_REASON_RECOVERY: &str = "recovery";
const MOBILE_SYNC_REASON_RECONNECTING: &str = "reconnecting";
const MODE_MUTATION_PREFIX: &str = "mode";
const PROMPT_MUTATION_PREFIX: &str = "prompt";
const NOTIFICATION_REPLY_MUTATION_PREFIX: &str = "notification-reply";
const ASSISTANT_SURFACE_MUTATION_PREFIX: &str = "assistant-surface";
const SIRI_CURRENT_MUTATION_PREFIX: &str = "siri-current";
const SIRI_DEFAULT_MUTATION_PREFIX: &str = "siri-default";
const DEFAULT_PROMPT_MUTATION_PREFIX: &str = "default-prompt";
const ARCHIVE_MUTATION_PREFIX: &str = "archive";
const DELETE_MUTATION_PREFIX: &str = "delete";
const MUTE_MUTATION_PREFIX: &str = "mute";
const LOCAL_ACCEPTED_DISPATCH_KIND: &str = "accepted";

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
        self.seed_core_from_local_store()?;
        self.client_core
            .start(endpoints, bearer_token, mobile_session_header)
    }

    pub fn stop(&self) -> Result<ClientStateSnapshot, ClientCoreError> {
        self.client_core.stop()
    }

    pub async fn observe(&self) -> Result<ClientStateMiniStreamUpdate, ClientCoreError> {
        let update = self.client_core.observe().await?;
        if update.did_change || update.reason == ClientStateMiniStreamUpdateReason::Heartbeat {
            self.persist_core_snapshot(&update.snapshot)?;
        }
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
            client_mutation_id,
        )?;
        Ok(ClientSessionModeIntentResult {
            accepted: true,
            preset,
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
            client_mutation_id,
        )?;
        Ok(ClientSessionPromptIntentResult {
            accepted: true,
            dispatch_kind: LOCAL_ACCEPTED_DISPATCH_KIND.to_owned(),
            prompt_id: String::new(),
        })
    }

    pub async fn set_assistant_surface(
        &self,
        assistant_surface: String,
    ) -> Result<ClientSessionCommandIntentResult, ClientCoreError> {
        let client_mutation_id = generated_client_mutation_id(ASSISTANT_SURFACE_MUTATION_PREFIX);
        self.client_core.accept_set_assistant_surface_durable(
            self.local_store.clone(),
            assistant_surface,
            client_mutation_id.clone(),
        )?;
        Ok(ClientSessionCommandIntentResult {
            accepted: true,
            client_mutation_id,
            entity_id: String::new(),
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

    fn seed_core_from_local_store(&self) -> Result<(), ClientCoreError> {
        let snapshot = self.local_store.snapshot()?;
        self.client_core
            .replace_state_minis(ClientStateMiniSnapshot::from(snapshot))?;
        Ok(())
    }

    fn persist_core_snapshot(
        &self,
        snapshot: &ClientStateSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.local_store
            .replace_state_minis(ClientStateMiniSnapshot::from(snapshot.clone()))
    }

    fn local_state_stream_update(
        &self,
        update: ClientStateMiniStreamUpdate,
    ) -> Result<ClientLocalStateStreamUpdate, ClientCoreError> {
        Ok(ClientLocalStateStreamUpdate {
            reason: update.reason,
            snapshot: self.local_snapshot()?,
            did_change: update.did_change,
            error_description: update.error_description,
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

        if !update.did_change {
            return Ok(ClientMobileSnapshotStreamUpdate {
                has_snapshot: false,
                snapshot_json: String::new(),
                sync_reason,
                should_stop,
                latest_seq,
                server_time,
                error_description: update.error_description,
                debug_message,
            });
        }

        let projection = reduce_state_minis_mobile_snapshot(
            latest_seq,
            update.snapshot.sessions,
            server_time.clone(),
        )?;

        Ok(ClientMobileSnapshotStreamUpdate {
            has_snapshot: projection.has_snapshot,
            snapshot_json: projection.snapshot_json,
            sync_reason,
            should_stop,
            latest_seq,
            server_time,
            error_description: update.error_description,
            debug_message,
        })
    }

    fn menu_snapshot_stream_update(
        &self,
        update: ClientLocalStateStreamUpdate,
    ) -> Result<ClientMenuSnapshotStreamUpdate, ClientCoreError> {
        let sync_reason = sync_reason(update.reason);
        let should_stop = update.reason == ClientStateMiniStreamUpdateReason::Stopped;
        let debug_message = recovery_wait_debug_message(&update);

        if !update.did_change {
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
    use crate::model::{ClientPendingCommandKind, ClientStateMini};

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
            })
            .expect("mobile projection");

        assert!(update.has_snapshot);
        assert_eq!(update.sync_reason, "delta");
        assert!(!update.should_stop);
        assert_eq!(update.latest_seq, 7);
        assert!(update.snapshot_json.contains(r#""revision":"rev-7""#));
        assert!(update.snapshot_json.contains(r#""id":"thread-main""#));
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

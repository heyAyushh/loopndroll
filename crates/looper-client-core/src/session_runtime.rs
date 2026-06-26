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
        ClientLocalStateStreamUpdate, ClientMobileSnapshotStreamUpdate, ClientStateMiniDelta,
        ClientStateMiniSnapshot, ClientStateMiniStreamUpdate, ClientStateMiniStreamUpdateReason,
        ClientStateSnapshot,
    },
};

const MOBILE_SYNC_REASON_DELTA: &str = "delta";
const MOBILE_SYNC_REASON_RECOVERY: &str = "recovery";

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
        if update.did_change {
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
                ClientStateMiniStreamUpdateReason::Stopped => {
                    return self.local_state_stream_update(update);
                }
                ClientStateMiniStreamUpdateReason::Delta
                | ClientStateMiniStreamUpdateReason::Heartbeat
                | ClientStateMiniStreamUpdateReason::Reconnecting
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

    pub fn replace_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        let snapshot = self.client_core.replace_state_minis(snapshot)?;
        self.persist_core_snapshot(&snapshot)
    }

    pub fn apply_state_mini_delta(
        &self,
        delta: ClientStateMiniDelta,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        let result = self.client_core.apply_state_mini_delta_with_result(delta)?;
        if result.did_change {
            return self.persist_core_snapshot(&result.snapshot);
        }
        self.local_snapshot_from_core_snapshot(&result.snapshot)
    }

    pub async fn set_mode(
        &self,
        thread_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        self.client_core
            .submit_set_mode_durable(
                self.local_store.clone(),
                thread_id,
                preset,
                client_mutation_id,
            )
            .await
    }

    pub fn queue_set_mode(
        &self,
        thread_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.client_core.queue_set_mode_durable(
            self.local_store.clone(),
            thread_id,
            preset,
            client_mutation_id,
        )
    }

    pub async fn send_prompt(
        &self,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        self.client_core
            .submit_send_prompt_durable(
                self.local_store.clone(),
                thread_id,
                prompt,
                assistant_surface,
                client_mutation_id,
            )
            .await
    }

    pub async fn submit_notification_reply(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        self.client_core
            .submit_notification_reply_durable(
                self.local_store.clone(),
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
    ) -> Result<ClientCommandAckEnvelope, ClientCoreError> {
        self.client_core
            .drain_notification_reply_outbox_durable(self.local_store.clone())
            .await
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

    pub fn outbox_depth(&self) -> Result<u32, ClientCoreError> {
        Ok(self.client_core.snapshot()?.outbox_depth)
    }
}

impl LooperClientCoreSessionRuntime {
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

    fn local_snapshot_from_core_snapshot(
        &self,
        snapshot: &ClientStateSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        let durable_snapshot = self.local_store.snapshot()?;
        Ok(ClientLocalStateSnapshot {
            latest_seq: snapshot.latest_seq,
            sessions: snapshot.state_minis.clone(),
            pending_commands: durable_snapshot.pending_commands,
            server_time: snapshot.server_time.clone(),
        })
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

fn sync_reason(reason: ClientStateMiniStreamUpdateReason) -> String {
    match reason {
        ClientStateMiniStreamUpdateReason::RecoveryRequired => MOBILE_SYNC_REASON_RECOVERY,
        ClientStateMiniStreamUpdateReason::Delta
        | ClientStateMiniStreamUpdateReason::Heartbeat
        | ClientStateMiniStreamUpdateReason::Reconnecting
        | ClientStateMiniStreamUpdateReason::Stopped => MOBILE_SYNC_REASON_DELTA,
    }
    .to_owned()
}

fn recovery_wait_debug_message(update: &ClientLocalStateStreamUpdate) -> String {
    if update.reason == ClientStateMiniStreamUpdateReason::RecoveryRequired
        && !update.did_change
        && !update.error_description.is_empty()
    {
        format!(
            "session-mini:client-core-stream-recovery-waiting error={}",
            update.error_description
        )
    } else {
        String::new()
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

        let error = test_runtime
            .block_on(runtime.send_prompt(
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "mutation-prompt".to_owned(),
            ))
            .expect_err("missing runtime config should fail transport");
        assert_eq!(error, ClientCoreError::NoEndpoint);

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SendSessionPrompt
        );
        assert_eq!(snapshot.pending_commands[0].thread_id, "thread-main");
        assert_eq!(snapshot.pending_commands[0].prompt, "continue");
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-prompt"
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 1);
        assert_eq!(runtime.outbox_depth().expect("outbox depth"), 1);
        drop(runtime);
        drop(test_runtime);
    }

    #[test]
    fn runtime_paints_mode_before_transport() {
        let test_runtime = tokio::runtime::Runtime::new().expect("test runtime");
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("mode-paint")).expect("runtime");
        runtime
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 7,
                sessions: vec![ClientStateMini {
                    session_id: "thread-main".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 7,
                    revision: "rev-7".to_owned(),
                    payload_json: r#"{"sessionId":"thread-main","assistantSurface":"codex","effectiveMode":"await-reply"}"#.to_owned(),
                }],
                server_time: "2026-06-25T00:00:00Z".to_owned(),
            })
            .expect("seed minis");

        let optimistic = runtime
            .queue_set_mode(
                "thread-main".to_owned(),
                "max-turns-2".to_owned(),
                "mutation-mode".to_owned(),
            )
            .expect("queue mode");

        assert_eq!(optimistic.pending_commands.len(), 1);
        assert_eq!(
            optimistic.pending_commands[0].kind,
            ClientPendingCommandKind::SetSessionMode
        );
        assert!(
            optimistic.sessions[0]
                .payload_json
                .contains(r#""effectiveMode":"max-turns-2""#)
        );

        let error = test_runtime
            .block_on(runtime.set_mode(
                "thread-main".to_owned(),
                "max-turns-2".to_owned(),
                "mutation-mode".to_owned(),
            ))
            .expect_err("missing runtime config should fail transport");
        assert_eq!(error, ClientCoreError::NoEndpoint);

        let snapshot = runtime.local_snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
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
    fn runtime_replaces_state_minis_in_core_and_durable_store() {
        let runtime =
            LooperClientCoreSessionRuntime::new(temp_store_path("state-mini")).expect("runtime");

        let snapshot = runtime
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 7,
                sessions: vec![ClientStateMini {
                    session_id: "thread-main".to_owned(),
                    assistant_surface: "codex".to_owned(),
                    seq: 7,
                    revision: "rev-7".to_owned(),
                    payload_json: r#"{"title":"Ready"}"#.to_owned(),
                }],
                server_time: "2026-06-26T00:00:00Z".to_owned(),
            })
            .expect("replace");

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

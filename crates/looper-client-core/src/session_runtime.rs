use std::sync::Arc;

use crate::{
    client::LooperClientCore,
    error::ClientCoreError,
    local_store::LooperClientCoreLocalStore,
    model::{
        ClientCommandAckEnvelope, ClientEndpoint, ClientLocalStateSnapshot, ClientStateMiniDelta,
        ClientStateMiniSnapshot, ClientStateMiniStreamUpdate, ClientStateSnapshot,
    },
};

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
}

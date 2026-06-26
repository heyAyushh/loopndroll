use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::ClientCoreError,
    model::{
        ClientLocalStateSnapshot, ClientPendingCommand, ClientPendingCommandKind, ClientStateMini,
        ClientStateMiniSnapshot,
    },
    state_mini::{normalize_state_minis, require_valid_sequence, validate_state_minis},
};

pub const DEFAULT_LOCAL_STORE_FILE_NAME: &str = "looper-realtime-state-minis.json";

#[derive(Debug, uniffi::Object)]
pub struct LooperClientCoreLocalStore {
    file_path: PathBuf,
    state: Mutex<StoredState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct StoredState {
    #[serde(rename = "latestSeq", alias = "latest_seq", default)]
    latest_seq: i64,
    #[serde(default)]
    sessions: Vec<ClientStateMini>,
    #[serde(rename = "pendingCommands", default)]
    pending_commands: Vec<StoredPendingCommand>,
    #[serde(rename = "serverTime", default)]
    server_time: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredPendingCommand {
    kind: ClientPendingCommandKind,
    #[serde(rename = "clientMutationID")]
    client_mutation_id: String,
    #[serde(rename = "threadID")]
    thread_id: String,
    #[serde(default)]
    preset: Option<String>,
    #[serde(rename = "assistantSurface", default)]
    assistant_surface: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(rename = "notificationID", default)]
    notification_id: Option<String>,
    #[serde(rename = "attemptCount", default)]
    attempt_count: u32,
}

#[uniffi::export]
impl LooperClientCoreLocalStore {
    #[uniffi::constructor]
    pub fn new(file_path: String) -> Result<Arc<Self>, ClientCoreError> {
        let file_path = require_store_path(file_path)?;
        let state = load_recovering(&file_path)?;
        Ok(Arc::new(Self {
            file_path,
            state: Mutex::new(state),
        }))
    }

    pub fn snapshot(&self) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        Ok(self.lock_state()?.snapshot())
    }

    pub fn replace_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let mut state = self.lock_state()?;
        state.latest_seq = snapshot.latest_seq;
        state.sessions = normalize_state_minis(snapshot.sessions);
        state.server_time = non_empty(snapshot.server_time);
        self.persist_locked(&state)?;
        Ok(state.snapshot())
    }

    pub fn enqueue(
        &self,
        command: ClientPendingCommand,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(
            &command.client_mutation_id,
            ClientCoreError::EmptyMutationId,
        )?;
        require_present(&command.thread_id, ClientCoreError::EmptyThreadId)?;

        let mut state = self.lock_state()?;
        let command = StoredPendingCommand::from(command);
        if let Some(existing) = state
            .pending_commands
            .iter_mut()
            .find(|pending| pending.client_mutation_id == command.client_mutation_id)
        {
            existing.kind = command.kind;
            existing.thread_id = command.thread_id;
            existing.preset = command.preset.or_else(|| existing.preset.clone());
            existing.assistant_surface = command
                .assistant_surface
                .or_else(|| existing.assistant_surface.clone());
            existing.prompt = command.prompt.or_else(|| existing.prompt.clone());
            existing.notification_id = command
                .notification_id
                .or_else(|| existing.notification_id.clone());
        } else {
            state.pending_commands.push(command);
        }
        self.persist_locked(&state)?;
        Ok(state.snapshot())
    }

    pub fn mark_attempted(
        &self,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        if let Some(command) = state
            .pending_commands
            .iter_mut()
            .find(|command| command.client_mutation_id == client_mutation_id)
        {
            command.attempt_count = command.attempt_count.saturating_add(1);
            self.persist_locked(&state)?;
        }
        Ok(state.snapshot())
    }

    pub fn mark_delivered(&self, client_mutation_id: String) -> Result<(), ClientCoreError> {
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state
            .pending_commands
            .retain(|command| command.client_mutation_id != client_mutation_id);
        self.persist_locked(&state)
    }
}

impl LooperClientCoreLocalStore {
    fn lock_state(&self) -> Result<MutexGuard<'_, StoredState>, ClientCoreError> {
        self.state
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }

    fn persist_locked(&self, state: &StoredState) -> Result<(), ClientCoreError> {
        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        }
        let data = serde_json::to_vec(state).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        let temp_path = temporary_path(&self.file_path)?;
        std::fs::write(&temp_path, data).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        std::fs::rename(temp_path, &self.file_path)
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)
    }
}

impl StoredState {
    fn snapshot(&self) -> ClientLocalStateSnapshot {
        ClientLocalStateSnapshot {
            latest_seq: self.latest_seq,
            sessions: normalize_state_minis(self.sessions.clone()),
            pending_commands: self
                .pending_commands
                .iter()
                .cloned()
                .map(ClientPendingCommand::from)
                .collect(),
            server_time: self.server_time.clone().unwrap_or_default(),
        }
    }
}

impl From<ClientPendingCommand> for StoredPendingCommand {
    fn from(command: ClientPendingCommand) -> Self {
        Self {
            kind: command.kind,
            client_mutation_id: command.client_mutation_id,
            thread_id: command.thread_id,
            preset: non_empty(command.preset),
            assistant_surface: non_empty(command.assistant_surface),
            prompt: non_empty(command.prompt),
            notification_id: non_empty(command.notification_id),
            attempt_count: command.attempt_count,
        }
    }
}

impl From<StoredPendingCommand> for ClientPendingCommand {
    fn from(command: StoredPendingCommand) -> Self {
        Self {
            kind: command.kind,
            client_mutation_id: command.client_mutation_id,
            thread_id: command.thread_id,
            preset: command.preset.unwrap_or_default(),
            assistant_surface: command.assistant_surface.unwrap_or_default(),
            prompt: command.prompt.unwrap_or_default(),
            notification_id: command.notification_id.unwrap_or_default(),
            attempt_count: command.attempt_count,
        }
    }
}

fn load_recovering(file_path: &Path) -> Result<StoredState, ClientCoreError> {
    match load(file_path) {
        Ok(state) => Ok(state),
        Err(ClientCoreError::InvalidSnapshotJson) => {
            let _ = std::fs::remove_file(file_path);
            Ok(StoredState::default())
        }
        Err(error) => Err(error),
    }
}

fn load(file_path: &Path) -> Result<StoredState, ClientCoreError> {
    if !file_path.exists() {
        return Ok(StoredState::default());
    }
    let data = std::fs::read(file_path).map_err(|_| ClientCoreError::LocalStoreReadFailed)?;
    if data.is_empty() {
        return Ok(StoredState::default());
    }
    serde_json::from_slice(&data).map_err(|_| ClientCoreError::InvalidSnapshotJson)
}

fn temporary_path(file_path: &Path) -> Result<PathBuf, ClientCoreError> {
    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ClientCoreError::LocalStorePathRequired)?;
    Ok(file_path.with_file_name(format!(".{file_name}.tmp")))
}

fn require_store_path(file_path: String) -> Result<PathBuf, ClientCoreError> {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return Err(ClientCoreError::LocalStorePathRequired);
    }
    Ok(PathBuf::from(trimmed))
}

fn require_present(value: &str, error: ClientCoreError) -> Result<(), ClientCoreError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn local_store_dedupes_outbox_attempts_and_persists_minis() {
        let path = temp_store_path("dedupe");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 1,
                sessions: vec![state_mini("thread-main", "codex", 1, "rev-1", "Cached")],
                server_time: "2026-06-24T00:00:00Z".to_owned(),
            })
            .expect("replace");
        store
            .enqueue(ClientPendingCommand {
                kind: ClientPendingCommandKind::SendSessionPrompt,
                client_mutation_id: "mutation-1".to_owned(),
                thread_id: "thread-main".to_owned(),
                preset: String::new(),
                assistant_surface: "codex".to_owned(),
                prompt: "continue".to_owned(),
                notification_id: String::new(),
                attempt_count: 0,
            })
            .expect("enqueue");
        store
            .enqueue(ClientPendingCommand {
                kind: ClientPendingCommandKind::SendSessionPrompt,
                client_mutation_id: "mutation-1".to_owned(),
                thread_id: "thread-main".to_owned(),
                preset: String::new(),
                assistant_surface: "codex".to_owned(),
                prompt: "continue".to_owned(),
                notification_id: String::new(),
                attempt_count: 0,
            })
            .expect("dedupe enqueue");
        store
            .mark_attempted("mutation-1".to_owned())
            .expect("attempt");
        let snapshot = store
            .mark_attempted("mutation-1".to_owned())
            .expect("attempt");

        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(snapshot.pending_commands[0].attempt_count, 2);
        assert_eq!(
            snapshot.sessions[0].payload_json,
            session_json("thread-main", "Cached")
        );

        drop(store);
        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let reopened_snapshot = reopened.snapshot().expect("snapshot");
        assert_eq!(reopened_snapshot.latest_seq, 1);
        assert_eq!(reopened_snapshot.pending_commands[0].attempt_count, 2);

        reopened
            .mark_delivered("mutation-1".to_owned())
            .expect("delivered");
        assert!(
            reopened
                .snapshot()
                .expect("snapshot")
                .pending_commands
                .is_empty()
        );
    }

    #[test]
    fn local_store_recovers_corrupt_cache() {
        let path = temp_store_path("corrupt");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(&path, b"not-json").expect("write corrupt");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("recover");

        let snapshot = store.snapshot().expect("snapshot");
        assert_eq!(snapshot.latest_seq, 0);
        assert!(snapshot.sessions.is_empty());
        assert!(snapshot.pending_commands.is_empty());
    }

    #[test]
    fn local_store_surfaces_read_failures() {
        let path = temp_store_path("directory");
        std::fs::create_dir_all(&path).expect("directory path");

        let error =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect_err("read");

        assert_eq!(error, ClientCoreError::LocalStoreReadFailed);
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
            payload_json: session_json(session_id, title),
        }
    }

    fn session_json(session_id: &str, title: &str) -> String {
        json!({
            "sessionId": session_id,
            "assistantSurface": "codex",
            "title": title,
        })
        .to_string()
    }

    fn temp_store_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("looper-client-core-local-store-tests")
            .join(format!("{name}-{}", std::process::id()))
            .join(DEFAULT_LOCAL_STORE_FILE_NAME)
    }
}

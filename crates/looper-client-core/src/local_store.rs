use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::ClientCoreError,
    model::{
        ClientEndpoint, ClientLocalStateSnapshot, ClientNotificationReplyRetryPlan,
        ClientPendingCommand, ClientPendingCommandKind, ClientStateMini, ClientStateMiniSnapshot,
    },
    state_mini::{normalize_state_minis, require_valid_sequence, validate_state_minis},
};

pub const DEFAULT_LOCAL_STORE_FILE_NAME: &str = "looper-realtime-state-minis.json";

const NOTIFICATION_REPLY_INITIAL_RETRY_DELAY_NANOSECONDS: u64 = 250_000_000;
const NOTIFICATION_REPLY_MAXIMUM_RETRY_DELAY_NANOSECONDS: u64 = 30_000_000_000;
const NOTIFICATION_REPLY_BACKOFF_MULTIPLIER: u64 = 2;
const MOBILE_SETTINGS_ENTITY_ID: &str = "mobile-settings";
const LEGACY_CONTROL_PAYLOAD_REVISION_FIELD: &str = "\"revision\"";
const LEGACY_CONTROL_PAYLOAD_GLOBAL_SETTINGS_FIELD: &str = "globalSettings";

#[derive(Debug)]
pub(crate) struct LooperClientCoreLocalStore {
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
    #[serde(
        rename = "lastGoodEndpointURL",
        alias = "lastGoodEndpointUrl",
        alias = "last_good_endpoint_url",
        default
    )]
    last_good_endpoint_url: Option<String>,
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
    #[serde(rename = "promptIntent", default)]
    prompt_intent: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(rename = "notificationID", default)]
    notification_id: Option<String>,
    #[serde(default)]
    archived: bool,
    #[serde(rename = "attemptCount", default)]
    attempt_count: u32,
}

impl LooperClientCoreLocalStore {
    pub(crate) fn new(file_path: String) -> Result<Arc<Self>, ClientCoreError> {
        let file_path = require_store_path(file_path)?;
        let state = load_recovering(&file_path)?;
        Ok(Arc::new(Self {
            file_path,
            state: Mutex::new(state),
        }))
    }

    pub(crate) fn snapshot(&self) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        Ok(self.lock_state()?.snapshot())
    }

    pub(crate) fn replace_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let mut state = self.lock_state()?;
        if snapshot.latest_seq < state.latest_seq {
            return Ok(state.snapshot());
        }
        state.latest_seq = snapshot.latest_seq;
        state.sessions = normalize_state_minis(snapshot.sessions);
        state.server_time = non_empty(snapshot.server_time);
        self.persist_locked(&state)?;
        Ok(state.snapshot())
    }

    pub(crate) fn endpoints_with_last_good(
        &self,
        endpoints: Vec<ClientEndpoint>,
    ) -> Result<Vec<ClientEndpoint>, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(endpoints_with_last_good(
            endpoints,
            state.last_good_endpoint_url.as_deref(),
        ))
    }

    pub(crate) fn mark_last_good_endpoint(
        &self,
        endpoint_url: String,
    ) -> Result<(), ClientCoreError> {
        let endpoint_url = normalized_endpoint_url(&endpoint_url)?;
        let mut state = self.lock_state()?;
        if state.last_good_endpoint_url.as_deref() == Some(endpoint_url.as_str()) {
            return Ok(());
        }
        state.last_good_endpoint_url = Some(endpoint_url);
        self.persist_locked(&state)
    }
}

impl LooperClientCoreLocalStore {
    pub(crate) fn enqueue(
        &self,
        command: ClientPendingCommand,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(
            &command.client_mutation_id,
            ClientCoreError::EmptyMutationId,
        )?;
        if !pending_command_allows_empty_thread_id(command.kind) {
            require_present(&command.thread_id, ClientCoreError::EmptyThreadId)?;
        }

        let mut state = self.lock_state()?;
        let command = StoredPendingCommand::from(command);
        if latest_pending_command_wins(command.kind) {
            state.pending_commands.retain(|pending| {
                pending.client_mutation_id == command.client_mutation_id
                    || !same_pending_command_target(pending, &command)
            });
        }
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
            existing.prompt_intent = command
                .prompt_intent
                .or_else(|| existing.prompt_intent.clone());
            existing.prompt = command.prompt.or_else(|| existing.prompt.clone());
            existing.notification_id = command
                .notification_id
                .or_else(|| existing.notification_id.clone());
            existing.archived = command.archived;
        } else {
            state.pending_commands.push(command);
        }
        self.persist_locked(&state)?;
        Ok(state.snapshot())
    }

    pub(crate) fn enqueue_set_mode_command(
        &self,
        thread_id: String,
        preset: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SetSessionMode,
            client_mutation_id,
            thread_id,
            preset,
            assistant_surface: String::new(),
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_send_prompt_command(
        &self,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        prompt_intent: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;
        let prompt_intent = normalized_prompt_intent(prompt_intent)?;

        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SendSessionPrompt,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface,
            prompt_intent,
            prompt,
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_notification_reply_command(
        &self,
        notification_id: String,
        thread_id: String,
        prompt: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(&notification_id, ClientCoreError::EmptyNotificationId)?;
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;

        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SubmitNotificationReply,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface,
            prompt_intent: String::new(),
            prompt,
            notification_id,
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_set_assistant_surface_command(
        &self,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(&assistant_surface, ClientCoreError::EmptySessionId)?;

        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SetAssistantSurface,
            client_mutation_id,
            thread_id: MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            preset: String::new(),
            assistant_surface,
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_set_siri_current_session_command(
        &self,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SetSiriCurrentSession,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface,
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_set_siri_default_session_command(
        &self,
        thread_id: String,
        assistant_surface: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SetSiriDefaultSession,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface,
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_save_default_prompt_command(
        &self,
        prompt: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_present(&prompt, ClientCoreError::EmptyPrompt)?;

        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SaveDefaultPrompt,
            client_mutation_id,
            thread_id: MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            preset: String::new(),
            assistant_surface: String::new(),
            prompt_intent: String::new(),
            prompt,
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_set_session_archived_command(
        &self,
        thread_id: String,
        archived: bool,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SetSessionArchived,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface: String::new(),
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_delete_session_command(
        &self,
        thread_id: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::DeleteSession,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface: String::new(),
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_mute_session_command(
        &self,
        thread_id: String,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::MuteSession,
            client_mutation_id,
            thread_id,
            preset: String::new(),
            assistant_surface: String::new(),
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn mark_attempted(
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

    pub(crate) fn mark_delivered(&self, client_mutation_id: String) -> Result<(), ClientCoreError> {
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let mut state = self.lock_state()?;
        state
            .pending_commands
            .retain(|command| command.client_mutation_id != client_mutation_id);
        self.persist_locked(&state)
    }
}

impl LooperClientCoreLocalStore {
    pub(crate) fn notification_reply_retry_plan(
        &self,
    ) -> Result<ClientNotificationReplyRetryPlan, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(state.notification_reply_retry_plan())
    }

    pub(crate) fn pending_notification_reply_command(
        &self,
    ) -> Result<Option<ClientPendingCommand>, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(state.pending_notification_reply_command())
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, StoredState>, ClientCoreError> {
        self.state
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)
    }

    fn persist_locked(&self, state: &StoredState) -> Result<(), ClientCoreError> {
        persist_state(&self.file_path, state)
    }
}

impl StoredState {
    fn repair_legacy_control_payload_cursor(&mut self) -> bool {
        if self.sessions.is_empty() || !has_legacy_control_payload(&self.sessions) {
            return false;
        }

        let latest_materialized_seq = self
            .sessions
            .iter()
            .map(|session| session.seq)
            .max()
            .unwrap_or_default();
        if self.latest_seq > latest_materialized_seq {
            self.latest_seq = latest_materialized_seq;
            return true;
        }
        false
    }

    fn coalesce_latest_pending_commands(&mut self) -> bool {
        let original_pending_commands = self.pending_commands.clone();
        let mut pending_commands = Vec::with_capacity(self.pending_commands.len());
        for command in self.pending_commands.drain(..) {
            if latest_pending_command_wins(command.kind) {
                pending_commands.retain(|pending| !same_pending_command_target(pending, &command));
            }
            pending_commands.push(command);
        }
        let changed = pending_commands != original_pending_commands;
        self.pending_commands = pending_commands;
        changed
    }

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

    fn pending_notification_reply_command(&self) -> Option<ClientPendingCommand> {
        self.pending_commands
            .iter()
            .find(|command| command.kind == ClientPendingCommandKind::SubmitNotificationReply)
            .cloned()
            .map(ClientPendingCommand::from)
    }

    fn notification_reply_retry_plan(&self) -> ClientNotificationReplyRetryPlan {
        let Some(command) = self.pending_notification_reply_command() else {
            return ClientNotificationReplyRetryPlan {
                has_pending: false,
                client_mutation_id: String::new(),
                thread_id: String::new(),
                notification_id: String::new(),
                attempt_count: 0,
                delay_nanoseconds: 0,
            };
        };

        ClientNotificationReplyRetryPlan {
            has_pending: true,
            client_mutation_id: command.client_mutation_id,
            thread_id: command.thread_id,
            notification_id: command.notification_id,
            attempt_count: command.attempt_count,
            delay_nanoseconds: notification_reply_retry_delay(command.attempt_count),
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
            prompt_intent: non_empty(command.prompt_intent),
            prompt: non_empty(command.prompt),
            notification_id: non_empty(command.notification_id),
            archived: command.archived,
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
            prompt_intent: command.prompt_intent.unwrap_or_else(default_prompt_intent),
            prompt: command.prompt.unwrap_or_default(),
            notification_id: command.notification_id.unwrap_or_default(),
            archived: command.archived,
            attempt_count: command.attempt_count,
        }
    }
}

fn load_recovering(file_path: &Path) -> Result<StoredState, ClientCoreError> {
    match load(file_path) {
        Ok(mut state) => {
            let repaired_legacy_cursor = state.repair_legacy_control_payload_cursor();
            let coalesced_pending_commands = state.coalesce_latest_pending_commands();
            if repaired_legacy_cursor || coalesced_pending_commands {
                persist_state(file_path, &state)?;
            }
            Ok(state)
        }
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

fn persist_state(file_path: &Path, state: &StoredState) -> Result<(), ClientCoreError> {
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
    }
    let data = serde_json::to_vec(state).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
    let temp_path = temporary_path(file_path)?;
    std::fs::write(&temp_path, data).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
    std::fs::rename(temp_path, file_path).map_err(|_| ClientCoreError::LocalStoreWriteFailed)
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

fn pending_command_allows_empty_thread_id(kind: ClientPendingCommandKind) -> bool {
    matches!(
        kind,
        ClientPendingCommandKind::SetSiriCurrentSession
            | ClientPendingCommandKind::SetSiriDefaultSession
    )
}

fn has_legacy_control_payload(sessions: &[ClientStateMini]) -> bool {
    sessions.iter().any(|session| {
        session
            .payload_json
            .contains(LEGACY_CONTROL_PAYLOAD_GLOBAL_SETTINGS_FIELD)
            || session
                .payload_json
                .contains(LEGACY_CONTROL_PAYLOAD_REVISION_FIELD)
    })
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn endpoints_with_last_good(
    mut endpoints: Vec<ClientEndpoint>,
    last_good_endpoint_url: Option<&str>,
) -> Vec<ClientEndpoint> {
    let Some(last_good_endpoint_url) =
        last_good_endpoint_url.and_then(|url| normalized_endpoint_url(url).ok())
    else {
        return endpoints;
    };
    if !endpoints
        .iter()
        .any(|endpoint| endpoint_matches_url(endpoint, &last_good_endpoint_url))
    {
        return endpoints;
    }

    for endpoint in &mut endpoints {
        endpoint.last_good = endpoint_matches_url(endpoint, &last_good_endpoint_url);
    }
    endpoints
}

fn endpoint_matches_url(endpoint: &ClientEndpoint, normalized_url: &str) -> bool {
    normalized_endpoint_url(&endpoint.url)
        .map(|url| url == normalized_url)
        .unwrap_or(false)
}

fn normalized_endpoint_url(endpoint_url: &str) -> Result<String, ClientCoreError> {
    let endpoint_url = endpoint_url.trim().trim_end_matches('/').to_owned();
    if endpoint_url.is_empty() {
        return Err(ClientCoreError::InvalidEndpoint);
    }
    Ok(endpoint_url)
}

fn latest_pending_command_wins(kind: ClientPendingCommandKind) -> bool {
    matches!(
        kind,
        ClientPendingCommandKind::SetSessionMode
            | ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SetSiriCurrentSession
            | ClientPendingCommandKind::SetSiriDefaultSession
            | ClientPendingCommandKind::SaveDefaultPrompt
    )
}

fn same_pending_command_target(
    existing: &StoredPendingCommand,
    command: &StoredPendingCommand,
) -> bool {
    existing.kind == command.kind
        && (pending_command_latest_wins_globally(command.kind)
            || existing.thread_id == command.thread_id)
}

fn pending_command_latest_wins_globally(kind: ClientPendingCommandKind) -> bool {
    matches!(
        kind,
        ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SetSiriCurrentSession
            | ClientPendingCommandKind::SetSiriDefaultSession
            | ClientPendingCommandKind::SaveDefaultPrompt
    )
}

fn notification_reply_retry_delay(attempt_count: u32) -> u64 {
    if attempt_count == 0 {
        return 0;
    }

    let exponent = attempt_count.saturating_sub(1);
    let multiplier = NOTIFICATION_REPLY_BACKOFF_MULTIPLIER.saturating_pow(exponent);
    NOTIFICATION_REPLY_INITIAL_RETRY_DELAY_NANOSECONDS
        .saturating_mul(multiplier)
        .min(NOTIFICATION_REPLY_MAXIMUM_RETRY_DELAY_NANOSECONDS)
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
                prompt_intent: "queue".to_owned(),
                prompt: "continue".to_owned(),
                notification_id: String::new(),
                archived: false,
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
                prompt_intent: "queue".to_owned(),
                prompt: "continue".to_owned(),
                notification_id: String::new(),
                archived: false,
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
    fn local_store_recovery_snapshot_preserves_pending_commands() {
        let path = temp_store_path("recovery-preserves-outbox");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 5,
                sessions: vec![state_mini("thread-old", "codex", 5, "rev-5", "Old")],
                server_time: "2026-06-24T00:00:00Z".to_owned(),
            })
            .expect("seed minis");
        store
            .enqueue(ClientPendingCommand {
                kind: ClientPendingCommandKind::SendSessionPrompt,
                client_mutation_id: "mutation-pending".to_owned(),
                thread_id: "thread-old".to_owned(),
                preset: String::new(),
                assistant_surface: "codex".to_owned(),
                prompt_intent: "queue".to_owned(),
                prompt: "continue".to_owned(),
                notification_id: String::new(),
                archived: false,
                attempt_count: 0,
            })
            .expect("enqueue");

        let snapshot = store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 6,
                sessions: vec![state_mini(
                    "thread-recovered",
                    "codex",
                    6,
                    "rev-6",
                    "Recovered",
                )],
                server_time: "2026-06-24T00:00:01Z".to_owned(),
            })
            .expect("recover minis");

        assert_eq!(snapshot.latest_seq, 6);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].session_id, "thread-recovered");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-pending"
        );
        assert_eq!(snapshot.pending_commands[0].thread_id, "thread-old");

        drop(store);
        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let reopened_snapshot = reopened.snapshot().expect("snapshot");
        assert_eq!(reopened_snapshot.sessions[0].session_id, "thread-recovered");
        assert_eq!(
            reopened_snapshot.pending_commands[0].client_mutation_id,
            "mutation-pending"
        );
    }

    #[test]
    fn local_store_coalesces_assistant_surface_switches_to_latest_command() {
        let path = temp_store_path("assistant-surface-latest-wins");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .enqueue_set_assistant_surface_command(
                "claude-code".to_owned(),
                "mutation-claude".to_owned(),
            )
            .expect("enqueue first surface");
        store
            .mark_attempted("mutation-claude".to_owned())
            .expect("attempt first surface");
        let snapshot = store
            .enqueue_set_assistant_surface_command("devin".to_owned(), "mutation-devin".to_owned())
            .expect("enqueue latest surface");

        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-devin"
        );
        assert_eq!(snapshot.pending_commands[0].assistant_surface, "devin");
        assert_eq!(snapshot.pending_commands[0].attempt_count, 0);

        drop(store);
        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let reopened_snapshot = reopened.snapshot().expect("snapshot");
        assert_eq!(reopened_snapshot.pending_commands.len(), 1);
        assert_eq!(
            reopened_snapshot.pending_commands[0].client_mutation_id,
            "mutation-devin"
        );
    }

    #[test]
    fn local_store_repairs_stale_assistant_surface_switches_on_load() {
        let path = temp_store_path("assistant-surface-load-repair");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "latestSeq": 5,
                "pendingCommands": [
                    {
                        "kind": "SetAssistantSurface",
                        "clientMutationID": "mutation-claude",
                        "threadID": "mobile-settings",
                        "assistantSurface": "claude-code",
                        "attemptCount": 1
                    },
                    {
                        "kind": "SetAssistantSurface",
                        "clientMutationID": "mutation-devin",
                        "threadID": "mobile-settings",
                        "assistantSurface": "devin",
                        "attemptCount": 0
                    }
                ]
            })
            .to_string(),
        )
        .expect("write stale cache");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let snapshot = store.snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-devin"
        );
        assert_eq!(snapshot.pending_commands[0].assistant_surface, "devin");

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read repaired cache"))
                .expect("repaired json");
        assert_eq!(
            persisted["pendingCommands"]
                .as_array()
                .expect("pending commands")
                .len(),
            1
        );
        assert_eq!(
            persisted["pendingCommands"][0]["clientMutationID"],
            "mutation-devin"
        );
    }

    #[test]
    fn local_store_coalesces_siri_current_session_to_latest_command() {
        let path = temp_store_path("siri-current-latest-wins");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .enqueue_set_siri_current_session_command(
                "thread-old".to_owned(),
                "codex".to_owned(),
                "mutation-old".to_owned(),
            )
            .expect("enqueue old current session");
        store
            .mark_attempted("mutation-old".to_owned())
            .expect("attempt old current session");
        let snapshot = store
            .enqueue_set_siri_current_session_command(
                "thread-new".to_owned(),
                "zed".to_owned(),
                "mutation-new".to_owned(),
            )
            .expect("enqueue new current session");

        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SetSiriCurrentSession
        );
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-new"
        );
        assert_eq!(snapshot.pending_commands[0].thread_id, "thread-new");
        assert_eq!(snapshot.pending_commands[0].assistant_surface, "zed");
        assert_eq!(snapshot.pending_commands[0].attempt_count, 0);
    }

    #[test]
    fn local_store_repairs_stale_siri_current_sessions_on_load() {
        let path = temp_store_path("siri-current-load-repair");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "latestSeq": 5,
                "pendingCommands": [
                    {
                        "kind": "SetSiriCurrentSession",
                        "clientMutationID": "mutation-old",
                        "threadID": "thread-old",
                        "assistantSurface": "codex",
                        "attemptCount": 109
                    },
                    {
                        "kind": "SetSiriCurrentSession",
                        "clientMutationID": "mutation-new",
                        "threadID": "thread-new",
                        "assistantSurface": "zed",
                        "attemptCount": 0
                    }
                ]
            })
            .to_string(),
        )
        .expect("write stale cache");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let snapshot = store.snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-new"
        );
        assert_eq!(snapshot.pending_commands[0].thread_id, "thread-new");
        assert_eq!(snapshot.pending_commands[0].assistant_surface, "zed");

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read repaired cache"))
                .expect("repaired json");
        assert_eq!(
            persisted["pendingCommands"]
                .as_array()
                .expect("pending commands")
                .len(),
            1
        );
        assert_eq!(
            persisted["pendingCommands"][0]["clientMutationID"],
            "mutation-new"
        );
    }

    #[test]
    fn local_store_persists_winning_endpoint_as_last_good() {
        let path = temp_store_path("last-good-endpoint");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .mark_last_good_endpoint(" http://100.64.0.2:8766/ ".to_owned())
            .expect("mark endpoint");
        drop(store);

        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let endpoints = reopened
            .endpoints_with_last_good(vec![
                ClientEndpoint {
                    url: "http://127.0.0.1:8766".to_owned(),
                    last_good: true,
                },
                ClientEndpoint {
                    url: "http://100.64.0.2:8766".to_owned(),
                    last_good: false,
                },
            ])
            .expect("endpoints");

        assert!(!endpoints[0].last_good);
        assert!(endpoints[1].last_good);
    }

    #[test]
    fn local_store_clamps_legacy_control_payload_cursor_on_load() {
        let path = temp_store_path("legacy-control-payload-cursor");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "latestSeq": 5246,
                "serverTime": "2026-06-27T15:35:48Z",
                "sessions": [{
                    "sessionID": "thread-old",
                    "assistantSurface": "codex",
                    "seq": 5206,
                    "revision": "rev-5206",
                    "payloadJSON": json!({
                        "sessionId": "thread-old",
                        "assistantSurface": "codex",
                        "title": "Old",
                        "revision": "rev-5206",
                        "globalSettings": {}
                    }).to_string()
                }]
            })
            .to_string(),
        )
        .expect("write legacy cache");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let snapshot = store.snapshot().expect("snapshot");

        assert_eq!(snapshot.latest_seq, 5206);
        assert_eq!(snapshot.sessions.len(), 1);
        assert_eq!(snapshot.sessions[0].seq, 5206);
    }

    #[test]
    fn typed_notification_reply_enqueue_dedupes_and_validates_payload() {
        let path = temp_store_path("typed-notification-reply");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .enqueue_notification_reply_command(
                "notification-1".to_owned(),
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "mutation-reply".to_owned(),
            )
            .expect("enqueue reply");
        let snapshot = store
            .enqueue_notification_reply_command(
                "notification-1".to_owned(),
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "mutation-reply".to_owned(),
            )
            .expect("dedupe reply");

        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SubmitNotificationReply
        );
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-reply"
        );
        assert_eq!(
            snapshot.pending_commands[0].notification_id,
            "notification-1"
        );
        assert_eq!(snapshot.pending_commands[0].attempt_count, 0);

        let error = store
            .enqueue_notification_reply_command(
                String::new(),
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "mutation-invalid".to_owned(),
            )
            .expect_err("notification id required");
        assert_eq!(error, ClientCoreError::EmptyNotificationId);
    }

    #[test]
    fn notification_reply_retry_plan_selects_pending_reply_with_capped_backoff() {
        let path = temp_store_path("notification-reply-retry-plan");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        assert_eq!(
            store
                .notification_reply_retry_plan()
                .expect("empty retry plan"),
            ClientNotificationReplyRetryPlan {
                has_pending: false,
                client_mutation_id: String::new(),
                thread_id: String::new(),
                notification_id: String::new(),
                attempt_count: 0,
                delay_nanoseconds: 0,
            }
        );

        store
            .enqueue_notification_reply_command(
                "notification-1".to_owned(),
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "mutation-reply".to_owned(),
            )
            .expect("enqueue reply");

        assert_eq!(
            store
                .notification_reply_retry_plan()
                .expect("initial retry plan"),
            ClientNotificationReplyRetryPlan {
                has_pending: true,
                client_mutation_id: "mutation-reply".to_owned(),
                thread_id: "thread-main".to_owned(),
                notification_id: "notification-1".to_owned(),
                attempt_count: 0,
                delay_nanoseconds: 0,
            }
        );

        store
            .mark_attempted("mutation-reply".to_owned())
            .expect("first attempt");
        store
            .mark_attempted("mutation-reply".to_owned())
            .expect("second attempt");
        assert_eq!(
            store
                .notification_reply_retry_plan()
                .expect("backoff retry plan")
                .delay_nanoseconds,
            NOTIFICATION_REPLY_INITIAL_RETRY_DELAY_NANOSECONDS
                * NOTIFICATION_REPLY_BACKOFF_MULTIPLIER
        );

        for _ in 0..20 {
            store
                .mark_attempted("mutation-reply".to_owned())
                .expect("additional attempt");
        }
        assert_eq!(
            store
                .notification_reply_retry_plan()
                .expect("capped retry plan")
                .delay_nanoseconds,
            NOTIFICATION_REPLY_MAXIMUM_RETRY_DELAY_NANOSECONDS
        );
    }

    #[test]
    fn assistant_surface_pending_requires_ack_after_matching_recovery_snapshot() {
        let path = temp_store_path("assistant-surface-pending-requires-ack");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .enqueue_set_assistant_surface_command(
                "codex".to_owned(),
                "mutation-surface".to_owned(),
            )
            .expect("enqueue surface");
        store
            .mark_attempted("mutation-surface".to_owned())
            .expect("attempt surface");

        let snapshot = store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 42,
                sessions: vec![state_mini("thread-main", "codex", 42, "rev-42", "Cached")],
                server_time: "2026-06-24T00:00:42Z".to_owned(),
            })
            .expect("recover state minis");

        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-surface"
        );

        store
            .mark_delivered("mutation-surface".to_owned())
            .expect("ack finality");
        assert!(
            store
                .snapshot()
                .expect("snapshot")
                .pending_commands
                .is_empty()
        );
    }

    #[test]
    fn assistant_surface_pending_requires_ack_after_matching_compact_revision() {
        let path = temp_store_path("assistant-surface-pending-requires-ack-compact-revision");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .enqueue_set_assistant_surface_command(
                "claude-code".to_owned(),
                "mutation-surface".to_owned(),
            )
            .expect("enqueue surface");
        store
            .mark_attempted("mutation-surface".to_owned())
            .expect("attempt surface");

        let snapshot = store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 42,
                sessions: vec![state_mini(
                    "thread-main",
                    "codex",
                    42,
                    "threads=thread-main:surface=claude-code:mobile-state=hash",
                    "Cached",
                )],
                server_time: "2026-06-24T00:00:42Z".to_owned(),
            })
            .expect("recover compact state minis");

        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-surface"
        );

        store
            .mark_delivered("mutation-surface".to_owned())
            .expect("ack finality");
        assert!(
            store
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
        let path = std::env::temp_dir()
            .join("looper-client-core-local-store-tests")
            .join(format!("{name}-{}", std::process::id()))
            .join(DEFAULT_LOCAL_STORE_FILE_NAME);
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir_all(parent);
        }
        path
    }
}

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    error::ClientCoreError,
    model::{
        ClientEndpoint, ClientLocalStateSnapshot, ClientNotificationReplyRetryPlan,
        ClientPendingCommand, ClientPendingCommandKind, ClientSessionDetailProjection,
        ClientSessionLatestReply, ClientStateMini, ClientStateMiniSnapshot, ClientTextChunk,
    },
    state_mini::{
        fresh_state_mini_snapshot_covered_node_ids, last_seq_by_node_from_minis,
        normalize_state_minis, require_valid_sequence, sort_state_minis, state_mini_key,
        state_mini_snapshot_is_stale_for_all_nodes, state_mini_snapshot_last_seq_by_node,
        validate_state_minis, StateMiniKey, DEFAULT_NODE_ID,
    },
};

pub const DEFAULT_LOCAL_STORE_FILE_NAME: &str = "looper-realtime-state-minis.json";

const NOTIFICATION_REPLY_INITIAL_RETRY_DELAY_NANOSECONDS: u64 = 250_000_000;
const NOTIFICATION_REPLY_MAXIMUM_RETRY_DELAY_NANOSECONDS: u64 = 30_000_000_000;
const NOTIFICATION_REPLY_BACKOFF_MULTIPLIER: u64 = 2;
const MOBILE_SETTINGS_ENTITY_ID: &str = "mobile-settings";
const LEGACY_CONTROL_PAYLOAD_REVISION_FIELD: &str = "\"revision\"";
const LEGACY_CONTROL_PAYLOAD_GLOBAL_SETTINGS_FIELD: &str = "globalSettings";
const MAX_LATEST_REPLY_BYTES: usize = 64 * 1024;

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
    #[serde(rename = "lastSeqByNode", alias = "last_seq_by_node", default)]
    last_seq_by_node: BTreeMap<String, i64>,
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
    #[serde(
        rename = "latestReplies",
        default,
        deserialize_with = "deserialize_latest_replies"
    )]
    latest_replies: HashMap<String, ClientSessionLatestReply>,
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
    #[serde(rename = "notificationTargetIds", default)]
    notification_target_ids: Vec<String>,
    #[serde(default)]
    archived: bool,
    #[serde(rename = "attemptCount", default)]
    attempt_count: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredLatestReplies {
    Keyed(HashMap<String, ClientSessionLatestReply>),
    LegacyList(Vec<ClientSessionLatestReply>),
}

fn deserialize_latest_replies<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, ClientSessionLatestReply>, D::Error>
where
    D: Deserializer<'de>,
{
    match StoredLatestReplies::deserialize(deserializer)? {
        StoredLatestReplies::Keyed(replies) => Ok(normalized_latest_replies(replies)),
        StoredLatestReplies::LegacyList(replies) => Ok(normalized_latest_replies(
            replies
                .into_iter()
                .map(|reply| (reply.session_id.clone(), reply)),
        )),
    }
}

fn normalized_latest_replies(
    replies: impl IntoIterator<Item = (String, ClientSessionLatestReply)>,
) -> HashMap<String, ClientSessionLatestReply> {
    let mut normalized: HashMap<String, ClientSessionLatestReply> = HashMap::new();
    for (key, mut reply) in replies {
        let session_id = trimmed_session_id(&reply.session_id)
            .or_else(|| trimmed_session_id(&key))
            .unwrap_or_default();
        if session_id.is_empty() {
            continue;
        }
        reply.session_id = session_id.clone();
        match normalized.get(&session_id) {
            Some(current) if current.latest_seq > reply.latest_seq => {}
            _ => {
                normalized.insert(session_id, reply);
            }
        }
    }
    normalized
}

fn trimmed_session_id(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
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

    pub(crate) fn pending_commands(&self) -> Result<Vec<ClientPendingCommand>, ClientCoreError> {
        Ok(self
            .lock_state()?
            .pending_commands
            .iter()
            .cloned()
            .map(ClientPendingCommand::from)
            .collect())
    }

    pub(crate) fn session_detail(
        &self,
        session_id: String,
    ) -> Result<ClientSessionDetailProjection, ClientCoreError> {
        let session_id = required_session_id(session_id)?;
        Ok(self.lock_state()?.session_detail(&session_id))
    }

    pub(crate) fn apply_text_chunk(
        &self,
        chunk: ClientTextChunk,
    ) -> Result<ClientSessionDetailProjection, ClientCoreError> {
        require_valid_sequence(chunk.seq)?;
        let session_id = required_session_id(chunk.thread_id.clone())?;

        let mut state = self.lock_state()?;
        let detail = state.apply_text_chunk(session_id, chunk);
        state.latest_seq = state.latest_seq.max(detail.latest_reply.latest_seq);
        if let Some(server_time) = non_empty(detail.latest_reply.server_time.clone()) {
            state.server_time = Some(newer_optional_time(
                state.server_time.clone().unwrap_or_default(),
                server_time,
            ));
        }
        self.persist_locked(&state)?;
        Ok(detail)
    }

    pub(crate) fn replace_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let mut state = self.lock_state()?;
        let sessions = normalize_state_minis(snapshot.sessions);
        if state_mini_snapshot_is_stale_for_all_nodes(
            state.latest_seq,
            &state.last_seq_by_node,
            &state.sessions,
            snapshot.latest_seq,
            &sessions,
        ) {
            return Ok(state.snapshot());
        }
        let fresh_node_ids = fresh_state_mini_snapshot_covered_node_ids(
            state.latest_seq,
            &state.last_seq_by_node,
            &state.sessions,
            snapshot.latest_seq,
            &sessions,
        );
        let fresh_last_seq_by_node =
            state_mini_snapshot_last_seq_by_node(snapshot.latest_seq, &sessions, &fresh_node_ids);
        state.merge_snapshot_minis_preserving_newer(sessions, &fresh_last_seq_by_node);
        state.merge_last_seq_by_node_from_minis();
        state.merge_last_seq_by_node(&fresh_last_seq_by_node);
        if state.last_seq_by_node.is_empty() && snapshot.latest_seq > 0 {
            state
                .last_seq_by_node
                .insert(DEFAULT_NODE_ID.to_owned(), snapshot.latest_seq);
        }
        state.latest_seq = state.latest_seq.max(snapshot.latest_seq);
        if let Some(server_time) = non_empty(snapshot.server_time) {
            state.server_time = Some(server_time);
        }
        state.retain_latest_replies_for_visible_sessions();
        self.persist_locked(&state)?;
        Ok(state.snapshot())
    }

    pub(crate) fn replace_local_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let mut state = self.lock_state()?;
        state.sessions = normalize_state_minis(snapshot.sessions);
        state.merge_last_seq_by_node_from_minis();
        if state.last_seq_by_node.is_empty() && snapshot.latest_seq > 0 {
            state
                .last_seq_by_node
                .insert(DEFAULT_NODE_ID.to_owned(), snapshot.latest_seq);
        }
        state.latest_seq = state.latest_seq.max(snapshot.latest_seq);
        if let Some(server_time) = non_empty(snapshot.server_time) {
            state.server_time = Some(server_time);
        }
        state.retain_latest_replies_for_visible_sessions();
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
            existing.notification_target_ids = if command.notification_target_ids.is_empty() {
                existing.notification_target_ids.clone()
            } else {
                command.notification_target_ids
            };
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
            archived: false,
            attempt_count: 0,
        })
    }

    pub(crate) fn enqueue_set_default_notification_targets_command(
        &self,
        notification_target_ids: Vec<String>,
        client_mutation_id: String,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        self.enqueue(ClientPendingCommand {
            kind: ClientPendingCommandKind::SetDefaultNotificationTargets,
            client_mutation_id,
            thread_id: MOBILE_SETTINGS_ENTITY_ID.to_owned(),
            preset: String::new(),
            assistant_surface: String::new(),
            prompt_intent: String::new(),
            prompt: String::new(),
            notification_id: String::new(),
            notification_target_ids,
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
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
            notification_target_ids: Vec::new(),
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

    fn drop_legacy_assistant_surface_commands(&mut self) -> bool {
        let original_len = self.pending_commands.len();
        self.pending_commands
            .retain(|command| command.kind != ClientPendingCommandKind::SetAssistantSurface);
        self.pending_commands.len() != original_len
    }

    fn normalize_local_minis(&mut self) -> bool {
        let normalized = normalize_state_minis(self.sessions.clone());
        let changed = normalized != self.sessions;
        self.sessions = normalized;
        changed
    }

    fn repair_last_seq_by_node(&mut self) -> bool {
        let original = self.last_seq_by_node.clone();
        self.merge_last_seq_by_node_from_minis();
        if self.last_seq_by_node.is_empty() && self.latest_seq > 0 {
            self.last_seq_by_node
                .insert(DEFAULT_NODE_ID.to_owned(), self.latest_seq);
        }
        self.last_seq_by_node != original
    }

    fn merge_snapshot_minis_preserving_newer(
        &mut self,
        sessions: Vec<ClientStateMini>,
        fresh_last_seq_by_node: &BTreeMap<String, i64>,
    ) -> bool {
        let before = self.sessions.clone();
        let incoming_keys = sessions
            .iter()
            .filter_map(|session| {
                let key = state_mini_key(session);
                fresh_last_seq_by_node
                    .contains_key(key.node_id())
                    .then_some(key)
            })
            .collect::<HashSet<_>>();
        self.sessions.retain(|current| {
            let key = state_mini_key(current);
            match fresh_last_seq_by_node.get(key.node_id()) {
                Some(fresh_seq) => incoming_keys.contains(&key) || current.seq > *fresh_seq,
                None => true,
            }
        });
        let mut index_by_key: HashMap<StateMiniKey, usize> =
            HashMap::with_capacity(self.sessions.len());
        for (index, current) in self.sessions.iter().enumerate() {
            index_by_key.entry(state_mini_key(current)).or_insert(index);
        }
        let mut changed = false;
        for incoming in sessions {
            let key = state_mini_key(&incoming);
            if !fresh_last_seq_by_node.contains_key(key.node_id()) {
                continue;
            }
            if let Some(index) = index_by_key.get(&key).copied() {
                if incoming.seq >= self.sessions[index].seq {
                    changed = self.sessions[index] != incoming || changed;
                    self.sessions[index] = incoming;
                }
            } else {
                index_by_key.insert(key, self.sessions.len());
                self.sessions.push(incoming);
                changed = true;
            }
        }
        sort_state_minis(&mut self.sessions);
        changed || self.sessions != before
    }

    fn merge_last_seq_by_node_from_minis(&mut self) {
        for (node_id, seq) in last_seq_by_node_from_minis(&self.sessions) {
            let entry = self.last_seq_by_node.entry(node_id).or_default();
            *entry = (*entry).max(seq);
        }
    }

    fn merge_last_seq_by_node(&mut self, cursors: &BTreeMap<String, i64>) {
        for (node_id, seq) in cursors {
            let entry = self.last_seq_by_node.entry(node_id.clone()).or_default();
            *entry = (*entry).max(*seq);
        }
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

    fn session_detail(&self, session_id: &str) -> ClientSessionDetailProjection {
        let Some(reply) = self.latest_replies.get(session_id).cloned() else {
            return ClientSessionDetailProjection::empty(session_id.to_owned());
        };

        ClientSessionDetailProjection {
            session_id: session_id.to_owned(),
            has_latest_reply: true,
            latest_reply: reply,
        }
    }

    fn apply_text_chunk(
        &mut self,
        session_id: String,
        chunk: ClientTextChunk,
    ) -> ClientSessionDetailProjection {
        if let Some(current) = self.latest_replies.get(&session_id).cloned() {
            if chunk.seq <= current.latest_seq {
                return self.session_detail(&session_id);
            }
            self.latest_replies
                .insert(session_id.clone(), updated_latest_reply(current, chunk));
        } else {
            self.latest_replies
                .insert(session_id.clone(), latest_reply_from_chunk(chunk));
        }
        self.session_detail(&session_id)
    }

    fn retain_latest_replies_for_visible_sessions(&mut self) -> bool {
        let visible_session_ids = self
            .sessions
            .iter()
            .map(|session| session.session_id.as_str())
            .collect::<HashSet<_>>();
        let original_len = self.latest_replies.len();
        self.latest_replies
            .retain(|session_id, _| visible_session_ids.contains(session_id.as_str()));
        self.latest_replies.len() != original_len
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
            notification_target_ids: command.notification_target_ids,
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
            notification_target_ids: command.notification_target_ids,
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
            let dropped_legacy_assistant_surface_commands =
                state.drop_legacy_assistant_surface_commands();
            let normalized_local_minis = state.normalize_local_minis();
            let repaired_last_seq_by_node = state.repair_last_seq_by_node();
            if repaired_legacy_cursor
                || coalesced_pending_commands
                || dropped_legacy_assistant_surface_commands
                || normalized_local_minis
                || repaired_last_seq_by_node
            {
                persist_state(file_path, &state)?;
            }
            Ok(state)
        }
        Err(ClientCoreError::InvalidSnapshotJson) => Err(ClientCoreError::InvalidSnapshotJson),
        Err(error) => Err(error),
    }
}

fn load(file_path: &Path) -> Result<StoredState, ClientCoreError> {
    if !file_path.exists() {
        return Ok(StoredState::default());
    }
    let data = std::fs::read(file_path).map_err(|_| ClientCoreError::LocalStoreReadFailed)?;
    if data.is_empty() {
        return Err(ClientCoreError::InvalidSnapshotJson);
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

fn required_session_id(value: String) -> Result<String, ClientCoreError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ClientCoreError::EmptySessionId)
    } else {
        Ok(trimmed.to_owned())
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
            | ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SetDefaultNotificationTargets
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

fn latest_reply_from_chunk(chunk: ClientTextChunk) -> ClientSessionLatestReply {
    let (text, is_truncated) = bounded_latest_reply_text(chunk.content, false);
    ClientSessionLatestReply {
        session_id: chunk.thread_id.trim().to_owned(),
        message_id: chunk.message_id,
        text,
        latest_seq: chunk.seq,
        is_final: chunk.is_final,
        is_truncated,
        server_time: chunk.server_time,
    }
}

fn updated_latest_reply(
    current: ClientSessionLatestReply,
    chunk: ClientTextChunk,
) -> ClientSessionLatestReply {
    let message_id = non_empty(chunk.message_id).unwrap_or(current.message_id.clone());
    let same_message = current.message_id == message_id || current.message_id.trim().is_empty();
    let next_text = if same_message {
        let mut text = current.text;
        text.push_str(&chunk.content);
        text
    } else {
        chunk.content
    };
    let (text, is_truncated) =
        bounded_latest_reply_text(next_text, same_message && current.is_truncated);

    ClientSessionLatestReply {
        session_id: current.session_id,
        message_id,
        text,
        latest_seq: chunk.seq,
        is_final: chunk.is_final,
        is_truncated,
        server_time: newer_optional_time(current.server_time, chunk.server_time),
    }
}

fn bounded_latest_reply_text(value: String, was_truncated: bool) -> (String, bool) {
    if value.len() <= MAX_LATEST_REPLY_BYTES {
        return (value, was_truncated);
    }

    let start = value
        .char_indices()
        .find_map(|(index, _)| (value.len() - index <= MAX_LATEST_REPLY_BYTES).then_some(index))
        .unwrap_or(value.len());
    (value[start..].to_owned(), true)
}

fn newer_optional_time(current: String, candidate: String) -> String {
    if candidate.trim().is_empty() || (!current.is_empty() && current > candidate) {
        current
    } else {
        candidate
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
            | ClientPendingCommandKind::SetSiriCurrentSession
            | ClientPendingCommandKind::SetSiriDefaultSession
            | ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SaveDefaultPrompt
            | ClientPendingCommandKind::SetDefaultNotificationTargets
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
        ClientPendingCommandKind::SetSiriCurrentSession
            | ClientPendingCommandKind::SetSiriDefaultSession
            | ClientPendingCommandKind::SetAssistantSurface
            | ClientPendingCommandKind::SaveDefaultPrompt
            | ClientPendingCommandKind::SetDefaultNotificationTargets
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

    const MANY_STALE_LOCAL_MINI_KEYS: i64 = 2_048;
    const STALE_AND_FRESH_MINI_VERSIONS_PER_KEY: i64 = 2;
    const FIRST_LOCAL_MINI_SEQUENCE: i64 = 1;
    const TEST_ACCOUNT_ID: &str = "local-account";
    const TEST_NODE_ID: &str = "node-a";
    const TEST_ASSISTANT_SURFACE: &str = "codex";

    #[test]
    fn local_store_text_chunk_persists_bounded_latest_reply_projection() {
        let path = temp_store_path("text-chunk-latest-reply");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .apply_text_chunk(text_chunk(7, "thread-main", "message-1", "Hello ", false))
            .expect("first chunk");
        store
            .apply_text_chunk(text_chunk(8, "thread-main", "message-1", "world", true))
            .expect("second chunk");
        store
            .apply_text_chunk(text_chunk(6, "thread-main", "message-1", " stale", true))
            .expect("stale chunk ignored");

        let detail = store
            .session_detail("thread-main".to_owned())
            .expect("detail projection");
        assert!(detail.has_latest_reply);
        assert_eq!(detail.latest_reply.text, "Hello world");
        assert_eq!(detail.latest_reply.latest_seq, 8);
        assert!(detail.latest_reply.is_final);
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read cache")).expect("cache json");
        let latest_replies = persisted["latestReplies"]
            .as_object()
            .expect("keyed latest replies");
        assert_eq!(latest_replies["thread-main"]["latest_seq"], json!(8));

        drop(store);
        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let reopened_detail = reopened
            .session_detail("thread-main".to_owned())
            .expect("reopened detail projection");
        assert_eq!(reopened_detail.latest_reply.text, "Hello world");
        assert_eq!(reopened_detail.latest_reply.message_id, "message-1");
    }

    #[test]
    fn local_store_text_chunk_loads_legacy_latest_reply_list_as_keyed_projection() {
        let path = temp_store_path("text-chunk-legacy-latest-reply-list");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "latestSeq": 8,
                "sessions": [
                    state_mini("thread-main", "codex", 8, "rev-8", "Cached")
                ],
                "latestReplies": [
                    {
                        "session_id": "thread-main",
                        "message_id": "message-stale",
                        "text": "stale",
                        "latest_seq": 5,
                        "is_final": true,
                        "is_truncated": false,
                        "server_time": "2026-06-24T00:00:05Z"
                    },
                    {
                        "session_id": "thread-main",
                        "message_id": "message-fresh",
                        "text": "fresh",
                        "latest_seq": 8,
                        "is_final": false,
                        "is_truncated": false,
                        "server_time": "2026-06-24T00:00:08Z"
                    }
                ]
            })
            .to_string(),
        )
        .expect("write legacy cache");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let detail = store
            .session_detail("thread-main".to_owned())
            .expect("detail projection");
        assert_eq!(detail.latest_reply.message_id, "message-fresh");
        assert_eq!(detail.latest_reply.text, "fresh");

        store
            .apply_text_chunk(text_chunk(9, "thread-main", "message-fresh", " live", true))
            .expect("live chunk");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read cache")).expect("cache json");
        let latest_replies = persisted["latestReplies"]
            .as_object()
            .expect("keyed latest replies");
        assert_eq!(latest_replies["thread-main"]["text"], json!("fresh live"));
        assert_eq!(latest_replies["thread-main"]["latest_seq"], json!(9));
    }

    #[test]
    fn local_store_text_chunk_replaces_latest_reply_for_new_message() {
        let path = temp_store_path("text-chunk-replaces-message");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .apply_text_chunk(text_chunk(7, "thread-main", "message-1", "Old", true))
            .expect("old chunk");
        let detail = store
            .apply_text_chunk(text_chunk(9, "thread-main", "message-2", "New", false))
            .expect("new chunk");

        assert_eq!(detail.latest_reply.message_id, "message-2");
        assert_eq!(detail.latest_reply.text, "New");
        assert!(!detail.latest_reply.is_final);
    }

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
                notification_target_ids: Vec::new(),
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
                notification_target_ids: Vec::new(),
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
        assert_eq!(payload_value(&snapshot.sessions[0])["title"], "Cached");

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
    fn local_store_persists_last_seq_by_node_and_preserves_newer_node_minis() {
        let path = temp_store_path("last-seq-by-node");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 20,
                sessions: vec![
                    node_state_mini("node-a", "thread-a", "codex", 20, "rev-a-20", "stream a"),
                    node_state_mini("node-b", "thread-b", "zed", 12, "rev-b-12", "cached b"),
                ],
                server_time: "2026-06-24T00:00:00Z".to_owned(),
            })
            .expect("seed minis");
        let snapshot = store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 21,
                sessions: vec![
                    node_state_mini("node-a", "thread-a", "codex", 15, "rev-a-15", "stale a"),
                    node_state_mini(
                        "node-a",
                        "thread-a-stale-extra",
                        "codex",
                        15,
                        "rev-a-extra-15",
                        "stale extra a",
                    ),
                    node_state_mini("node-b", "thread-b", "zed", 21, "rev-b-21", "fresh b"),
                ],
                server_time: "2026-06-24T00:00:01Z".to_owned(),
            })
            .expect("merge recovery minis");

        let thread_a = snapshot
            .sessions
            .iter()
            .find(|session| session.session_id == "thread-a")
            .expect("preserved node-a");
        let thread_b = snapshot
            .sessions
            .iter()
            .find(|session| session.session_id == "thread-b")
            .expect("updated node-b");
        assert_eq!(payload_value(thread_a)["title"], "stream a");
        assert_eq!(payload_value(thread_b)["title"], "fresh b");
        assert!(
            snapshot
                .sessions
                .iter()
                .all(|session| session.session_id != "thread-a-stale-extra")
        );

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read cache")).expect("cache json");
        assert_eq!(persisted["lastSeqByNode"]["node-a"], 20);
        assert_eq!(persisted["lastSeqByNode"]["node-b"], 21);
    }

    #[test]
    fn local_store_empty_fresh_snapshot_clears_default_node_minis() {
        let path = temp_store_path("empty-fresh-snapshot");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 5,
                sessions: vec![state_mini("thread-old", "codex", 5, "rev-5", "Old")],
                server_time: "2026-06-24T00:00:00Z".to_owned(),
            })
            .expect("seed minis");

        let snapshot = store
            .replace_state_minis(ClientStateMiniSnapshot {
                latest_seq: 6,
                sessions: vec![],
                server_time: "2026-06-24T00:00:01Z".to_owned(),
            })
            .expect("apply empty fresh snapshot");

        assert_eq!(snapshot.latest_seq, 6);
        assert!(snapshot.sessions.is_empty());
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read cache")).expect("cache json");
        assert_eq!(persisted["lastSeqByNode"][DEFAULT_NODE_ID], 6);
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
                notification_target_ids: Vec::new(),
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
        assert_eq!(
            snapshot
                .sessions
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["thread-recovered"]
        );
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
        assert_eq!(
            reopened_snapshot
                .sessions
                .iter()
                .map(|session| session.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["thread-recovered"]
        );
        assert_eq!(
            reopened_snapshot.pending_commands[0].client_mutation_id,
            "mutation-pending"
        );
    }

    #[test]
    fn local_store_drops_legacy_assistant_surface_switches_on_load() {
        let path = temp_store_path("assistant-surface-load-drop");
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
                    },
                    {
                        "kind": "SetSiriCurrentSession",
                        "clientMutationID": "mutation-siri-current",
                        "threadID": "thread-main",
                        "assistantSurface": "codex",
                        "attemptCount": 2
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
            snapshot.pending_commands[0].kind,
            ClientPendingCommandKind::SetSiriCurrentSession
        );
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-siri-current"
        );
        assert_eq!(snapshot.pending_commands[0].assistant_surface, "codex");

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read repaired cache"))
                .expect("repaired json");
        let pending_commands = persisted["pendingCommands"]
            .as_array()
            .expect("pending commands");
        assert_eq!(pending_commands.len(), 1);
        assert_eq!(
            pending_commands[0]["clientMutationID"],
            "mutation-siri-current"
        );
        assert_eq!(pending_commands[0]["kind"], "SetSiriCurrentSession");
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
    fn local_store_load_normalizes_many_stale_minis_without_quadratic_scan() {
        let path = temp_store_path("many-stale-local-minis");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        let mut sessions = Vec::with_capacity(
            (MANY_STALE_LOCAL_MINI_KEYS * STALE_AND_FRESH_MINI_VERSIONS_PER_KEY) as usize,
        );
        for index in 0..MANY_STALE_LOCAL_MINI_KEYS {
            let session_id = format!("thread-{index}");
            let stale_seq = FIRST_LOCAL_MINI_SEQUENCE + index;
            let fresh_seq = stale_seq + MANY_STALE_LOCAL_MINI_KEYS;
            sessions.push(node_state_mini(
                TEST_NODE_ID,
                &session_id,
                TEST_ASSISTANT_SURFACE,
                stale_seq,
                &format!("rev-{stale_seq}"),
                &format!("stale {index}"),
            ));
            sessions.push(node_state_mini(
                TEST_NODE_ID,
                &session_id,
                TEST_ASSISTANT_SURFACE,
                fresh_seq,
                &format!("rev-{fresh_seq}"),
                &format!("fresh {index}"),
            ));
        }
        std::fs::write(
            &path,
            json!({
                "latestSeq": MANY_STALE_LOCAL_MINI_KEYS * STALE_AND_FRESH_MINI_VERSIONS_PER_KEY,
                "sessions": sessions,
            })
            .to_string(),
        )
        .expect("write stale cache");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let snapshot = store.snapshot().expect("snapshot");

        assert_eq!(snapshot.sessions.len(), MANY_STALE_LOCAL_MINI_KEYS as usize);
        for (index, session) in snapshot.sessions.iter().enumerate() {
            let expected_seq =
                FIRST_LOCAL_MINI_SEQUENCE + index as i64 + MANY_STALE_LOCAL_MINI_KEYS;
            assert_eq!(session.seq, expected_seq);
            let payload = payload_value(session);
            assert_eq!(payload["accountId"], TEST_ACCOUNT_ID);
            assert_eq!(payload["nodeId"], TEST_NODE_ID);
            assert_eq!(payload["title"], format!("fresh {index}"));
        }

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read repaired cache"))
                .expect("repaired json");
        assert_eq!(
            persisted["sessions"]
                .as_array()
                .expect("persisted sessions")
                .len(),
            MANY_STALE_LOCAL_MINI_KEYS as usize
        );
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
    fn local_store_surfaces_corrupt_cache_without_deleting() {
        let path = temp_store_path("corrupt");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(&path, b"not-json").expect("write corrupt");

        let error = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect_err("corrupt cache should not be replaced with empty state");

        assert_eq!(error, ClientCoreError::InvalidSnapshotJson);
        assert_eq!(
            std::fs::read(&path).expect("corrupt cache retained"),
            b"not-json"
        );
    }

    #[test]
    fn local_store_surfaces_empty_cache_without_seq_zero_collapse() {
        let path = temp_store_path("empty-cache");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(&path, b"").expect("write empty cache");

        let error = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect_err("empty cache should not be treated as an empty snapshot");

        assert_eq!(error, ClientCoreError::InvalidSnapshotJson);
        assert!(
            std::fs::read(&path)
                .expect("empty cache retained")
                .is_empty()
        );
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

    fn node_state_mini(
        node_id: &str,
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
            payload_json: json!({
                "accountId": "local-account",
                "nodeId": node_id,
                "sessionId": session_id,
                "assistantSurface": assistant_surface,
                "title": title,
            })
            .to_string(),
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

    fn payload_value(session: &ClientStateMini) -> serde_json::Value {
        serde_json::from_str(&session.payload_json).expect("payload json")
    }

    fn text_chunk(
        seq: i64,
        thread_id: &str,
        message_id: &str,
        content: &str,
        is_final: bool,
    ) -> ClientTextChunk {
        ClientTextChunk {
            seq,
            thread_id: thread_id.to_owned(),
            message_id: message_id.to_owned(),
            content: content.to_owned(),
            is_final,
            server_time: format!("2026-06-24T00:00:{seq:02}Z"),
        }
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

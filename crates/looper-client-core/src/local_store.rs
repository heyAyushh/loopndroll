use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    error::ClientCoreError,
    model::{
        ClientEndpoint, ClientEndpointTransport, ClientLocalStateSnapshot,
        ClientNotificationReplyRetryPlan, ClientPendingCommand, ClientPendingCommandKind,
        ClientSessionDetailProjection, ClientSessionLatestReply, ClientStateMini,
        ClientStateMiniSnapshot, ClientTextChunk,
    },
    state_mini::{
        DEFAULT_NODE_ID, fresh_state_mini_snapshot_covered_node_ids, last_seq_by_node_from_minis,
        merge_state_minis_preserving_newer, normalize_state_minis, require_valid_sequence,
        state_mini_snapshot_is_stale_for_all_nodes, state_mini_snapshot_last_seq_by_node,
        validate_state_minis,
    },
};

pub const DEFAULT_LOCAL_STORE_FILE_NAME: &str = "looper-realtime-state-minis.json";
pub const DEFAULT_LOCAL_STORE_DETAIL_FILE_NAME: &str = "looper-realtime-session-details.json";

const NOTIFICATION_REPLY_INITIAL_RETRY_DELAY_NANOSECONDS: u64 = 250_000_000;
const NOTIFICATION_REPLY_MAXIMUM_RETRY_DELAY_NANOSECONDS: u64 = 30_000_000_000;
const NOTIFICATION_REPLY_BACKOFF_MULTIPLIER: u64 = 2;
const MOBILE_SETTINGS_ENTITY_ID: &str = "mobile-settings";
const MAX_LATEST_REPLY_BYTES: usize = 64 * 1024;
pub(crate) const LOCAL_STORE_DEBOUNCE_INTERVAL: Duration = Duration::from_millis(250);
const LOCAL_STORE_MAX_STALENESS: Duration = Duration::from_secs(2);
const MAX_STORED_SESSION_DETAIL_BYTES: usize = 2 * 1024 * 1024;
const LOCAL_STORE_PERSISTER_THREAD_NAME: &str = "looper-client-core-local-store-persister";

pub(crate) struct LooperClientCoreLocalStore {
    file_path: PathBuf,
    detail_file_path: PathBuf,
    state: Mutex<StoredState>,
    persister: StorePersister,
}

impl fmt::Debug for LooperClientCoreLocalStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LooperClientCoreLocalStore")
            .field("file_path", &self.file_path)
            .field("detail_file_path", &self.detail_file_path)
            .finish_non_exhaustive()
    }
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
        default,
        skip_serializing
    )]
    last_good_endpoint_url: Option<String>,
    #[serde(rename = "lastGoodEndpoint", default)]
    last_good_endpoint: Option<StoredLastGoodEndpoint>,
    #[serde(
        rename = "latestReplies",
        default,
        deserialize_with = "deserialize_latest_replies",
        skip_serializing
    )]
    latest_replies: HashMap<String, ClientSessionLatestReply>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct StoredPrimaryState {
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
        default,
        skip_serializing
    )]
    last_good_endpoint_url: Option<String>,
    #[serde(rename = "lastGoodEndpoint", default)]
    last_good_endpoint: Option<StoredLastGoodEndpoint>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
struct StoredDetailState {
    #[serde(
        rename = "latestReplies",
        default,
        deserialize_with = "deserialize_latest_replies"
    )]
    latest_replies: HashMap<String, ClientSessionLatestReply>,
}

#[derive(Clone, Debug, Default)]
struct StorePersistSnapshot {
    primary: Option<StoredPrimaryState>,
    detail: Option<StoredDetailState>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct StoredLastGoodEndpoint {
    url: String,
    transport: ClientEndpointTransport,
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

trait LocalStoreFileWriter: Send + Sync {
    fn write_atomic(&self, file_path: &Path, data: Vec<u8>) -> Result<(), ClientCoreError>;
}

#[derive(Debug)]
struct FsLocalStoreFileWriter;

impl LocalStoreFileWriter for FsLocalStoreFileWriter {
    fn write_atomic(&self, file_path: &Path, data: Vec<u8>) -> Result<(), ClientCoreError> {
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        }
        let temp_path = temporary_path(file_path)?;
        std::fs::write(&temp_path, data).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        std::fs::rename(temp_path, file_path).map_err(|_| ClientCoreError::LocalStoreWriteFailed)
    }
}

enum StorePersisterCommand {
    Schedule(StorePersistSnapshot),
    PersistPrimaryNow(
        StoredPrimaryState,
        mpsc::Sender<Result<(), ClientCoreError>>,
    ),
    /// Durability barrier. No production caller since stream stop dropped its
    /// flush (it stalled restarts behind the write queue); tests use it to
    /// assert coalescing, and Shutdown performs the same drain on Drop.
    #[cfg_attr(not(test), allow(dead_code))]
    Flush(mpsc::Sender<Result<(), ClientCoreError>>),
    Shutdown(mpsc::Sender<Result<(), ClientCoreError>>),
}

struct StorePersister {
    sender: Mutex<Option<mpsc::Sender<StorePersisterCommand>>>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for StorePersister {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StorePersister")
            .finish_non_exhaustive()
    }
}

impl StorePersister {
    fn new(
        primary_file_path: PathBuf,
        detail_file_path: PathBuf,
        writer: Arc<dyn LocalStoreFileWriter>,
    ) -> Result<Self, ClientCoreError> {
        let (sender, receiver) = mpsc::channel();
        let handle = thread::Builder::new()
            .name(LOCAL_STORE_PERSISTER_THREAD_NAME.to_owned())
            .spawn(move || {
                StorePersisterWorker {
                    primary_file_path,
                    detail_file_path,
                    writer,
                    pending: StorePersistSnapshot::default(),
                    first_pending_at: None,
                    last_update_at: None,
                    last_error: None,
                }
                .run(receiver);
            })
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            handle: Mutex::new(Some(handle)),
        })
    }

    fn schedule(&self, snapshot: StorePersistSnapshot) -> Result<(), ClientCoreError> {
        if snapshot.is_empty() {
            return Ok(());
        }
        let sender = self
            .sender
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)?
            .clone()
            .ok_or(ClientCoreError::LocalStoreWriteFailed)?;
        sender
            .send(StorePersisterCommand::Schedule(snapshot))
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)
    }

    fn persist_primary_now(&self, primary: StoredPrimaryState) -> Result<(), ClientCoreError> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)?
            .clone()
            .ok_or(ClientCoreError::LocalStoreWriteFailed)?;
        let (ack_sender, ack_receiver) = mpsc::channel();
        sender
            .send(StorePersisterCommand::PersistPrimaryNow(
                primary, ack_sender,
            ))
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        ack_receiver
            .recv()
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)?
    }

    fn flush(&self) -> Result<(), ClientCoreError> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| ClientCoreError::StateLockPoisoned)?
            .clone()
            .ok_or(ClientCoreError::LocalStoreWriteFailed)?;
        let (ack_sender, ack_receiver) = mpsc::channel();
        sender
            .send(StorePersisterCommand::Flush(ack_sender))
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
        ack_receiver
            .recv()
            .map_err(|_| ClientCoreError::LocalStoreWriteFailed)?
    }
}

impl Drop for StorePersister {
    fn drop(&mut self) {
        let sender = self.sender.get_mut().ok().and_then(Option::take);
        if let Some(sender) = sender {
            let (ack_sender, ack_receiver) = mpsc::channel();
            let _ = sender.send(StorePersisterCommand::Shutdown(ack_sender));
            let _ = ack_receiver.recv();
        }
        if let Ok(handle) = self.handle.get_mut() {
            if let Some(handle) = handle.take() {
                let _ = handle.join();
            }
        }
    }
}

struct StorePersisterWorker {
    primary_file_path: PathBuf,
    detail_file_path: PathBuf,
    writer: Arc<dyn LocalStoreFileWriter>,
    pending: StorePersistSnapshot,
    first_pending_at: Option<Instant>,
    last_update_at: Option<Instant>,
    last_error: Option<ClientCoreError>,
}

impl StorePersistSnapshot {
    fn primary(primary: StoredPrimaryState) -> Self {
        Self {
            primary: Some(primary),
            detail: None,
        }
    }

    fn primary_and_detail(primary: StoredPrimaryState, detail: StoredDetailState) -> Self {
        Self {
            primary: Some(primary),
            detail: Some(detail),
        }
    }

    fn merge(&mut self, snapshot: StorePersistSnapshot) {
        if snapshot.primary.is_some() {
            self.primary = snapshot.primary;
        }
        if snapshot.detail.is_some() {
            self.detail = snapshot.detail;
        }
    }

    fn is_empty(&self) -> bool {
        self.primary.is_none() && self.detail.is_none()
    }
}

impl StorePersisterWorker {
    fn run(&mut self, receiver: mpsc::Receiver<StorePersisterCommand>) {
        loop {
            match self.next_command(&receiver) {
                Ok(Some(command)) => {
                    if !self.handle_command(command) {
                        break;
                    }
                }
                Ok(None) => {
                    let _ = self.flush_pending();
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = self.flush_pending();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = self.flush_pending();
                }
            }
        }
    }

    fn next_command(
        &self,
        receiver: &mpsc::Receiver<StorePersisterCommand>,
    ) -> Result<Option<StorePersisterCommand>, mpsc::RecvTimeoutError> {
        let Some(timeout) = self.next_flush_timeout() else {
            return receiver
                .recv()
                .map(Some)
                .map_err(|_| mpsc::RecvTimeoutError::Disconnected);
        };
        match receiver.recv_timeout(timeout) {
            Ok(command) => Ok(Some(command)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn next_flush_timeout(&self) -> Option<Duration> {
        if self.pending.is_empty() {
            return None;
        }
        let now = Instant::now();
        let debounce_due = self
            .last_update_at
            .map(|updated| updated + LOCAL_STORE_DEBOUNCE_INTERVAL)
            .unwrap_or(now);
        let stale_due = self
            .first_pending_at
            .map(|pending| pending + LOCAL_STORE_MAX_STALENESS)
            .unwrap_or(now);
        let due = debounce_due.min(stale_due);
        Some(due.saturating_duration_since(now))
    }

    fn handle_command(&mut self, command: StorePersisterCommand) -> bool {
        match command {
            StorePersisterCommand::Schedule(snapshot) => {
                self.schedule(snapshot);
                true
            }
            StorePersisterCommand::PersistPrimaryNow(primary, ack_sender) => {
                self.pending.primary = Some(primary);
                let result = self.flush_primary();
                let _ = ack_sender.send(result);
                true
            }
            StorePersisterCommand::Flush(ack_sender) => {
                let result = self.flush_pending();
                let _ = ack_sender.send(result);
                true
            }
            StorePersisterCommand::Shutdown(ack_sender) => {
                let result = self.flush_pending();
                let _ = ack_sender.send(result);
                false
            }
        }
    }

    fn schedule(&mut self, snapshot: StorePersistSnapshot) {
        if snapshot.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.pending.is_empty() {
            self.first_pending_at = Some(now);
        }
        self.pending.merge(snapshot);
        self.last_update_at = Some(now);
        if self
            .first_pending_at
            .is_some_and(|pending| now.duration_since(pending) >= LOCAL_STORE_MAX_STALENESS)
        {
            let _ = self.flush_pending();
        }
    }

    fn flush_pending(&mut self) -> Result<(), ClientCoreError> {
        let primary_result = self.flush_primary();
        let detail_result = self.flush_detail();
        match (primary_result, detail_result) {
            (Err(error), _) | (_, Err(error)) => Err(error),
            (Ok(()), Ok(())) => {
                self.last_error.take();
                Ok(())
            }
        }
    }

    fn flush_primary(&mut self) -> Result<(), ClientCoreError> {
        let Some(primary) = self.pending.primary.clone() else {
            return Ok(());
        };
        match persist_json(&self.primary_file_path, &primary, self.writer.as_ref()) {
            Ok(()) => {
                self.pending.primary = None;
                self.clear_pending_clock_if_empty();
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn flush_detail(&mut self) -> Result<(), ClientCoreError> {
        let Some(detail) = self.pending.detail.clone() else {
            return Ok(());
        };
        match persist_json(&self.detail_file_path, &detail, self.writer.as_ref()) {
            Ok(()) => {
                self.pending.detail = None;
                self.clear_pending_clock_if_empty();
                Ok(())
            }
            Err(error) => {
                self.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    fn clear_pending_clock_if_empty(&mut self) {
        if self.pending.is_empty() {
            self.first_pending_at = None;
            self.last_update_at = None;
        }
    }
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
        Self::new_with_writer(file_path, Arc::new(FsLocalStoreFileWriter))
    }

    fn new_with_writer(
        file_path: PathBuf,
        writer: Arc<dyn LocalStoreFileWriter>,
    ) -> Result<Arc<Self>, ClientCoreError> {
        let detail_file_path = detail_file_path(&file_path)?;
        let (state, initial_persist) =
            load_recovering(&file_path, &detail_file_path, writer.as_ref())?;
        let persister = StorePersister::new(file_path.clone(), detail_file_path.clone(), writer)?;
        let store = Arc::new(Self {
            file_path,
            detail_file_path,
            state: Mutex::new(state),
            persister,
        });
        store.schedule_debounced_persist(initial_persist)?;
        Ok(store)
    }

    #[cfg(test)]
    fn new_with_test_writer(
        file_path: String,
        writer: Arc<dyn LocalStoreFileWriter>,
    ) -> Result<Arc<Self>, ClientCoreError> {
        let file_path = require_store_path(file_path)?;
        Self::new_with_writer(file_path, writer)
    }

    pub(crate) fn flush(&self) -> Result<(), ClientCoreError> {
        self.persister.flush()
    }

    fn schedule_debounced_persist(
        &self,
        snapshot: StorePersistSnapshot,
    ) -> Result<(), ClientCoreError> {
        self.persister.schedule(snapshot)
    }

    fn persist_primary_now(&self, primary: StoredPrimaryState) -> Result<(), ClientCoreError> {
        self.persister.persist_primary_now(primary)
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

        let (detail, persist_snapshot) = {
            let mut state = self.lock_state()?;
            let detail = state.apply_text_chunk(session_id, chunk);
            state.latest_seq = state.latest_seq.max(detail.latest_reply.latest_seq);
            if let Some(server_time) = non_empty(detail.latest_reply.server_time.clone()) {
                state.server_time = Some(newer_optional_time(
                    state.server_time.clone().unwrap_or_default(),
                    server_time,
                ));
            }
            state.enforce_detail_size_cap();
            (
                detail,
                StorePersistSnapshot::primary_and_detail(
                    state.primary_state(),
                    state.detail_state(),
                ),
            )
        };
        self.schedule_debounced_persist(persist_snapshot)?;
        Ok(detail)
    }

    pub(crate) fn replace_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let (local_snapshot, persist_snapshot) = {
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
            let fresh_last_seq_by_node = state_mini_snapshot_last_seq_by_node(
                snapshot.latest_seq,
                &sessions,
                &fresh_node_ids,
            );
            merge_state_minis_preserving_newer(
                &mut state.sessions,
                sessions,
                &fresh_last_seq_by_node,
            );
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
            let detail_changed = state.retain_latest_replies_for_visible_sessions()
                || state.enforce_detail_size_cap();
            let local_snapshot = state.snapshot();
            let persist_snapshot = if detail_changed {
                StorePersistSnapshot::primary_and_detail(
                    state.primary_state(),
                    state.detail_state(),
                )
            } else {
                StorePersistSnapshot::primary(state.primary_state())
            };
            (local_snapshot, persist_snapshot)
        };
        self.schedule_debounced_persist(persist_snapshot)?;
        Ok(local_snapshot)
    }

    /// Persists the client core's own optimistic session-mini state verbatim, bypassing
    /// the freshness guard in [`Self::replace_state_minis`].
    ///
    /// This is intentional, not a shortcut: an optimistic local mutation (e.g. setting a
    /// session's mode ahead of transport ack) edits `payload_json` in place without
    /// bumping `seq`, because the mutation has not been acknowledged by the server yet.
    /// The freshness guard treats an incoming snapshot at the same seq as stale and
    /// drops it, which would silently discard the optimistic edit. The snapshot passed
    /// here always originates from the caller's own just-mutated `ClientCoreState`, not
    /// from an external or potentially-stale source, so clobbering is safe: there is
    /// nothing fresher to preserve.
    pub(crate) fn replace_local_state_minis(
        &self,
        snapshot: ClientStateMiniSnapshot,
    ) -> Result<ClientLocalStateSnapshot, ClientCoreError> {
        require_valid_sequence(snapshot.latest_seq)?;
        validate_state_minis(&snapshot.sessions)?;

        let (local_snapshot, persist_snapshot) = {
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
            let detail_changed = state.retain_latest_replies_for_visible_sessions()
                || state.enforce_detail_size_cap();
            let local_snapshot = state.snapshot();
            let persist_snapshot = if detail_changed {
                StorePersistSnapshot::primary_and_detail(
                    state.primary_state(),
                    state.detail_state(),
                )
            } else {
                StorePersistSnapshot::primary(state.primary_state())
            };
            (local_snapshot, persist_snapshot)
        };
        self.schedule_debounced_persist(persist_snapshot)?;
        Ok(local_snapshot)
    }

    pub(crate) fn endpoints_with_last_good(
        &self,
        endpoints: Vec<ClientEndpoint>,
    ) -> Result<Vec<ClientEndpoint>, ClientCoreError> {
        let state = self.lock_state()?;
        Ok(endpoints_with_last_good(
            endpoints,
            state.last_good_endpoint(),
        ))
    }

    pub(crate) fn mark_last_good_endpoint(
        &self,
        endpoint_url: String,
        transport: ClientEndpointTransport,
    ) -> Result<(), ClientCoreError> {
        let endpoint_url = normalized_endpoint_url(&endpoint_url)?;
        let persist_snapshot = {
            let mut state = self.lock_state()?;
            let next = StoredLastGoodEndpoint {
                url: endpoint_url,
                transport,
            };
            if state.last_good_endpoint.as_ref() == Some(&next) {
                return Ok(());
            }
            state.last_good_endpoint = Some(next);
            state.last_good_endpoint_url = None;
            StorePersistSnapshot::primary(state.primary_state())
        };
        self.schedule_debounced_persist(persist_snapshot)
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

        let (local_snapshot, primary) = {
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
            (state.snapshot(), state.primary_state())
        };
        self.persist_primary_now(primary)?;
        Ok(local_snapshot)
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

        let (local_snapshot, primary) = {
            let mut state = self.lock_state()?;
            let primary = if let Some(command) = state
                .pending_commands
                .iter_mut()
                .find(|command| command.client_mutation_id == client_mutation_id)
            {
                command.attempt_count = command.attempt_count.saturating_add(1);
                Some(state.primary_state())
            } else {
                None
            };
            (state.snapshot(), primary)
        };
        if let Some(primary) = primary {
            // Retry bookkeeping only: losing an attempt-count bump to a crash
            // is harmless, so it takes the debounced path. Immediate writes
            // here serialized the full store once per delivery attempt and
            // backlogged the single persister thread by tens of seconds
            // during command bursts (measured 37s on device), stalling every
            // caller that needed a durability barrier.
            self.schedule_debounced_persist(StorePersistSnapshot::primary(primary))?;
        }
        Ok(local_snapshot)
    }

    pub(crate) fn mark_delivered(&self, client_mutation_id: String) -> Result<(), ClientCoreError> {
        require_present(&client_mutation_id, ClientCoreError::EmptyMutationId)?;

        let primary = {
            let mut state = self.lock_state()?;
            state
                .pending_commands
                .retain(|command| command.client_mutation_id != client_mutation_id);
            state.primary_state()
        };
        // Ack cleanup is bookkeeping, not durability: a crash before the
        // debounced write lands merely re-sends an already-delivered command,
        // which the server-side command ledger dedupes. One full-store write
        // per ack is what built the 37s persister backlog.
        self.schedule_debounced_persist(StorePersistSnapshot::primary(primary))
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
}

impl StoredState {
    fn primary_state(&self) -> StoredPrimaryState {
        StoredPrimaryState {
            latest_seq: self.latest_seq,
            sessions: self.sessions.clone(),
            last_seq_by_node: self.last_seq_by_node.clone(),
            pending_commands: self.pending_commands.clone(),
            server_time: self.server_time.clone(),
            last_good_endpoint_url: self.last_good_endpoint_url.clone(),
            last_good_endpoint: self.last_good_endpoint.clone(),
        }
    }

    fn detail_state(&self) -> StoredDetailState {
        StoredDetailState {
            latest_replies: self.latest_replies.clone(),
        }
    }

    fn last_good_endpoint(&self) -> Option<StoredLastGoodEndpoint> {
        self.last_good_endpoint.clone().or_else(|| {
            self.last_good_endpoint_url
                .as_deref()
                .and_then(|url| normalized_endpoint_url(url).ok())
                .map(|url| StoredLastGoodEndpoint {
                    url,
                    transport: ClientEndpointTransport::H2,
                })
        })
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
            if chunk.seq < current.latest_seq {
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

    fn enforce_detail_size_cap(&mut self) -> bool {
        if latest_reply_detail_bytes(&self.latest_replies) <= MAX_STORED_SESSION_DETAIL_BYTES {
            return false;
        }

        let original_len = self.latest_replies.len();
        let mut sessions_by_age = self
            .latest_replies
            .values()
            .map(|reply| (reply.latest_seq, reply.session_id.clone()))
            .collect::<Vec<_>>();
        sessions_by_age
            .sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        for (_, session_id) in sessions_by_age {
            if latest_reply_detail_bytes(&self.latest_replies) <= MAX_STORED_SESSION_DETAIL_BYTES {
                break;
            }
            self.latest_replies.remove(&session_id);
        }
        self.latest_replies.len() != original_len
    }
}

impl From<StoredPrimaryState> for StoredState {
    fn from(primary: StoredPrimaryState) -> Self {
        Self {
            latest_seq: primary.latest_seq,
            sessions: primary.sessions,
            last_seq_by_node: primary.last_seq_by_node,
            pending_commands: primary.pending_commands,
            server_time: primary.server_time,
            last_good_endpoint_url: primary.last_good_endpoint_url,
            last_good_endpoint: primary.last_good_endpoint,
            latest_replies: HashMap::new(),
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

struct LoadedDetailState {
    latest_replies: HashMap<String, ClientSessionLatestReply>,
    overrides_primary_detail: bool,
    needs_persist: bool,
}

fn load_recovering(
    file_path: &Path,
    detail_file_path: &Path,
    writer: &dyn LocalStoreFileWriter,
) -> Result<(StoredState, StorePersistSnapshot), ClientCoreError> {
    let mut state = match load_primary(file_path) {
        Ok(state) => state,
        Err(ClientCoreError::InvalidSnapshotJson) => {
            let state = StoredState::default();
            persist_json(file_path, &state.primary_state(), writer)?;
            state
        }
        Err(error) => return Err(error),
    };

    let loaded_detail = load_detail_recovering(detail_file_path, writer)?;
    if loaded_detail.overrides_primary_detail {
        state.latest_replies = loaded_detail.latest_replies;
    }

    let legacy_combined_detail = !state.latest_replies.is_empty()
        && !loaded_detail.overrides_primary_detail
        && !detail_file_path.exists();
    let coalesced_pending_commands = state.coalesce_latest_pending_commands();
    let dropped_legacy_assistant_surface_commands = state.drop_legacy_assistant_surface_commands();
    let normalized_local_minis = state.normalize_local_minis();
    let repaired_last_seq_by_node = state.repair_last_seq_by_node();
    let detail_cap_changed = state.enforce_detail_size_cap();

    if coalesced_pending_commands
        || dropped_legacy_assistant_surface_commands
        || normalized_local_minis
        || repaired_last_seq_by_node
    {
        persist_json(file_path, &state.primary_state(), writer)?;
    }

    let mut initial_persist = StorePersistSnapshot::default();
    if legacy_combined_detail {
        initial_persist.primary = Some(state.primary_state());
    }
    if legacy_combined_detail || loaded_detail.needs_persist || detail_cap_changed {
        initial_persist.detail = Some(state.detail_state());
    }

    Ok((state, initial_persist))
}

fn load_primary(file_path: &Path) -> Result<StoredState, ClientCoreError> {
    if !file_path.exists() {
        return Ok(StoredState::default());
    }
    let data = std::fs::read(file_path).map_err(|_| ClientCoreError::LocalStoreReadFailed)?;
    if data.is_empty() {
        return Err(ClientCoreError::InvalidSnapshotJson);
    }
    serde_json::from_slice(&data).map_err(|_| ClientCoreError::InvalidSnapshotJson)
}

fn load_detail_recovering(
    file_path: &Path,
    writer: &dyn LocalStoreFileWriter,
) -> Result<LoadedDetailState, ClientCoreError> {
    match load_detail(file_path) {
        Ok(Some(detail)) => Ok(LoadedDetailState {
            latest_replies: detail.latest_replies,
            overrides_primary_detail: true,
            needs_persist: false,
        }),
        Ok(None) => Ok(LoadedDetailState {
            latest_replies: HashMap::new(),
            overrides_primary_detail: false,
            needs_persist: false,
        }),
        Err(ClientCoreError::InvalidDetailJson) => {
            persist_json(file_path, &StoredDetailState::default(), writer)?;
            Ok(LoadedDetailState {
                latest_replies: HashMap::new(),
                overrides_primary_detail: false,
                needs_persist: false,
            })
        }
        Err(error) => Err(error),
    }
}

fn load_detail(file_path: &Path) -> Result<Option<StoredDetailState>, ClientCoreError> {
    if !file_path.exists() {
        return Ok(None);
    }
    let data = std::fs::read(file_path).map_err(|_| ClientCoreError::LocalStoreReadFailed)?;
    if data.is_empty() {
        return Err(ClientCoreError::InvalidDetailJson);
    }
    serde_json::from_slice(&data)
        .map(Some)
        .map_err(|_| ClientCoreError::InvalidDetailJson)
}

fn persist_json<T: Serialize>(
    file_path: &Path,
    value: &T,
    writer: &dyn LocalStoreFileWriter,
) -> Result<(), ClientCoreError> {
    let data = serde_json::to_vec(value).map_err(|_| ClientCoreError::LocalStoreWriteFailed)?;
    writer.write_atomic(file_path, data)
}

fn temporary_path(file_path: &Path) -> Result<PathBuf, ClientCoreError> {
    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ClientCoreError::LocalStorePathRequired)?;
    Ok(file_path.with_file_name(format!(".{file_name}.tmp")))
}

fn detail_file_path(file_path: &Path) -> Result<PathBuf, ClientCoreError> {
    file_path
        .parent()
        .map(|parent| parent.join(DEFAULT_LOCAL_STORE_DETAIL_FILE_NAME))
        .or_else(|| Some(PathBuf::from(DEFAULT_LOCAL_STORE_DETAIL_FILE_NAME)))
        .ok_or(ClientCoreError::LocalStorePathRequired)
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
        merged_latest_reply_text(&current.text, &chunk.content)
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

fn merged_latest_reply_text(current: &str, incoming: &str) -> String {
    if incoming.is_empty() || incoming == current {
        current.to_owned()
    } else if incoming.starts_with(current) {
        incoming.to_owned()
    } else {
        let mut text = current.to_owned();
        text.push_str(incoming);
        text
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

fn latest_reply_detail_bytes(replies: &HashMap<String, ClientSessionLatestReply>) -> usize {
    replies
        .values()
        .map(|reply| {
            reply.session_id.len()
                + reply.message_id.len()
                + reply.text.len()
                + reply.server_time.len()
                + std::mem::size_of_val(&reply.latest_seq)
                + std::mem::size_of_val(&reply.is_final)
                + std::mem::size_of_val(&reply.is_truncated)
        })
        .sum()
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
    last_good_endpoint: Option<StoredLastGoodEndpoint>,
) -> Vec<ClientEndpoint> {
    let Some(last_good_endpoint) = last_good_endpoint else {
        return endpoints;
    };
    if !endpoints
        .iter()
        .any(|endpoint| endpoint_matches_last_good(endpoint, &last_good_endpoint))
    {
        return endpoints;
    }

    for endpoint in &mut endpoints {
        endpoint.last_good = endpoint_matches_last_good(endpoint, &last_good_endpoint);
    }
    endpoints
}

fn endpoint_matches_last_good(
    endpoint: &ClientEndpoint,
    last_good_endpoint: &StoredLastGoodEndpoint,
) -> bool {
    if endpoint.transport != last_good_endpoint.transport {
        return false;
    }
    normalized_endpoint_url(&endpoint.url)
        .map(|url| url == last_good_endpoint.url)
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
    use std::{
        sync::{
            Arc, Condvar, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use serde_json::json;

    use super::*;

    const MANY_STALE_LOCAL_MINI_KEYS: i64 = 2_048;
    const STALE_AND_FRESH_MINI_VERSIONS_PER_KEY: i64 = 2;
    const FIRST_LOCAL_MINI_SEQUENCE: i64 = 1;
    const TEST_ACCOUNT_ID: &str = "local-account";
    const TEST_NODE_ID: &str = "node-a";
    const TEST_ASSISTANT_SURFACE: &str = "codex";
    const TEST_SNAPSHOT_READ_BUDGET: Duration = Duration::from_millis(100);
    const TEST_WRITER_ENTER_TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Default)]
    struct CountingFileWriter {
        primary_writes: AtomicUsize,
        detail_writes: AtomicUsize,
    }

    impl CountingFileWriter {
        fn primary_writes(&self) -> usize {
            self.primary_writes.load(Ordering::SeqCst)
        }

        fn detail_writes(&self) -> usize {
            self.detail_writes.load(Ordering::SeqCst)
        }
    }

    impl LocalStoreFileWriter for CountingFileWriter {
        fn write_atomic(&self, file_path: &Path, data: Vec<u8>) -> Result<(), ClientCoreError> {
            if file_path.file_name().and_then(|name| name.to_str())
                == Some(DEFAULT_LOCAL_STORE_DETAIL_FILE_NAME)
            {
                self.detail_writes.fetch_add(1, Ordering::SeqCst);
            } else {
                self.primary_writes.fetch_add(1, Ordering::SeqCst);
            }
            FsLocalStoreFileWriter.write_atomic(file_path, data)
        }
    }

    #[derive(Default)]
    struct BlockingFileWriter {
        entered: (Mutex<bool>, Condvar),
        release: (Mutex<bool>, Condvar),
    }

    impl BlockingFileWriter {
        fn wait_until_entered(&self) {
            let (lock, condvar) = &self.entered;
            let mut entered = lock.lock().expect("entered lock");
            let deadline = Instant::now() + TEST_WRITER_ENTER_TIMEOUT;
            while !*entered {
                let now = Instant::now();
                assert!(now < deadline, "writer did not enter");
                let timeout = deadline.saturating_duration_since(now);
                let (next_entered, _) = condvar
                    .wait_timeout(entered, timeout)
                    .expect("entered condvar");
                entered = next_entered;
            }
        }

        fn release(&self) {
            let (lock, condvar) = &self.release;
            *lock.lock().expect("release lock") = true;
            condvar.notify_all();
        }
    }

    impl LocalStoreFileWriter for BlockingFileWriter {
        fn write_atomic(&self, file_path: &Path, data: Vec<u8>) -> Result<(), ClientCoreError> {
            {
                let (lock, condvar) = &self.entered;
                *lock.lock().expect("entered lock") = true;
                condvar.notify_all();
            }
            let (lock, condvar) = &self.release;
            let mut released = lock.lock().expect("release lock");
            while !*released {
                released = condvar.wait(released).expect("release condvar");
            }
            FsLocalStoreFileWriter.write_atomic(file_path, data)
        }
    }

    #[test]
    fn local_store_debounced_text_chunk_writes_coalesce() {
        let path = temp_store_path("debounced-text-coalesces");
        let writer = Arc::new(CountingFileWriter::default());
        let store = LooperClientCoreLocalStore::new_with_test_writer(
            path.to_string_lossy().into_owned(),
            writer.clone(),
        )
        .expect("store");

        for seq in 1..=20 {
            store
                .apply_text_chunk(text_chunk(
                    seq,
                    "thread-main",
                    "message-1",
                    "chunk ",
                    seq == 20,
                ))
                .expect("text chunk");
        }
        store.flush().expect("flush coalesced writes");

        assert_eq!(writer.primary_writes(), 1);
        assert_eq!(writer.detail_writes(), 1);
        let detail = store
            .session_detail("thread-main".to_owned())
            .expect("detail");
        assert_eq!(detail.latest_reply.latest_seq, 20);
    }

    #[test]
    fn local_store_command_ack_bookkeeping_writes_coalesce() {
        let path = temp_store_path("ack-bookkeeping-coalesces");
        let writer = Arc::new(CountingFileWriter::default());
        let store = LooperClientCoreLocalStore::new_with_test_writer(
            path.to_string_lossy().into_owned(),
            writer.clone(),
        )
        .expect("store");

        let burst_size = 20;
        for index in 0..burst_size {
            store
                .enqueue_send_prompt_command(
                    "thread-main".to_owned(),
                    format!("prompt {index}"),
                    "codex".to_owned(),
                    "queue".to_owned(),
                    format!("mutation-{index}"),
                )
                .expect("enqueue prompt");
        }
        let writes_after_enqueues = writer.primary_writes();

        for index in 0..burst_size {
            store
                .mark_attempted(format!("mutation-{index}"))
                .expect("mark attempted");
            store
                .mark_delivered(format!("mutation-{index}"))
                .expect("mark delivered");
        }
        store.flush().expect("flush coalesced ack writes");

        // 40 bookkeeping mutations (attempt + delivery per command) must
        // coalesce through the debounced persister instead of writing the
        // full store once per call — that per-ack write pattern backlogged
        // the persister by ~37s on device and stalled stream restarts.
        let bookkeeping_writes = writer.primary_writes() - writes_after_enqueues;
        assert!(
            bookkeeping_writes < burst_size,
            "expected coalesced bookkeeping writes, got {bookkeeping_writes} for {} mutations",
            burst_size * 2
        );
        let snapshot = store.snapshot().expect("snapshot");
        assert!(snapshot.pending_commands.is_empty());
    }

    #[test]
    fn local_store_pending_command_enqueue_is_durable_immediately() {
        let path = temp_store_path("pending-command-immediate-durable");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .enqueue_send_prompt_command(
                "thread-main".to_owned(),
                "continue".to_owned(),
                "codex".to_owned(),
                "queue".to_owned(),
                "mutation-immediate".to_owned(),
            )
            .expect("enqueue prompt");

        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let snapshot = reopened.snapshot().expect("snapshot");
        assert_eq!(snapshot.pending_commands.len(), 1);
        assert_eq!(
            snapshot.pending_commands[0].client_mutation_id,
            "mutation-immediate"
        );
    }

    #[test]
    fn local_store_splits_legacy_combined_detail_on_flush() {
        let path = temp_store_path("legacy-combined-detail-split");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "latestSeq": 8,
                "sessions": [
                    state_mini("thread-main", "codex", 8, "rev-8", "Cached")
                ],
                "latestReplies": {
                    "thread-main": {
                        "session_id": "thread-main",
                        "message_id": "message-1",
                        "text": "legacy detail",
                        "latest_seq": 8,
                        "is_final": true,
                        "is_truncated": false,
                        "server_time": "2026-06-24T00:00:08Z"
                    }
                }
            })
            .to_string(),
        )
        .expect("write legacy combined cache");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        store.flush().expect("flush split");

        let primary: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read primary"))
                .expect("primary json");
        assert!(primary.get("latestReplies").is_none());
        let detail: serde_json::Value =
            serde_json::from_slice(&std::fs::read(detail_store_path(&path)).expect("read detail"))
                .expect("detail json");
        assert_eq!(
            detail["latestReplies"]["thread-main"]["text"],
            json!("legacy detail")
        );
    }

    #[test]
    fn local_store_evicts_oldest_detail_when_detail_cap_is_exceeded() {
        let path = temp_store_path("detail-cap-evicts-oldest");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let session_count = (MAX_STORED_SESSION_DETAIL_BYTES / MAX_LATEST_REPLY_BYTES) + 4;
        let large_reply = "x".repeat(MAX_LATEST_REPLY_BYTES);

        for index in 0..session_count {
            store
                .apply_text_chunk(text_chunk(
                    index as i64 + 1,
                    &format!("thread-{index}"),
                    "message-1",
                    &large_reply,
                    true,
                ))
                .expect("text chunk");
        }
        store.flush().expect("flush capped detail");

        let first_detail = store
            .session_detail("thread-0".to_owned())
            .expect("first detail");
        let newest_detail = store
            .session_detail(format!("thread-{}", session_count - 1))
            .expect("newest detail");
        assert!(!first_detail.has_latest_reply);
        assert!(newest_detail.has_latest_reply);
    }

    #[test]
    fn local_store_flushes_pending_detail_on_drop() {
        let path = temp_store_path("drop-flushes-detail");
        {
            let store = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
                .expect("store");
            store
                .apply_text_chunk(text_chunk(
                    7,
                    "thread-main",
                    "message-1",
                    "drop flush",
                    true,
                ))
                .expect("text chunk");
        }

        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let detail = reopened
            .session_detail("thread-main".to_owned())
            .expect("detail");
        assert_eq!(detail.latest_reply.text, "drop flush");
    }

    #[test]
    fn local_store_snapshot_read_does_not_wait_for_slow_persist() {
        let path = temp_store_path("snapshot-not-blocked-by-persist");
        let writer = Arc::new(BlockingFileWriter::default());
        let store = LooperClientCoreLocalStore::new_with_test_writer(
            path.to_string_lossy().into_owned(),
            writer.clone(),
        )
        .expect("store");

        store
            .apply_text_chunk(text_chunk(
                7,
                "thread-main",
                "message-1",
                "slow write",
                true,
            ))
            .expect("text chunk");
        writer.wait_until_entered();

        let started = Instant::now();
        store.snapshot().expect("snapshot");
        assert!(started.elapsed() < TEST_SNAPSHOT_READ_BUDGET);
        writer.release();
        store.flush().expect("flush after release");
    }

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
        store.flush().expect("flush cache");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(detail_store_path(&path)).expect("read detail"))
                .expect("detail json");
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
        store.flush().expect("flush detail");
        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(detail_store_path(&path)).expect("read detail"))
                .expect("detail json");
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
    fn local_store_text_chunk_merges_same_seq_live_progress() {
        let path = temp_store_path("text-chunk-same-seq-progress");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .apply_text_chunk(text_chunk(7, "thread-main", "message-1", "Hello ", false))
            .expect("first chunk");
        let detail = store
            .apply_text_chunk(text_chunk(7, "thread-main", "message-1", "world", true))
            .expect("same-seq final chunk");
        store
            .apply_text_chunk(text_chunk(6, "thread-main", "message-1", " stale", true))
            .expect("lower-seq stale chunk ignored");

        assert_eq!(detail.latest_reply.text, "Hello world");
        assert_eq!(detail.latest_reply.latest_seq, 7);
        assert!(detail.latest_reply.is_final);
        assert_eq!(
            store
                .session_detail("thread-main".to_owned())
                .expect("detail projection")
                .latest_reply
                .text,
            "Hello world"
        );
    }

    #[test]
    fn local_store_text_chunk_replaces_same_seq_full_message_without_duplication() {
        let path = temp_store_path("text-chunk-same-seq-full-message");
        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");

        store
            .apply_text_chunk(text_chunk(7, "thread-main", "message-1", "Hello ", false))
            .expect("first chunk");
        store
            .apply_text_chunk(text_chunk(
                7,
                "thread-main",
                "message-1",
                "Hello world",
                false,
            ))
            .expect("same-seq full message");
        let detail = store
            .apply_text_chunk(text_chunk(
                7,
                "thread-main",
                "message-1",
                "Hello world",
                true,
            ))
            .expect("same-seq duplicate full message");

        assert_eq!(detail.latest_reply.text, "Hello world");
        assert_eq!(detail.latest_reply.latest_seq, 7);
        assert!(detail.latest_reply.is_final);
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

        store.flush().expect("flush cache");
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
        store.flush().expect("flush cache");
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
            .mark_last_good_endpoint(
                " http://100.64.0.2:8766/ ".to_owned(),
                ClientEndpointTransport::H2,
            )
            .expect("mark endpoint");
        drop(store);

        let reopened =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("reopen");
        let endpoints = reopened
            .endpoints_with_last_good(vec![
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: "http://127.0.0.1:8766".to_owned(),
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: true,
                },
                ClientEndpoint {
                    transport: crate::model::ClientEndpointTransport::H2,
                    url: "http://100.64.0.2:8766".to_owned(),
                    recovery_base_url: String::new(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
            ])
            .expect("endpoints");

        assert!(!endpoints[0].last_good);
        assert!(endpoints[1].last_good);
    }

    #[test]
    fn local_store_migrates_legacy_last_good_url_and_persists_transport_tuple() {
        let path = temp_store_path("last-good-endpoint-transport-migration");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "lastGoodEndpointURL": " http://100.64.0.2:8766/ ",
                "sessions": []
            })
            .to_string(),
        )
        .expect("legacy store");

        let store =
            LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned()).expect("store");
        let endpoints = store
            .endpoints_with_last_good(vec![
                ClientEndpoint {
                    transport: ClientEndpointTransport::H3,
                    url: "https://100.64.0.2:8766".to_owned(),
                    recovery_base_url: "http://100.64.0.2:8765".to_owned(),
                    h3_certificate_sha256: "sha256:01".to_owned(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
                ClientEndpoint {
                    transport: ClientEndpointTransport::H2,
                    url: "http://100.64.0.2:8766".to_owned(),
                    recovery_base_url: "http://100.64.0.2:8765".to_owned(),
                    h3_certificate_sha256: String::new(),
                    h3_certificate_spki_sha256: String::new(),
                    last_good: false,
                },
            ])
            .expect("endpoints");
        assert!(!endpoints[0].last_good);
        assert!(
            endpoints[1].last_good,
            "legacy URL migrates as H2 last-good"
        );

        store
            .mark_last_good_endpoint(
                " https://100.64.0.2:8766/ ".to_owned(),
                ClientEndpointTransport::H3,
            )
            .expect("mark h3 endpoint");
        drop(store);

        let persisted: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read store"))
                .expect("persisted json");
        assert_eq!(
            persisted["lastGoodEndpoint"]["url"],
            "https://100.64.0.2:8766"
        );
        assert_eq!(persisted["lastGoodEndpoint"]["transport"], "h3");
        assert!(
            persisted.get("lastGoodEndpointURL").is_none(),
            "legacy URL-only field should not be written after migration"
        );
    }

    #[test]
    fn local_store_recovers_from_invalid_last_good_endpoint_transport() {
        let path = temp_store_path("invalid-last-good-endpoint-transport");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "lastGoodEndpoint": {
                    "url": "https://100.64.0.2:8766",
                    "transport": "websocket"
                },
                "sessions": []
            })
            .to_string(),
        )
        .expect("invalid transport store");

        // The invalid tuple must not silently default to H2 — but neither may it brick
        // the runtime forever. The store resets to a fresh state (no last-good pin).
        let store = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect("invalid transport recovers with a fresh store");
        assert_eq!(store.snapshot().expect("snapshot").latest_seq, 0);
    }

    #[test]
    fn local_store_recovers_from_missing_last_good_endpoint_transport_tuple() {
        let path = temp_store_path("missing-last-good-endpoint-transport");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "lastGoodEndpoint": {
                    "url": "https://100.64.0.2:8766"
                },
                "sessions": []
            })
            .to_string(),
        )
        .expect("missing transport store");

        let store = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect("missing transport tuple recovers with a fresh store");
        assert_eq!(store.snapshot().expect("snapshot").latest_seq, 0);
    }

    #[test]
    fn local_store_load_preserves_cursor_ahead_of_materialized_minis() {
        // A payload containing "revision" or "globalSettings" is not a reliable signal
        // of a legacy control payload: normalize_state_mini_for_source_with_key injects
        // a `revision` field into every normalized session, so this shape is produced
        // by ordinary, current-format snapshots too. The stored cursor can legitimately
        // sit ahead of the newest materialized session seq (e.g. a text-chunk cursor
        // advance that has not yet produced a new session mini), and loading the store
        // must never clamp it back down -- cursor monotonicity is the invariant to keep.
        let path = temp_store_path("cursor-ahead-of-materialized-minis");
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

        assert_eq!(snapshot.latest_seq, 5246);
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
    fn local_store_recovers_from_corrupt_cache_with_fresh_state() {
        let path = temp_store_path("corrupt");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(&path, b"not-json").expect("write corrupt");

        // A corrupt cache is a cache problem, not a fatal one: the store starts fresh
        // (and rewrites the file) instead of bricking the runtime until a manual wipe.
        let store = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect("corrupt cache recovers with a fresh store");
        assert_eq!(store.snapshot().expect("snapshot").latest_seq, 0);
        assert_ne!(
            std::fs::read(&path).expect("cache rewritten"),
            b"not-json".to_vec()
        );
    }

    #[test]
    fn local_store_recovers_from_corrupt_detail_cache_without_resetting_primary() {
        let path = temp_store_path("corrupt-detail");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(
            &path,
            json!({
                "latestSeq": 5,
                "sessions": [
                    state_mini("thread-main", "codex", 5, "rev-5", "Cached")
                ]
            })
            .to_string(),
        )
        .expect("write primary");
        std::fs::write(detail_store_path(&path), b"not-json").expect("write corrupt detail");

        let store = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect("corrupt detail recovers");
        let snapshot = store.snapshot().expect("snapshot");
        assert_eq!(snapshot.latest_seq, 5);
        assert_eq!(snapshot.sessions[0].session_id, "thread-main");
        let detail = store
            .session_detail("thread-main".to_owned())
            .expect("detail");
        assert!(!detail.has_latest_reply);
        assert_ne!(
            std::fs::read(detail_store_path(&path)).expect("detail rewritten"),
            b"not-json".to_vec()
        );
    }

    #[test]
    fn local_store_recovers_from_empty_cache_with_fresh_state() {
        let path = temp_store_path("empty-cache");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
        std::fs::write(&path, b"").expect("write empty cache");

        let store = LooperClientCoreLocalStore::new(path.to_string_lossy().into_owned())
            .expect("empty cache recovers with a fresh store");
        assert_eq!(store.snapshot().expect("snapshot").latest_seq, 0);
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

    fn detail_store_path(primary_path: &Path) -> PathBuf {
        primary_path
            .parent()
            .expect("primary parent")
            .join(DEFAULT_LOCAL_STORE_DETAIL_FILE_NAME)
    }
}

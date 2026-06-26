use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum ConnectionPhase {
    Disconnected,
    Connecting,
    Ready,
    Reconnecting,
}

impl Default for ConnectionPhase {
    fn default() -> Self {
        Self::Disconnected
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, uniffi::Enum)]
pub enum ClientCommandKind {
    SetSessionMode,
    SendSessionPrompt,
    SubmitNotificationReply,
    Resume,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum OutboundSessionFrameKind {
    Command,
    Resume,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientEndpoint {
    pub url: String,
    pub last_good: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientBaseUrlRaceCandidate {
    pub base_url: String,
    pub delay_nanoseconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientPendingMutation {
    pub client_mutation_id: String,
    pub command_kind: ClientCommandKind,
    pub thread_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, uniffi::Enum)]
pub enum ClientPendingCommandKind {
    #[serde(rename = "SetSessionMode")]
    SetSessionMode,
    #[serde(rename = "SendSessionPrompt")]
    SendSessionPrompt,
    #[serde(rename = "SubmitNotificationReply")]
    SubmitNotificationReply,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct ClientPendingCommand {
    #[serde(rename = "kind")]
    pub kind: ClientPendingCommandKind,
    #[serde(rename = "clientMutationID")]
    pub client_mutation_id: String,
    #[serde(rename = "threadID")]
    pub thread_id: String,
    #[serde(default)]
    pub preset: String,
    #[serde(rename = "assistantSurface", default)]
    pub assistant_surface: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(rename = "notificationID", default)]
    pub notification_id: String,
    #[serde(rename = "attemptCount", default)]
    pub attempt_count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ClientNotificationReplyRetryPlan {
    pub(crate) has_pending: bool,
    pub(crate) client_mutation_id: String,
    pub(crate) thread_id: String,
    pub(crate) notification_id: String,
    pub(crate) attempt_count: u32,
    pub(crate) delay_nanoseconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct OutboundSessionFrame {
    pub frame_kind: OutboundSessionFrameKind,
    pub command_kind: ClientCommandKind,
    pub thread_id: String,
    pub preset: String,
    pub prompt: String,
    pub assistant_surface: String,
    pub notification_id: String,
    pub client_mutation_id: String,
    pub after_seq: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientCommandAck {
    pub accepted: bool,
    pub client_mutation_id: String,
    pub ack_seq: i64,
    pub entity_id: String,
    pub revision: String,
    pub server_time: String,
    pub idempotent_replay: bool,
    pub error_code: String,
    pub reject_reason: String,
    pub current_state: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientCommandMetadata {
    pub command_kind: ClientCommandKind,
    pub client_mutation_id: String,
    pub preset: String,
    pub dispatch_kind: String,
    pub notification_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientCommandAckEnvelope {
    pub command_kind: ClientCommandKind,
    pub ack: ClientCommandAck,
    pub preset: String,
    pub dispatch_kind: String,
    pub prompt_id: String,
    pub notification_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientCommandBatchResponse {
    pub accepted: bool,
    pub command_acks: Vec<ClientCommandAckEnvelope>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateDelta {
    pub seq: i64,
    pub entity_id: String,
    pub kind: String,
    pub revision: String,
    pub server_time: String,
    pub payload_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct ClientStateMini {
    #[serde(rename = "sessionID", alias = "sessionId")]
    pub session_id: String,
    #[serde(rename = "assistantSurface")]
    pub assistant_surface: String,
    pub seq: i64,
    pub revision: String,
    #[serde(rename = "payloadJSON", alias = "payloadJson")]
    pub payload_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct ClientStateMiniSnapshot {
    #[serde(rename = "latestSeq", alias = "latest_seq")]
    pub latest_seq: i64,
    pub sessions: Vec<ClientStateMini>,
    #[serde(rename = "serverTime")]
    pub server_time: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateMiniDelta {
    pub seq: i64,
    pub latest_seq: i64,
    pub entity_id: String,
    pub kind: String,
    pub revision: String,
    pub server_time: String,
    pub has_session: bool,
    pub session: ClientStateMini,
    pub sessions: Vec<ClientStateMini>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateSnapshot {
    pub phase: ConnectionPhase,
    pub endpoint_url: String,
    pub latest_seq: i64,
    pub revision: String,
    pub server_time: String,
    pub state_minis: Vec<ClientStateMini>,
    pub pending_mutations: Vec<ClientPendingMutation>,
    pub outbox_depth: u32,
    pub last_error: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct ClientLocalStateSnapshot {
    #[serde(rename = "latestSeq", alias = "latest_seq")]
    pub latest_seq: i64,
    pub sessions: Vec<ClientStateMini>,
    #[serde(rename = "pendingCommands")]
    pub pending_commands: Vec<ClientPendingCommand>,
    #[serde(rename = "serverTime")]
    pub server_time: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateMiniDeltaApplyResult {
    pub snapshot: ClientStateSnapshot,
    pub did_change: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum ClientStateMiniStreamUpdateReason {
    Delta,
    Heartbeat,
    Reconnecting,
    RecoveryRequired,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateMiniStreamUpdate {
    pub reason: ClientStateMiniStreamUpdateReason,
    pub snapshot: ClientStateSnapshot,
    pub did_change: bool,
    pub latest_seq: i64,
    pub error_description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientLocalStateStreamUpdate {
    pub reason: ClientStateMiniStreamUpdateReason,
    pub snapshot: ClientLocalStateSnapshot,
    pub did_change: bool,
    pub error_description: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientMobileSnapshotStreamUpdate {
    pub has_snapshot: bool,
    pub snapshot_json: String,
    pub sync_reason: String,
    pub should_stop: bool,
    pub latest_seq: i64,
    pub server_time: String,
    pub error_description: String,
    pub debug_message: String,
}

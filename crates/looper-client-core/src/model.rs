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

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
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
pub struct ClientPendingMutation {
    pub client_mutation_id: String,
    pub command_kind: ClientCommandKind,
    pub thread_id: String,
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
pub struct ClientStateDelta {
    pub seq: i64,
    pub entity_id: String,
    pub kind: String,
    pub revision: String,
    pub server_time: String,
    pub payload_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateMini {
    pub session_id: String,
    pub assistant_surface: String,
    pub seq: i64,
    pub revision: String,
    pub payload_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientStateMiniSnapshot {
    pub latest_seq: i64,
    pub sessions: Vec<ClientStateMini>,
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

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use uuid::Uuid;

mod control;
mod ids;
mod model;
mod protocol;
#[cfg(test)]
mod tests;

pub use ids::{
    acp_agent_and_session_id_for_client_public_thread_id,
    acp_session_id_for_client_public_thread_id, public_agent_id_for_client_agent_id,
    public_thread_id_for_client_acp_session, public_thread_id_for_client_agent_acp_session,
};
pub use model::{
    LooperAcpControlCancel, LooperAcpControlCancelResponse, LooperAcpControlPrompt,
    LooperAcpControlPromptResponse, LooperAcpControlSessionResponse, LooperAcpDeliveredPrompt,
    LooperAcpObservedSession, LooperAcpRuntimeSession, LooperAcpRuntimeStatus,
};
pub use protocol::supported_acp_methods;

pub const LOOPER_ACP_AGENT_ID: &str = "looper";
pub const LOOPER_ACP_AGENT_NAME: &str = "Looper";
pub const LOOPER_ACP_SUPPORTED_METHODS: &[&str] = &[
    INITIALIZE_METHOD,
    SESSION_NEW_METHOD,
    SESSION_PROMPT_METHOD,
    SESSION_CANCEL_METHOD,
];

const ACP_PROTOCOL_VERSION: u16 = 1;
pub(super) const AGENT_MESSAGE_CHUNK_UPDATE: &str = "agent_message_chunk";
pub(super) const CONTROL_PROMPT_SOURCE: &str = "looper-control";
const DEFAULT_CLIENT_ID: &str = "devin";
pub(super) const END_TURN_STOP_REASON: &str = "end_turn";
pub(super) const INITIALIZE_METHOD: &str = "initialize";
const JSON_RPC_VERSION: &str = "2.0";
pub const LOCAL_CONTROL_CONNECTION_ID: &str = "looper-local-control";
pub(super) const LOOPER_SESSION_PREFIX: &str = "acp/looper/";
pub(super) const MOBILE_PROMPT_SOURCE: &str = "looper-mobile";
pub(super) const SESSION_CANCEL_METHOD: &str = "session/cancel";
pub(super) const SESSION_NEW_METHOD: &str = "session/new";
pub(super) const SESSION_PROMPT_METHOD: &str = "session/prompt";
pub(super) const SESSION_UPDATE_METHOD: &str = "session/update";
pub(super) const TEXT_CONTENT_TYPE: &str = "text";
pub(super) const TEXT_UPDATE_DETAIL: &str =
    "Looper received this prompt through the local ACP bridge.";
pub(super) const USER_MESSAGE_CHUNK_UPDATE: &str = "user_message_chunk";

pub(super) type OutboundMessageSender = mpsc::UnboundedSender<String>;

#[derive(Clone)]
pub struct LooperAcpRuntime {
    client_id: &'static str,
    pub(super) state: Arc<Mutex<RuntimeState>>,
}

impl Default for LooperAcpRuntime {
    fn default() -> Self {
        Self::new(DEFAULT_CLIENT_ID)
    }
}

#[derive(Default)]
pub(super) struct RuntimeState {
    pub(super) connections: BTreeMap<String, RuntimeConnection>,
    pub(super) sessions: BTreeMap<String, LooperAcpRuntimeSession>,
}

pub(super) struct RuntimeConnection {
    pub(super) agent_id: String,
    pub(super) sender: OutboundMessageSender,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LooperAcpControlError {
    PromptRequired,
    DeliveryUnavailable,
    SessionNotFound,
}

impl fmt::Display for LooperAcpControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeliveryUnavailable => {
                formatter.write_str("Looper ACP session transport is unavailable")
            }
            Self::PromptRequired => formatter.write_str("prompt is required"),
            Self::SessionNotFound => formatter.write_str("Looper ACP session was not found"),
        }
    }
}

impl Error for LooperAcpControlError {}

impl LooperAcpRuntime {
    pub fn new(client_id: &'static str) -> Self {
        Self {
            client_id,
            state: Arc::new(Mutex::new(RuntimeState::default())),
        }
    }

    pub fn client_id(&self) -> &'static str {
        self.client_id
    }

    pub fn status(&self) -> LooperAcpRuntimeStatus {
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let state = self.state.lock().expect("looper acp runtime state");
        let mut sessions = state.sessions.values().cloned().collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        LooperAcpRuntimeStatus {
            connected: !state.connections.is_empty(),
            connection_count: state.connections.len(),
            session_count: sessions.len(),
            sessions,
        }
    }

    pub fn register_connection(&self, sender: OutboundMessageSender) -> String {
        self.register_connection_for_agent(LOOPER_ACP_AGENT_ID, sender)
    }

    pub fn register_connection_for_agent(
        &self,
        agent_id: impl Into<String>,
        sender: OutboundMessageSender,
    ) -> String {
        let connection_id = format!("{}-acp-{}", self.client_id, Uuid::new_v4());
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        state.connections.insert(
            connection_id.clone(),
            RuntimeConnection {
                agent_id: agent_id.into(),
                sender,
            },
        );
        connection_id
    }

    pub fn unregister_connection(&self, connection_id: &str) {
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        state.connections.remove(connection_id);
        for session in state.sessions.values_mut() {
            if session.connection_id == connection_id {
                session.cancelled = true;
                session.updated_at_ms = now_millis();
            }
        }
    }

    pub fn observe_session(&self, input: LooperAcpObservedSession) -> LooperAcpRuntimeSession {
        let now = now_millis();
        let session_key = input.session_id.clone();
        let public_thread_id = public_thread_id_for_client_agent_acp_session(
            self.client_id,
            &input.agent_id,
            &input.session_id,
        );
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        let session =
            state
                .sessions
                .entry(session_key)
                .or_insert_with(|| LooperAcpRuntimeSession {
                    public_thread_id,
                    session_id: input.session_id.clone(),
                    agent_id: input.agent_id.clone(),
                    connection_id: input
                        .connection_id
                        .clone()
                        .unwrap_or_else(|| LOCAL_CONTROL_CONNECTION_ID.to_owned()),
                    cwd: input.cwd.clone(),
                    created_at_ms: now,
                    updated_at_ms: now,
                    latest_user_prompt: None,
                    latest_assistant_message: None,
                    cancelled: false,
                });
        if let Some(connection_id) = input.connection_id {
            session.connection_id = connection_id;
        }
        if input.cwd.is_some() {
            session.cwd = input.cwd;
        }
        if input.latest_user_prompt.is_some() {
            session.latest_user_prompt = input.latest_user_prompt;
        }
        if input.latest_assistant_message.is_some() {
            session.latest_assistant_message = input.latest_assistant_message;
        }
        session.cancelled = input.cancelled;
        session.updated_at_ms = now;
        session.clone()
    }
}

pub(super) fn new_runtime_session(
    client_id: &str,
    agent_id: &str,
    session_id: &str,
    connection_id: &str,
    cwd: Option<String>,
    now: i64,
) -> LooperAcpRuntimeSession {
    LooperAcpRuntimeSession {
        public_thread_id: public_thread_id_for_client_agent_acp_session(
            client_id, agent_id, session_id,
        ),
        session_id: session_id.to_owned(),
        agent_id: agent_id.to_owned(),
        connection_id: connection_id.to_owned(),
        cwd,
        created_at_ms: now,
        updated_at_ms: now,
        latest_user_prompt: None,
        latest_assistant_message: None,
        cancelled: false,
    }
}

pub(super) fn agent_id_for_connection(state: &RuntimeState, connection_id: &str) -> String {
    state
        .connections
        .get(connection_id)
        .map(|connection| connection.agent_id.clone())
        .unwrap_or_else(|| LOOPER_ACP_AGENT_ID.to_owned())
}

pub(super) fn now_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    i64::try_from(millis).unwrap_or(i64::MAX)
}

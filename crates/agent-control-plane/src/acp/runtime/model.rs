use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpRuntimeStatus {
    pub connected: bool,
    pub connection_count: usize,
    pub session_count: usize,
    pub sessions: Vec<LooperAcpRuntimeSession>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpRuntimeSession {
    pub session_id: String,
    pub public_thread_id: String,
    pub agent_id: String,
    pub connection_id: String,
    pub cwd: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub latest_user_prompt: Option<String>,
    pub latest_assistant_message: Option<String>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpDeliveredPrompt {
    pub prompt_id: String,
    pub session_id: String,
    pub public_thread_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpControlPrompt {
    pub prompt_id: String,
    pub session_id: String,
    pub public_thread_id: String,
    pub delivered_to_connection: bool,
    pub session: LooperAcpRuntimeSession,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpControlCancel {
    pub session_id: String,
    pub public_thread_id: String,
    pub session: LooperAcpRuntimeSession,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpControlSessionResponse {
    pub session: LooperAcpRuntimeSession,
    pub runtime: LooperAcpRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpControlPromptResponse {
    pub prompt_id: String,
    pub delivered_to_connection: bool,
    pub session: LooperAcpRuntimeSession,
    pub runtime: LooperAcpRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpControlCancelResponse {
    pub session: LooperAcpRuntimeSession,
    pub runtime: LooperAcpRuntimeStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LooperAcpObservedSession {
    pub agent_id: String,
    pub session_id: String,
    pub connection_id: Option<String>,
    pub cwd: Option<String>,
    pub latest_user_prompt: Option<String>,
    pub latest_assistant_message: Option<String>,
    pub cancelled: bool,
}

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

pub const LOOPER_ACP_AGENT_ID: &str = "looper";
pub const LOOPER_ACP_AGENT_NAME: &str = "Looper";
pub const LOOPER_ACP_ROUTE: &str = "/acp/client-hosts/devin";
pub const LEGACY_LOOPER_ACP_ROUTE: &str = "/acp/devin";

const ACP_PROTOCOL_VERSION: u16 = 1;
const AGENT_MESSAGE_CHUNK_UPDATE: &str = "agent_message_chunk";
const END_TURN_STOP_REASON: &str = "end_turn";
const INITIALIZE_METHOD: &str = "initialize";
const JSON_RPC_VERSION: &str = "2.0";
const LOOPER_SESSION_PREFIX: &str = "acp/looper/";
const MOBILE_PROMPT_SOURCE: &str = "looper-mobile";
const SESSION_CANCEL_METHOD: &str = "session/cancel";
const SESSION_NEW_METHOD: &str = "session/new";
const SESSION_PROMPT_METHOD: &str = "session/prompt";
const SESSION_UPDATE_METHOD: &str = "session/update";
const TEXT_CONTENT_TYPE: &str = "text";
const TEXT_UPDATE_DETAIL: &str = "Looper received this prompt through the local ACP bridge.";
const USER_MESSAGE_CHUNK_UPDATE: &str = "user_message_chunk";

type OutboundMessageSender = mpsc::UnboundedSender<String>;

#[derive(Clone, Default)]
pub struct DevinAcpRuntime {
    state: Arc<Mutex<RuntimeState>>,
}

#[derive(Default)]
struct RuntimeState {
    connections: BTreeMap<String, OutboundMessageSender>,
    sessions: BTreeMap<String, DevinAcpRuntimeSession>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpRuntimeStatus {
    pub connected: bool,
    pub connection_count: usize,
    pub session_count: usize,
    pub sessions: Vec<DevinAcpRuntimeSession>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpRuntimeSession {
    pub session_id: String,
    pub public_thread_id: String,
    pub connection_id: String,
    pub cwd: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub latest_user_prompt: Option<String>,
    pub latest_assistant_message: Option<String>,
    pub cancelled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevinAcpDeliveredPrompt {
    pub prompt_id: String,
    pub session_id: String,
    pub public_thread_id: String,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

impl DevinAcpRuntime {
    pub fn status(&self) -> DevinAcpRuntimeStatus {
        let state = self.state.lock().expect("devin acp runtime state");
        let mut sessions = state.sessions.values().cloned().collect::<Vec<_>>();
        sessions.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        DevinAcpRuntimeStatus {
            connected: !state.connections.is_empty(),
            connection_count: state.connections.len(),
            session_count: sessions.len(),
            sessions,
        }
    }

    pub fn register_connection(&self, sender: OutboundMessageSender) -> String {
        let connection_id = format!("devin-acp-{}", Uuid::new_v4());
        let mut state = self.state.lock().expect("devin acp runtime state");
        state.connections.insert(connection_id.clone(), sender);
        connection_id
    }

    pub fn unregister_connection(&self, connection_id: &str) {
        let mut state = self.state.lock().expect("devin acp runtime state");
        state.connections.remove(connection_id);
        for session in state.sessions.values_mut() {
            if session.connection_id == connection_id {
                session.cancelled = true;
                session.updated_at_ms = now_millis();
            }
        }
    }

    pub fn handle_text_message(&self, connection_id: &str, text: &str) -> Vec<String> {
        let parsed = serde_json::from_str::<JsonRpcRequest>(text);
        match parsed {
            Ok(request) => self.handle_request(connection_id, request),
            Err(error) => vec![json_rpc_error(None, -32700, &error.to_string())],
        }
    }

    pub fn deliver_mobile_prompt(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<DevinAcpDeliveredPrompt> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(anyhow!("prompt is required"));
        }

        let mut state = self.state.lock().expect("devin acp runtime state");
        let session_key = runtime_session_key(session_id);
        let (connection_id, public_thread_id) = {
            let session = state
                .sessions
                .get_mut(&session_key)
                .ok_or_else(|| anyhow!("Looper ACP session is not connected"))?;
            session.latest_user_prompt = Some(prompt.to_owned());
            session.updated_at_ms = now_millis();
            (
                session.connection_id.clone(),
                session.public_thread_id.clone(),
            )
        };
        let sender = state
            .connections
            .get(&connection_id)
            .ok_or_else(|| anyhow!("Looper ACP connection is not active"))?
            .clone();
        drop(state);

        let prompt_id = format!("mobile-prompt-{}", Uuid::new_v4());
        send_json_rpc_notification(
            &sender,
            session_update_notification(
                &session_key,
                USER_MESSAGE_CHUNK_UPDATE,
                prompt,
                Some(json!({
                    "source": MOBILE_PROMPT_SOURCE,
                    "promptId": prompt_id,
                })),
            ),
        )?;
        Ok(DevinAcpDeliveredPrompt {
            prompt_id,
            session_id: session_key,
            public_thread_id,
        })
    }

    fn handle_request(&self, connection_id: &str, request: JsonRpcRequest) -> Vec<String> {
        match request.method.as_str() {
            INITIALIZE_METHOD => {
                vec![json_rpc_result(request.id, initialize_response())]
            }
            SESSION_NEW_METHOD => {
                vec![self.handle_new_session(connection_id, request.id, request.params)]
            }
            SESSION_PROMPT_METHOD => self.handle_prompt(connection_id, request.id, request.params),
            SESSION_CANCEL_METHOD => {
                self.handle_cancel(request.params);
                Vec::new()
            }
            method => request
                .id
                .map(|id| {
                    json_rpc_error(Some(id), -32601, &format!("unsupported method: {method}"))
                })
                .into_iter()
                .collect(),
        }
    }

    fn handle_new_session(
        &self,
        connection_id: &str,
        request_id: Option<Value>,
        params: Value,
    ) -> String {
        let session_id = format!("{LOOPER_SESSION_PREFIX}{}", Uuid::new_v4());
        let now = now_millis();
        let session = DevinAcpRuntimeSession {
            public_thread_id: public_thread_id_for_acp_session(&session_id),
            session_id: session_id.clone(),
            connection_id: connection_id.to_owned(),
            cwd: params.get("cwd").and_then(Value::as_str).map(str::to_owned),
            created_at_ms: now,
            updated_at_ms: now,
            latest_user_prompt: None,
            latest_assistant_message: None,
            cancelled: false,
        };
        let mut state = self.state.lock().expect("devin acp runtime state");
        state.sessions.insert(session_id.clone(), session);
        json_rpc_result(request_id, json!({ "sessionId": session_id }))
    }

    fn handle_prompt(
        &self,
        connection_id: &str,
        request_id: Option<Value>,
        params: Value,
    ) -> Vec<String> {
        let Some(session_id) = params.get("sessionId").and_then(Value::as_str) else {
            return request_id
                .map(|id| json_rpc_error(Some(id), -32602, "sessionId is required"))
                .into_iter()
                .collect();
        };
        let prompt_text = prompt_text(&params).unwrap_or_default();
        let now = now_millis();
        {
            let mut state = self.state.lock().expect("devin acp runtime state");
            let session = state
                .sessions
                .entry(session_id.to_owned())
                .or_insert_with(|| DevinAcpRuntimeSession {
                    public_thread_id: public_thread_id_for_acp_session(session_id),
                    session_id: session_id.to_owned(),
                    connection_id: connection_id.to_owned(),
                    cwd: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                    latest_user_prompt: None,
                    latest_assistant_message: None,
                    cancelled: false,
                });
            session.latest_user_prompt = (!prompt_text.is_empty()).then_some(prompt_text.clone());
            session.updated_at_ms = now;
            session.cancelled = false;
        }

        let assistant_text = TEXT_UPDATE_DETAIL;
        {
            let mut state = self.state.lock().expect("devin acp runtime state");
            if let Some(session) = state.sessions.get_mut(session_id) {
                session.latest_assistant_message = Some(assistant_text.to_owned());
            }
        }
        vec![
            json_rpc_notification(session_update_notification(
                session_id,
                AGENT_MESSAGE_CHUNK_UPDATE,
                assistant_text,
                None,
            )),
            json_rpc_result(
                request_id,
                json!({
                    "stopReason": END_TURN_STOP_REASON,
                    "userMessageId": params.get("messageId").cloned().unwrap_or(Value::Null),
                }),
            ),
        ]
    }

    fn handle_cancel(&self, params: Value) {
        let Some(session_id) = params.get("sessionId").and_then(Value::as_str) else {
            return;
        };
        let mut state = self.state.lock().expect("devin acp runtime state");
        if let Some(session) = state.sessions.get_mut(session_id) {
            session.cancelled = true;
            session.updated_at_ms = now_millis();
        }
    }
}

pub fn websocket_url_for_base_url(base_url: &str) -> String {
    let base_url = base_url.trim_end_matches('/');
    let websocket_base_url = if let Some(rest) = base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base_url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        format!("ws://{base_url}")
    };
    format!("{websocket_base_url}{LOOPER_ACP_ROUTE}")
}

pub fn public_thread_id_for_acp_session(session_id: &str) -> String {
    let normalized_session_id = session_id
        .strip_prefix("acp/")
        .unwrap_or(session_id)
        .replace('/', ":");
    format!("devin:{normalized_session_id}")
}

pub fn acp_session_id_for_public_thread_id(thread_id: &str) -> Option<String> {
    let remainder = thread_id.strip_prefix("devin:looper:")?;
    let normalized = remainder.replace(':', "/");
    Some(format!("{LOOPER_SESSION_PREFIX}{normalized}"))
}

fn runtime_session_key(session_id: &str) -> String {
    acp_session_id_for_public_thread_id(session_id).unwrap_or_else(|| session_id.to_owned())
}

fn initialize_response() -> Value {
    json!({
        "protocolVersion": ACP_PROTOCOL_VERSION,
        "agentCapabilities": {
            "loadSession": false,
            "promptCapabilities": {
                "audio": false,
                "embeddedContext": false,
                "image": false
            },
            "mcpCapabilities": {
                "acp": false,
                "http": false,
                "sse": false
            },
            "sessionCapabilities": {}
        }
    })
}

fn prompt_text(params: &Value) -> Option<String> {
    params
        .get("prompt")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|block| {
                    let is_text =
                        block.get("type").and_then(Value::as_str) == Some(TEXT_CONTENT_TYPE);
                    is_text
                        .then(|| block.get("text").and_then(Value::as_str))
                        .flatten()
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|value| !value.trim().is_empty())
}

fn session_update_notification(
    session_id: &str,
    update_kind: &str,
    text: &str,
    meta: Option<Value>,
) -> Value {
    let mut update = json!({
        "sessionUpdate": update_kind,
        "content": {
            "type": TEXT_CONTENT_TYPE,
            "text": text
        }
    });
    if let Some(meta) = meta {
        update["_meta"] = meta;
    }
    json!({
        "method": SESSION_UPDATE_METHOD,
        "params": {
            "sessionId": session_id,
            "update": update
        }
    })
}

fn send_json_rpc_notification(sender: &OutboundMessageSender, value: Value) -> Result<()> {
    sender
        .send(json_rpc_notification(value))
        .map_err(|_| anyhow!("Looper ACP connection is closed"))
}

fn json_rpc_notification(mut value: Value) -> String {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "jsonrpc".to_owned(),
            Value::String(JSON_RPC_VERSION.to_owned()),
        );
    }
    value.to_string()
}

fn json_rpc_result(id: Option<Value>, result: Value) -> String {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": id.unwrap_or(Value::Null),
        "result": result
    })
    .to_string()
}

fn json_rpc_error(id: Option<Value>, code: i64, message: &str) -> String {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message
        }
    })
    .to_string()
}

fn now_millis() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();

    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_acp_sessions_to_public_devin_threads() {
        assert_eq!(
            public_thread_id_for_acp_session("acp/looper/123"),
            "devin:looper:123"
        );
        assert_eq!(
            acp_session_id_for_public_thread_id("devin:looper:123").as_deref(),
            Some("acp/looper/123")
        );
    }

    #[test]
    fn handles_basic_acp_session_lifecycle() {
        let runtime = DevinAcpRuntime::default();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let connection_id = runtime.register_connection(sender);

        let init = runtime.handle_text_message(
            &connection_id,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}"#,
        );
        assert_eq!(init.len(), 1);
        assert!(init[0].contains("\"protocolVersion\":1"));

        let created = runtime.handle_text_message(
            &connection_id,
            r#"{"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":"/tmp","mcpServers":[]}}"#,
        );
        let created: Value = serde_json::from_str(&created[0]).expect("created response");
        let session_id = created["result"]["sessionId"].as_str().expect("session id");
        assert!(session_id.starts_with(LOOPER_SESSION_PREFIX));

        let prompt = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{{"sessionId":"{session_id}","prompt":[{{"type":"text","text":"hello"}}]}}}}"#
        );
        let responses = runtime.handle_text_message(&connection_id, &prompt);
        assert_eq!(responses.len(), 2);
        assert!(responses[0].contains(SESSION_UPDATE_METHOD));
        assert!(responses[1].contains(END_TURN_STOP_REASON));

        let delivered = runtime
            .deliver_mobile_prompt(session_id, "from phone")
            .expect("deliver prompt");
        assert_eq!(delivered.session_id, session_id);
        let outbound = receiver.try_recv().expect("mobile prompt update");
        assert!(outbound.contains(USER_MESSAGE_CHUNK_UPDATE));
        assert!(outbound.contains("from phone"));
    }
}

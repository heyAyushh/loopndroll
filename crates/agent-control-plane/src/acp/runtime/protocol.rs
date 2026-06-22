use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};

use super::model::LooperAcpRuntimeSession;
use super::{
    ACP_PROTOCOL_VERSION, JSON_RPC_VERSION, LOOPER_ACP_AGENT_ID, LOOPER_ACP_SUPPORTED_METHODS,
    MOBILE_PROMPT_SOURCE, OutboundMessageSender, SESSION_PROMPT_METHOD, SESSION_UPDATE_METHOD,
    TEXT_CONTENT_TYPE, USER_MESSAGE_CHUNK_UPDATE,
};

#[derive(Debug, Deserialize)]
pub(super) struct JsonRpcRequest {
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

pub fn supported_acp_methods() -> Vec<String> {
    LOOPER_ACP_SUPPORTED_METHODS
        .iter()
        .map(|method| (*method).to_owned())
        .collect()
}

pub(super) fn prompt_id_for_source(source: &str) -> String {
    let prefix = match source {
        MOBILE_PROMPT_SOURCE => "mobile-prompt",
        _ => "control-prompt",
    };
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

pub(super) fn initialize_response() -> Value {
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

pub(super) fn prompt_text(params: &Value) -> Option<String> {
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

pub(super) fn session_update_notification(
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

fn send_json_rpc_request(sender: &OutboundMessageSender, value: Value) -> Result<()> {
    sender
        .send(json_rpc_request(value))
        .map_err(|_| anyhow!("Looper ACP connection is closed"))
}

pub(super) fn deliver_prompt_to_connection(
    sender: &OutboundMessageSender,
    session: &LooperAcpRuntimeSession,
    prompt: &str,
    prompt_id: &str,
    source: &str,
) -> Result<()> {
    if session.agent_id == LOOPER_ACP_AGENT_ID {
        return send_json_rpc_notification(
            sender,
            session_update_notification(
                &session.session_id,
                USER_MESSAGE_CHUNK_UPDATE,
                prompt,
                Some(json!({
                    "source": source,
                    "promptId": prompt_id,
                })),
            ),
        );
    }

    send_json_rpc_request(
        sender,
        proxied_session_prompt_request(&session.session_id, prompt, prompt_id, source),
    )
}

fn proxied_session_prompt_request(
    session_id: &str,
    prompt: &str,
    prompt_id: &str,
    source: &str,
) -> Value {
    json!({
        "id": prompt_id,
        "method": SESSION_PROMPT_METHOD,
        "params": {
            "sessionId": session_id,
            "prompt": [
                {
                    "type": TEXT_CONTENT_TYPE,
                    "text": prompt
                }
            ],
            "_meta": {
                "source": source,
                "promptId": prompt_id
            }
        }
    })
}

fn json_rpc_request(mut value: Value) -> String {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "jsonrpc".to_owned(),
            Value::String(JSON_RPC_VERSION.to_owned()),
        );
    }
    value.to_string()
}

pub(super) fn json_rpc_notification(mut value: Value) -> String {
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "jsonrpc".to_owned(),
            Value::String(JSON_RPC_VERSION.to_owned()),
        );
    }
    value.to_string()
}

pub(super) fn json_rpc_result(id: Option<Value>, result: Value) -> String {
    json!({
        "jsonrpc": JSON_RPC_VERSION,
        "id": id.unwrap_or(Value::Null),
        "result": result
    })
    .to_string()
}

pub(super) fn json_rpc_error(id: Option<Value>, code: i64, message: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_text_joins_text_blocks_only() {
        let params = json!({
            "prompt": [
                { "type": "text", "text": "first" },
                { "type": "image", "url": "ignored" },
                { "type": "text", "text": "second" }
            ]
        });

        assert_eq!(prompt_text(&params).as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn looper_prompt_delivers_as_user_session_update_notification() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let session = LooperAcpRuntimeSession {
            session_id: "acp/looper/1".to_owned(),
            public_thread_id: "zed:codex:looper:1".to_owned(),
            agent_id: LOOPER_ACP_AGENT_ID.to_owned(),
            connection_id: "connection".to_owned(),
            cwd: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            latest_user_prompt: None,
            latest_assistant_message: None,
            cancelled: false,
        };

        deliver_prompt_to_connection(&sender, &session, "hello", "prompt-1", "test")
            .expect("deliver prompt");

        let message = receiver.try_recv().expect("notification");
        assert!(message.contains(SESSION_UPDATE_METHOD));
        assert!(message.contains(USER_MESSAGE_CHUNK_UPDATE));
        assert!(message.contains("hello"));
    }
}

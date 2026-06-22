use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{
    SESSION_CANCEL_METHOD, SESSION_NEW_METHOD, SESSION_PROMPT_METHOD, SESSION_UPDATE_METHOD,
    TEXT_CONTENT_TYPE,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct PendingNewSession {
    pub(super) cwd: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ObservedSessionEvent {
    pub(super) agent_id: String,
    pub(super) session_id: String,
    pub(super) cwd: Option<String>,
    pub(super) latest_user_prompt: Option<String>,
    pub(super) latest_assistant_message: Option<String>,
    pub(super) cancelled: bool,
}

pub(super) async fn post_observed_sessions(
    client_id: String,
    mut receiver: mpsc::UnboundedReceiver<ObservedSessionEvent>,
) {
    let client = reqwest::Client::new();
    while let Some(event) = receiver.recv().await {
        let body = json!({
            "agentId": event.agent_id,
            "sessionId": event.session_id,
            "cwd": event.cwd,
            "latestUserPrompt": event.latest_user_prompt,
            "latestAssistantMessage": event.latest_assistant_message,
            "cancelled": event.cancelled,
        });
        let path = format!("/desktop/acp-client-hosts/{client_id}/sessions/observe");
        if let Err(error) = client
            .post(crate::cli::transport::url(&path))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
        {
            eprintln!("looper ACP observation failed: {error}");
        }
    }
}

pub(super) fn observe_zed_request(
    agent_id: &str,
    line: &str,
    pending_new_sessions: &Arc<Mutex<BTreeMap<String, PendingNewSession>>>,
    observer_sender: &mpsc::UnboundedSender<ObservedSessionEvent>,
) {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return;
    };
    let params = message.get("params").unwrap_or(&Value::Null);
    match method {
        SESSION_NEW_METHOD => {
            if let Some(id) = request_id_key(message.get("id")) {
                let mut pending = pending_new_sessions
                    .lock()
                    // SAFE-EXPECT: poisoned pending-session state means session-new observation is inconsistent.
                    .expect("pending Zed ACP session map");
                pending.insert(
                    id,
                    PendingNewSession {
                        cwd: params.get("cwd").and_then(Value::as_str).map(str::to_owned),
                    },
                );
            }
        }
        SESSION_PROMPT_METHOD => {
            if let Some(session_id) = params.get("sessionId").and_then(Value::as_str) {
                let _ = observer_sender.send(ObservedSessionEvent {
                    agent_id: agent_id.to_owned(),
                    session_id: session_id.to_owned(),
                    cwd: None,
                    latest_user_prompt: prompt_text(params),
                    latest_assistant_message: None,
                    cancelled: false,
                });
            }
        }
        SESSION_CANCEL_METHOD => {
            if let Some(session_id) = params.get("sessionId").and_then(Value::as_str) {
                let _ = observer_sender.send(ObservedSessionEvent {
                    agent_id: agent_id.to_owned(),
                    session_id: session_id.to_owned(),
                    cwd: None,
                    latest_user_prompt: None,
                    latest_assistant_message: None,
                    cancelled: true,
                });
            }
        }
        _ => {}
    }
}

pub(super) fn observe_agent_output(
    agent_id: &str,
    line: &str,
    pending_new_sessions: &Arc<Mutex<BTreeMap<String, PendingNewSession>>>,
    observer_sender: &mpsc::UnboundedSender<ObservedSessionEvent>,
) {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if message.get("method").and_then(Value::as_str) == Some(SESSION_UPDATE_METHOD) {
        observe_session_update(agent_id, &message, observer_sender);
        return;
    }
    let Some(id) = request_id_key(message.get("id")) else {
        return;
    };
    let Some(session_id) = message
        .get("result")
        .and_then(|result| result.get("sessionId"))
        .and_then(Value::as_str)
    else {
        return;
    };
    let pending = pending_new_sessions
        .lock()
        // SAFE-EXPECT: poisoned pending-session state means session-new observation is inconsistent.
        .expect("pending Zed ACP session map")
        .remove(&id)
        .unwrap_or_default();
    let _ = observer_sender.send(ObservedSessionEvent {
        agent_id: agent_id.to_owned(),
        session_id: session_id.to_owned(),
        cwd: pending.cwd,
        latest_user_prompt: None,
        latest_assistant_message: None,
        cancelled: false,
    });
}

fn observe_session_update(
    agent_id: &str,
    message: &Value,
    observer_sender: &mpsc::UnboundedSender<ObservedSessionEvent>,
) {
    let params = message.get("params").unwrap_or(&Value::Null);
    let Some(session_id) = params.get("sessionId").and_then(Value::as_str) else {
        return;
    };
    let latest_assistant_message = params
        .get("update")
        .and_then(|update| update.get("content"))
        .and_then(|content| {
            (content.get("type").and_then(Value::as_str) == Some(TEXT_CONTENT_TYPE))
                .then(|| content.get("text").and_then(Value::as_str))
                .flatten()
        })
        .map(str::to_owned);
    let _ = observer_sender.send(ObservedSessionEvent {
        agent_id: agent_id.to_owned(),
        session_id: session_id.to_owned(),
        cwd: None,
        latest_user_prompt: None,
        latest_assistant_message,
        cancelled: false,
    });
}

fn request_id_key(value: Option<&Value>) -> Option<String> {
    let value = value?;
    (!value.is_null()).then(|| value.to_string())
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

use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ids::runtime_session_key;
use super::protocol::{
    JsonRpcRequest, deliver_prompt_to_connection, initialize_response, json_rpc_error,
    json_rpc_notification, json_rpc_result, prompt_id_for_source, prompt_text,
    session_update_notification,
};
use super::{
    AGENT_MESSAGE_CHUNK_UPDATE, CONTROL_PROMPT_SOURCE, END_TURN_STOP_REASON,
    LOCAL_CONTROL_CONNECTION_ID, LOOPER_ACP_AGENT_ID, LOOPER_SESSION_PREFIX,
    LooperAcpControlCancel, LooperAcpControlError, LooperAcpControlPrompt,
    LooperAcpDeliveredPrompt, LooperAcpRuntime, LooperAcpRuntimeSession, MOBILE_PROMPT_SOURCE,
    SESSION_CANCEL_METHOD, SESSION_NEW_METHOD, SESSION_PROMPT_METHOD, TEXT_UPDATE_DETAIL,
    agent_id_for_connection, new_runtime_session, now_millis,
};

impl LooperAcpRuntime {
    pub fn handle_text_message(&self, connection_id: &str, text: &str) -> Vec<String> {
        let parsed = serde_json::from_str::<JsonRpcRequest>(text);
        match parsed {
            Ok(request) => self.handle_request(connection_id, request),
            Err(error) => vec![json_rpc_error(None, -32700, &error.to_string())],
        }
    }

    pub fn create_control_session(&self, cwd: Option<String>) -> LooperAcpRuntimeSession {
        let session_id = format!("{LOOPER_SESSION_PREFIX}{}", Uuid::new_v4());
        let now = now_millis();
        let session = new_runtime_session(
            self.client_id,
            LOOPER_ACP_AGENT_ID,
            &session_id,
            LOCAL_CONTROL_CONNECTION_ID,
            cwd,
            now,
        );
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        state.sessions.insert(session_id, session.clone());
        session
    }

    pub fn prompt_control_session(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<LooperAcpControlPrompt, LooperAcpControlError> {
        self.prompt_session(session_id, prompt, CONTROL_PROMPT_SOURCE)
    }

    pub fn cancel_control_session(
        &self,
        session_id: &str,
    ) -> Result<LooperAcpControlCancel, LooperAcpControlError> {
        let session_key = runtime_session_key(self.client_id, session_id);
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        let session = state
            .sessions
            .get_mut(&session_key)
            .ok_or(LooperAcpControlError::SessionNotFound)?;
        session.cancelled = true;
        session.updated_at_ms = now_millis();
        let session = session.clone();
        Ok(LooperAcpControlCancel {
            session_id: session.session_id.clone(),
            public_thread_id: session.public_thread_id.clone(),
            session,
        })
    }

    pub fn deliver_mobile_prompt(
        &self,
        session_id: &str,
        prompt: &str,
    ) -> Result<LooperAcpDeliveredPrompt> {
        let delivered = self
            .prompt_session(session_id, prompt, MOBILE_PROMPT_SOURCE)
            .map_err(|error| anyhow!(error.to_string()))?;
        Ok(LooperAcpDeliveredPrompt {
            prompt_id: delivered.prompt_id,
            session_id: delivered.session_id,
            public_thread_id: delivered.public_thread_id,
        })
    }

    fn prompt_session(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
    ) -> Result<LooperAcpControlPrompt, LooperAcpControlError> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(LooperAcpControlError::PromptRequired);
        }

        let session_key = runtime_session_key(self.client_id, session_id);
        let prompt_id = prompt_id_for_source(source);
        let (sender, session) = {
            // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
            let mut state = self.state.lock().expect("looper acp runtime state");
            let (connection_id, agent_id) = {
                let session = state
                    .sessions
                    .get(&session_key)
                    .ok_or(LooperAcpControlError::SessionNotFound)?;
                (session.connection_id.clone(), session.agent_id.clone())
            };
            let sender = state
                .connections
                .get(&connection_id)
                .map(|connection| connection.sender.clone());
            if agent_id != LOOPER_ACP_AGENT_ID && sender.is_none() {
                return Err(LooperAcpControlError::DeliveryUnavailable);
            }
            let session = state
                .sessions
                .get_mut(&session_key)
                .ok_or(LooperAcpControlError::SessionNotFound)?;
            session.latest_user_prompt = Some(prompt.to_owned());
            session.latest_assistant_message = Some(TEXT_UPDATE_DETAIL.to_owned());
            session.updated_at_ms = now_millis();
            session.cancelled = false;
            let session = session.clone();
            (sender, session)
        };

        let delivered_to_connection = sender.as_ref().is_some_and(|sender| {
            deliver_prompt_to_connection(sender, &session, prompt, &prompt_id, source).is_ok()
        });

        Ok(LooperAcpControlPrompt {
            prompt_id,
            session_id: session.session_id.clone(),
            public_thread_id: session.public_thread_id.clone(),
            delivered_to_connection,
            session,
        })
    }

    fn handle_request(&self, connection_id: &str, request: JsonRpcRequest) -> Vec<String> {
        match request.method.as_str() {
            super::INITIALIZE_METHOD => vec![json_rpc_result(request.id, initialize_response())],
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
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        let agent_id = agent_id_for_connection(&state, connection_id);
        let session = new_runtime_session(
            self.client_id,
            &agent_id,
            &session_id,
            connection_id,
            params.get("cwd").and_then(Value::as_str).map(str::to_owned),
            now,
        );
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
            // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
            let mut state = self.state.lock().expect("looper acp runtime state");
            let agent_id = agent_id_for_connection(&state, connection_id);
            let session = state
                .sessions
                .entry(session_id.to_owned())
                .or_insert_with(|| {
                    new_runtime_session(
                        self.client_id,
                        &agent_id,
                        session_id,
                        connection_id,
                        None,
                        now,
                    )
                });
            session.latest_user_prompt = (!prompt_text.is_empty()).then_some(prompt_text.clone());
            session.updated_at_ms = now;
            session.cancelled = false;
        }

        let assistant_text = TEXT_UPDATE_DETAIL;
        {
            // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
            let mut state = self.state.lock().expect("looper acp runtime state");
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
        // SAFE-EXPECT: poisoned runtime state means a prior panic may have left sessions inconsistent.
        let mut state = self.state.lock().expect("looper acp runtime state");
        if let Some(session) = state.sessions.get_mut(session_id) {
            session.cancelled = true;
            session.updated_at_ms = now_millis();
        }
    }
}

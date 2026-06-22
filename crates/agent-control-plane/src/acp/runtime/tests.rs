use super::*;
use crate::acp::runtime::ids::runtime_session_key;
use serde_json::Value;

#[test]
fn maps_acp_sessions_to_public_threads_by_client() {
    assert_eq!(
        public_thread_id_for_client_acp_session("devin", "acp/looper/123"),
        "devin:looper:123"
    );
    assert_eq!(
        public_thread_id_for_client_acp_session("zed", "acp/looper/123"),
        "zed:codex:looper:123"
    );
    assert_eq!(
        acp_session_id_for_client_public_thread_id("zed", "zed:codex:looper:123").as_deref(),
        Some("acp/looper/123")
    );
    assert_eq!(
        acp_session_id_for_client_public_thread_id("zed", "zed:looper:123").as_deref(),
        Some("acp/looper/123")
    );
    assert_eq!(
        runtime_session_key("zed", "zed:codex:looper:123"),
        "acp/looper/123"
    );
    assert_eq!(
        acp_session_id_for_client_public_thread_id("devin", "zed:looper:123"),
        None
    );
}

#[test]
fn maps_zed_codex_acp_target_to_public_codex_path() {
    assert_eq!(
        public_agent_id_for_client_agent_id("zed", "codex-acp"),
        "codex"
    );
    assert_eq!(
        public_agent_id_for_client_agent_id("devin", "codex-acp"),
        "codex-acp"
    );
    assert_eq!(
        public_agent_id_for_client_agent_id("zed", "codex"),
        "codex-direct"
    );
    assert_eq!(
        public_thread_id_for_client_agent_acp_session("zed", "codex-acp", "session-1"),
        "zed:codex:session-1"
    );
    assert_eq!(
        public_thread_id_for_client_agent_acp_session("zed", "codex", "session-1"),
        "zed:codex-direct:session-1"
    );
    assert_eq!(
        acp_agent_and_session_id_for_client_public_thread_id("zed", "zed:codex:session-1"),
        Some(("codex-acp".to_owned(), "session-1".to_owned()))
    );
    assert_eq!(
        acp_agent_and_session_id_for_client_public_thread_id("zed", "zed:codex:looper:session-1"),
        Some(("codex-acp".to_owned(), "acp/looper/session-1".to_owned()))
    );
    assert_eq!(
        acp_agent_and_session_id_for_client_public_thread_id("zed", "zed:codex-direct:session-1"),
        Some(("codex".to_owned(), "session-1".to_owned()))
    );
}

#[test]
fn handles_basic_acp_session_lifecycle() {
    let runtime = LooperAcpRuntime::new("zed");
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
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let created: Value = serde_json::from_str(&created[0]).expect("created response");
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let session_id = created["result"]["sessionId"].as_str().expect("session id");
    assert!(session_id.starts_with(LOOPER_SESSION_PREFIX));
    assert_eq!(
        public_thread_id_for_client_acp_session("zed", session_id),
        runtime.status().sessions[0].public_thread_id
    );

    let prompt = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{{"sessionId":"{session_id}","prompt":[{{"type":"text","text":"hello"}}]}}}}"#
    );
    let responses = runtime.handle_text_message(&connection_id, &prompt);
    assert_eq!(responses.len(), 2);
    assert!(responses[0].contains(SESSION_UPDATE_METHOD));
    assert!(responses[1].contains(END_TURN_STOP_REASON));

    let delivered = runtime
        .deliver_mobile_prompt(session_id, "from phone")
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("deliver prompt");
    assert_eq!(delivered.session_id, session_id);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let outbound = receiver.try_recv().expect("mobile prompt update");
    assert!(outbound.contains(USER_MESSAGE_CHUNK_UPDATE));
    assert!(outbound.contains("from phone"));
}

#[test]
fn controls_local_runtime_sessions_without_transport_connection() {
    let runtime = LooperAcpRuntime::new("zed");
    let session = runtime.create_control_session(Some("/tmp/looper".to_owned()));

    assert!(session.session_id.starts_with(LOOPER_SESSION_PREFIX));
    assert_eq!(
        session.public_thread_id,
        "zed:codex:looper:".to_owned() + &session.session_id["acp/looper/".len()..]
    );
    assert_eq!(session.cwd.as_deref(), Some("/tmp/looper"));

    let prompt = runtime
        .prompt_control_session(&session.public_thread_id, "run it")
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("prompt session");
    assert_eq!(prompt.session_id, session.session_id);
    assert!(!prompt.delivered_to_connection);
    assert_eq!(prompt.session.latest_user_prompt.as_deref(), Some("run it"));

    let cancel = runtime
        .cancel_control_session(&session.public_thread_id)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("cancel session");
    assert!(cancel.session.cancelled);
}

#[test]
fn rejects_proxy_session_prompt_without_live_transport() {
    let runtime = LooperAcpRuntime::new("zed");
    let observed = runtime.observe_session(LooperAcpObservedSession {
        agent_id: ZED_CODEX_ACP_AGENT_ID.to_owned(),
        session_id: "codex-session-1".to_owned(),
        connection_id: Some("stale-zed-connection".to_owned()),
        cwd: Some("/tmp/zed-project".to_owned()),
        latest_user_prompt: None,
        latest_assistant_message: None,
        cancelled: false,
    });

    let error = runtime
        .prompt_control_session(&observed.public_thread_id, "continue")
        .expect_err("stale observed proxy sessions are not promptable");

    assert_eq!(error, LooperAcpControlError::DeliveryUnavailable);

    let retained_session = runtime
        .status()
        .sessions
        .into_iter()
        .find(|session| session.session_id == observed.session_id)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("stale observed session remains visible");
    assert_eq!(
        retained_session.latest_user_prompt,
        observed.latest_user_prompt
    );
    assert_eq!(
        retained_session.latest_assistant_message,
        observed.latest_assistant_message
    );
    assert_eq!(retained_session.updated_at_ms, observed.updated_at_ms);
    assert_eq!(retained_session.cancelled, observed.cancelled);
}

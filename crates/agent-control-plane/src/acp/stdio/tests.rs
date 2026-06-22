use super::*;

#[tokio::test]
async fn rejects_unknown_stdio_client() {
    let error = run_stdio_agent("missing", &[])
        .await
        .expect_err("unknown client");
    assert!(error.to_string().contains("unsupported ACP stdio client"));
}

#[test]
fn parses_proxy_target_after_separator() {
    let args = vec![
        "codex-acp".to_owned(),
        "--".to_owned(),
        "/tmp/codex-acp".to_owned(),
        "-c".to_owned(),
        "model=\"gpt-5\"".to_owned(),
    ];

    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let target = parse_proxy_target(&args).expect("target");

    assert_eq!(target.agent_id, "codex-acp");
    assert_eq!(target.command, "/tmp/codex-acp");
    assert_eq!(target.args, vec!["-c", "model=\"gpt-5\""]);
}

#[test]
fn observes_session_new_response_from_pending_request() {
    let pending = Arc::new(Mutex::new(BTreeMap::new()));
    let (sender, mut receiver) = mpsc::unbounded_channel();
    observe_zed_request(
        "codex-acp",
        r#"{"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":"/tmp/project"}}"#,
        &pending,
        &sender,
    );
    observe_agent_output(
        "codex-acp",
        r#"{"jsonrpc":"2.0","id":1,"result":{"sessionId":"codex-session-1"}}"#,
        &pending,
        &sender,
    );

    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let event = receiver.try_recv().expect("observed session");
    assert_eq!(event.agent_id, "codex-acp");
    assert_eq!(event.session_id, "codex-session-1");
    assert_eq!(event.cwd.as_deref(), Some("/tmp/project"));
}

use super::*;

#[tokio::test]
async fn mobile_snapshot_includes_classification_metadata_for_all_surfaces() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    fixture.write_grok_session("grok-session-1", "/tmp/project", "Ship Grok hooks");
    fixture.write_claude_session(
        "claude-session-1",
        "/tmp/claude-project",
        "Build native Claude support",
        "Claude session is visible.",
    );
    fixture.write_zed_settings();
    let router = build_router(fixture.control_plane_with_running_zed());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;

    let codex_session = mobile_surface_session(&snapshot, "codex", "thread-main");
    assert_eq!(codex_session["metadata"]["assistantKind"], "codex");
    assert_eq!(
        codex_session["metadata"]["supportsSubagents"],
        serde_json::json!(true)
    );
    assert_eq!(codex_session["metadata"]["spawn"]["launchKind"], "main");

    let codex_child_session = mobile_surface_session(&snapshot, "codex", "thread-child");
    assert_eq!(
        codex_child_session["metadata"]["spawn"]["parentThreadId"],
        "thread-main"
    );
    assert_eq!(
        codex_child_session["metadata"]["spawn"]["launchKind"],
        "subagent"
    );

    let devin_session = mobile_surface_session(&snapshot, "devin", "devin:devin-cli:brindle-cadet");
    assert_eq!(devin_session["metadata"]["assistantKind"], "devin-desktop");
    assert_eq!(devin_session["metadata"]["originator"], "Devin - Next");
    assert_eq!(devin_session["metadata"]["sourceDisplayName"], "Devin");
    assert_eq!(
        devin_session["metadata"]["supportsSubagents"],
        serde_json::json!(false)
    );

    let grok_session = mobile_surface_session(&snapshot, "grok-build", "grok-session-1");
    assert_eq!(grok_session["metadata"]["assistantKind"], "grok-build");
    assert_eq!(grok_session["metadata"]["originator"], "Grok Build");
    assert_eq!(grok_session["metadata"]["sourceDisplayName"], "Grok Build");

    let claude_session =
        mobile_surface_session(&snapshot, "claude-code", "claude:claude-session-1");
    assert_eq!(claude_session["metadata"]["assistantKind"], "claude-code");
    assert_eq!(claude_session["metadata"]["originator"], "Claude Code");
    assert_eq!(
        claude_session["metadata"]["sourceDisplayName"],
        "Claude Code"
    );

    assert!(
        snapshot["surfaceSessions"]["zed"]
            .as_array()
            .expect("zed sessions")
            .is_empty()
    );
    let desktop_snapshot = request_json(&router, "/desktop/snapshot").await;
    assert!(
        desktop_snapshot["acp_targets"]
            .as_array()
            .expect("acp targets")
            .iter()
            .any(|target| target["id"] == "zed:looper"
                && target["client"] == "zed"
                && target["ready"] == serde_json::json!(true)
                && target["status"] == "ready")
    );
}

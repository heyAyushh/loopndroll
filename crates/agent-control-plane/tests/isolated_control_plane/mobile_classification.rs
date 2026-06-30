use super::*;

#[tokio::test]
async fn mobile_snapshot_hides_codex_subagents_from_home_surface() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());
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
    assert!(
        !mobile_surface_has_session(&snapshot, "codex", "thread-child"),
        "subagent worker threads should not appear as top-level Codex home sessions"
    );
}

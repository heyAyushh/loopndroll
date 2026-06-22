use super::*;

#[tokio::test]

async fn devin_acp_attach_route_is_not_supported() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_settings();
    let router = build_router(fixture.control_plane());
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let loopback = Some("127.0.0.1:49153".parse().expect("loopback socket"));

    let attach_response = request_with_body_options(
        &router,
        Method::POST,
        "/desktop/devin/acp-bridge/attach",
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        serde_json::to_vec(&serde_json::json!({ "agentId": "codex" })).expect("json body"),
        &[(axum::http::header::CONTENT_TYPE, "application/json")],
        loopback,
    )
    .await;

    assert_eq!(attach_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]

async fn devin_acp_control_routes_create_prompt_and_cancel_looper_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_settings();
    let router = build_router(fixture.control_plane());
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let loopback = Some("127.0.0.1:49153".parse().expect("loopback socket"));

    let installed = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/devin/install",
        serde_json::Value::Null,
        &[],
        loopback,
    )
    .await;
    assert_eq!(installed["install"]["installed_agent_id"], "looper");

    let created = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/devin/acp-bridge/sessions",
        serde_json::json!({ "cwd": "/tmp/looper" }),
        &[],
        loopback,
    )
    .await;
    let thread_id = created["session"]["public_thread_id"]
        .as_str()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("public thread id");
    assert!(thread_id.starts_with("devin:looper:"));
    assert_eq!(created["session"]["cwd"], "/tmp/looper");
    assert_eq!(created["runtime"]["session_count"], 1);

    let prompted = request_json_body_with_options(
        &router,
        Method::POST,
        &format!("/desktop/acp-client-hosts/devin/sessions/{thread_id}/prompt"),
        serde_json::json!({ "prompt": "keep going" }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(prompted["session"]["public_thread_id"], thread_id);
    assert_eq!(prompted["session"]["latest_user_prompt"], "keep going");
    assert_eq!(prompted["session"]["cancelled"], false);
    assert_eq!(prompted["delivered_to_connection"], false);
    assert!(
        prompted["prompt_id"]
            .as_str()
            // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
            .expect("prompt id")
            .starts_with("control-prompt-")
    );

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/snapshot?profile=menu",
        &[],
        loopback,
    )
    .await;
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    assert!(snapshot["threads"].as_array().expect("threads").iter().any(
        |thread| thread["thread_id"] == thread_id
            && thread["runtime_status"] == "active"
            && thread["capabilities"]["assistant_kind"] == "devin-desktop"
    ));

    let cancelled = request_json_with_options(
        &router,
        Method::POST,
        &format!("/desktop/devin/acp-bridge/sessions/{thread_id}/cancel"),
        &[],
        loopback,
    )
    .await;
    assert_eq!(cancelled["session"]["public_thread_id"], thread_id);
    assert_eq!(cancelled["session"]["cancelled"], true);

    let missing = request_with_body_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/devin/sessions/devin:looper:missing/prompt",
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        serde_json::to_vec(&serde_json::json!({ "prompt": "hello" })).expect("json body"),
        &[(axum::http::header::CONTENT_TYPE, "application/json")],
        loopback,
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let blank_prompt = request_with_body_options(
        &router,
        Method::POST,
        &format!("/desktop/acp-client-hosts/devin/sessions/{thread_id}/prompt"),
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        serde_json::to_vec(&serde_json::json!({ "prompt": " " })).expect("json body"),
        &[(axum::http::header::CONTENT_TYPE, "application/json")],
        loopback,
    )
    .await;
    assert_eq!(blank_prompt.status(), StatusCode::BAD_REQUEST);
}

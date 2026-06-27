use super::*;

#[tokio::test]

async fn zed_acp_control_routes_install_create_prompt_and_cancel_looper_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let loopback = Some("127.0.0.1:49153".parse().expect("loopback socket"));

    let installed = request_json_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/install",
        &[],
        loopback,
    )
    .await;
    assert_eq!(installed["install"]["installed_agent_id"], "codex");
    assert_eq!(
        installed["host"]["agents"][0]["control_level"],
        "agent-configured"
    );
    assert_eq!(installed["host"]["agents"][0]["id"], "codex");

    let observed = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/sessions/observe",
        serde_json::json!({
            "agentId": "codex-acp",
            "sessionId": "codex-session-1",
            "cwd": "/tmp/project",
            "latestUserPrompt": "Say hi"
        }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(
        observed["session"]["public_thread_id"],
        "zed:codex:codex-session-1"
    );
    assert_eq!(observed["session"]["agent_id"], "codex-acp");
    assert_eq!(observed["session"]["latest_user_prompt"], "Say hi");

    let host_after_observe = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/acp-client-hosts/zed",
        &[],
        loopback,
    )
    .await;
    assert_eq!(
        host_after_observe["host"]["sessions"][0]["thread_id"],
        "zed:codex:codex-session-1"
    );
    assert_eq!(
        host_after_observe["host"]["sessions"][0]["provider_id"],
        "codex"
    );
    assert_eq!(host_after_observe["host"]["sessions"][0]["title"], "Codex");

    let transcript_path = fixture.write_transcript(
        "zed-codex-transcript.jsonl",
        &[serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Not much. Ready."
                    }
                ]
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);

    let desktop_snapshot = request_json(&router, "/desktop/snapshot").await;
    let projected_zed_thread = desktop_snapshot["threads"]
        .as_array()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("desktop threads")
        .iter()
        .find(|thread| thread["thread_id"] == "zed:codex:codex-session-1")
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("projected zed codex thread");
    assert_eq!(projected_zed_thread["title"], "Main task");
    assert_eq!(
        projected_zed_thread["assistant_preview"],
        "Not much. Ready."
    );
    assert_eq!(projected_zed_thread["originator"], "Zed");
    assert_eq!(
        projected_zed_thread["capabilities"]["assistant_kind"],
        "zed"
    );
    let active_before_local_control = desktop_snapshot["active_thread_count"]
        .as_u64()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("active thread count before local control");

    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];
    set_mobile_assistant_surface(
        control_plane.clone(),
        &authorization,
        "zed",
        "zed-mobile-surface",
    )
    .await;
    let mobile_zed_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let zed_session = mobile_snapshot_session(&mobile_zed_snapshot, "zed:codex:codex-session-1");
    assert_eq!(zed_session["title"], "Main task");
    assert_eq!(zed_session["metadata"]["projectPath"], "/tmp/project");
    assert_eq!(zed_session["assistantPreview"], "Not much. Ready.");
    assert_eq!(zed_session["assistantClient"], "zed");
    assert_eq!(zed_session["canSendPrompt"], serde_json::json!(false));

    prime_state_mini_cache(&control_plane);
    let mobile_prompted = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SendSessionPrompt(SendSessionPromptRequest {
            thread_id: "zed:codex:codex-session-1".to_owned(),
            prompt: "Continue from iPhone.".to_owned(),
            assistant_surface: "zed".to_owned(),
            client_mutation_id: "zed-stale-mobile-prompt".to_owned(),
        }),
    )
    .await;
    assert!(!mobile_prompted.accepted);
    assert_eq!(mobile_prompted.error_code, "mode_required");

    let stale_desktop_prompt = request_with_body_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/sessions/zed:codex:codex-session-1/prompt",
        serde_json::to_vec(&serde_json::json!({ "prompt": "Continue from desktop." }))
            // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
            .expect("prompt body"),
        &[(axum::http::header::CONTENT_TYPE, "application/json")],
        loopback,
    )
    .await;
    assert_eq!(stale_desktop_prompt.status(), StatusCode::BAD_GATEWAY);

    let created = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/sessions",
        serde_json::json!({ "cwd": "/tmp/zed-looper" }),
        &[],
        loopback,
    )
    .await;
    let thread_id = created["session"]["public_thread_id"]
        .as_str()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("public thread id");
    assert!(thread_id.starts_with("zed:codex:looper:"));
    assert_eq!(created["session"]["cwd"], "/tmp/zed-looper");
    assert_eq!(created["runtime"]["session_count"], 2);

    let prompted = request_json_body_with_options(
        &router,
        Method::POST,
        &format!("/desktop/acp-client-hosts/zed/sessions/{thread_id}/prompt"),
        serde_json::json!({ "prompt": "control zed" }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(prompted["session"]["public_thread_id"], thread_id);
    assert_eq!(prompted["session"]["latest_user_prompt"], "control zed");
    assert_eq!(prompted["delivered_to_connection"], false);

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/snapshot?profile=menu",
        &[],
        loopback,
    )
    .await;
    assert_eq!(
        snapshot["active_thread_count"].as_u64(),
        Some(active_before_local_control)
    );
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    assert!(snapshot["threads"].as_array().expect("threads").iter().any(
        |thread| thread["thread_id"] == thread_id
            && thread["runtime_status"] == "stopped"
            && thread["capabilities"]["assistant_kind"] == "zed"
    ));

    let cancelled = request_json_with_options(
        &router,
        Method::POST,
        &format!("/desktop/acp-client-hosts/zed/sessions/{thread_id}/cancel"),
        &[],
        loopback,
    )
    .await;
    assert_eq!(cancelled["session"]["public_thread_id"], thread_id);
    assert_eq!(cancelled["session"]["cancelled"], true);
    let snapshot_after_cancel = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/snapshot?profile=menu",
        &[],
        loopback,
    )
    .await;
    assert_eq!(
        snapshot_after_cancel["active_thread_count"].as_u64(),
        Some(active_before_local_control)
    );
}

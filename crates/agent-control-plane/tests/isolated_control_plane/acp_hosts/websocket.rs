use super::*;

#[tokio::test]

async fn devin_acp_legacy_and_generic_websocket_routes_initialize() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("bind websocket test listener");
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let address = listener.local_addr().expect("listener address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("websocket test server");
    });

    for path in ["/acp/devin", "/acp/client-hosts/devin"] {
        let response = initialize_devin_acp_websocket(address, path).await;
        assert_eq!(response["jsonrpc"], "2.0", "{path}");
        assert_eq!(response["id"], 1, "{path}");
        assert_eq!(response["result"]["protocolVersion"], 1, "{path}");
        assert_eq!(
            response["result"]["agentCapabilities"]["loadSession"],
            serde_json::json!(false),
            "{path}"
        );
    }

    server.abort();
}

#[tokio::test]

async fn zed_acp_websocket_registers_codex_agent_and_receives_mobile_prompts() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("bind websocket test listener");
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let address = listener.local_addr().expect("listener address");
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("websocket test server");
    });

    let before_revision = control_plane
        .mobile_snapshot_revision()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("mobile revision before zed session");
    let url = format!("ws://{address}/acp/client-hosts/zed?agentId=codex-acp");
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let (mut socket, _) = connect_async(url).await.expect("connect zed websocket");
    socket
        .send(WebSocketMessage::Text(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "session/new",
                "params": {
                    "cwd": "/tmp/project"
                }
            })
            .to_string(),
        ))
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("send session new");
    let message = socket
        .next()
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("session response")
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("websocket message");
    let WebSocketMessage::Text(text) = message else {
        panic!("expected text websocket session response");
    };
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let response: serde_json::Value = serde_json::from_str(&text).expect("session json");
    let session_id = response["result"]["sessionId"]
        .as_str()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("session id");
    let zed_host = control_plane
        .acp_client_host_response("zed")
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("zed acp host");
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    assert!(zed_host.host.runtime.expect("zed runtime").connected);
    let after_revision = control_plane
        .mobile_snapshot_revision()
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("mobile revision after zed session");
    assert_ne!(after_revision, before_revision);

    let prompted = control_plane
        .prompt_acp_client_host_control_session_response(
            "zed",
            &format!("zed:codex:{session_id}"),
            "Continue from iPhone.",
        )
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("prompt zed codex session");
    assert!(prompted.delivered_to_connection);

    let message = socket
        .next()
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("prompt request")
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("websocket message");
    let WebSocketMessage::Text(text) = message else {
        panic!("expected text websocket prompt request");
    };
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let request: serde_json::Value = serde_json::from_str(&text).expect("prompt json");
    assert_eq!(request["method"], "session/prompt");
    assert_eq!(request["params"]["sessionId"], session_id);
    assert_eq!(
        request["params"]["prompt"][0]["text"],
        "Continue from iPhone."
    );

    server.abort();
}

async fn initialize_devin_acp_websocket(
    address: std::net::SocketAddr,
    path: &str,
) -> serde_json::Value {
    let url = format!("ws://{address}{path}");
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let (mut socket, _) = connect_async(url).await.expect("connect websocket");
    socket
        .send(WebSocketMessage::Text(
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": 1
                }
            })
            .to_string(),
        ))
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("send initialize");
    let message = socket
        .next()
        .await
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("initialize response")
        // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
        .expect("websocket message");
    let WebSocketMessage::Text(text) = message else {
        panic!("expected text websocket initialize response");
    };
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    serde_json::from_str(&text).expect("initialize json")
}

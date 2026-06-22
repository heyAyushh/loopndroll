use super::*;

#[tokio::test]
async fn acp_install_routes_reject_remote_callers() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let remote_socket = Some("192.168.99.25:49152".parse().expect("remote socket"));

    for path in [
        "/desktop/devin/acp-bridge/install",
        "/desktop/acp-client-hosts/devin/install",
        "/desktop/acp-client-hosts/zed/install",
    ] {
        let response = request_with_options(&router, Method::POST, path, &[], remote_socket).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]

async fn acp_control_routes_reject_remote_callers() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let remote_socket = Some("192.168.99.25:49152".parse().expect("remote socket"));
    let json_headers = [(axum::http::header::CONTENT_TYPE, "application/json")];

    for (path, body) in [
        (
            "/desktop/devin/acp-bridge/sessions",
            serde_json::json!({ "cwd": "/tmp/looper" }),
        ),
        (
            "/desktop/devin/acp-bridge/sessions/devin:looper:remote/prompt",
            serde_json::json!({ "prompt": "keep going" }),
        ),
        (
            "/desktop/acp-client-hosts/devin/sessions",
            serde_json::json!({ "cwd": "/tmp/looper" }),
        ),
        (
            "/desktop/acp-client-hosts/devin/sessions/devin:looper:remote/prompt",
            serde_json::json!({ "prompt": "keep going" }),
        ),
    ] {
        let response = request_with_body_options(
            &router,
            Method::POST,
            path,
            // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
            serde_json::to_vec(&body).expect("json body"),
            &json_headers,
            remote_socket,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }

    for path in [
        "/desktop/devin/acp-bridge/sessions/devin:looper:remote/cancel",
        "/desktop/acp-client-hosts/devin/sessions/devin:looper:remote/cancel",
    ] {
        let response = request_with_options(&router, Method::POST, path, &[], remote_socket).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn acp_websocket_routes_reject_remote_callers_before_upgrade() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());
    // SAFE-EXPECT: integration test fixture assertions should fail at the broken setup step.
    let remote_socket = Some("192.168.99.25:49152".parse().expect("remote socket"));
    let websocket_headers = [
        (axum::http::header::HOST, "127.0.0.1:8765"),
        (axum::http::header::CONNECTION, "Upgrade"),
        (axum::http::header::UPGRADE, "websocket"),
        (axum::http::header::SEC_WEBSOCKET_VERSION, "13"),
        (
            axum::http::header::SEC_WEBSOCKET_KEY,
            "dGhlIHNhbXBsZSBub25jZQ==",
        ),
    ];

    for path in [
        "/acp/devin",
        "/acp/client-hosts/devin",
        "/acp/client-hosts/zed?agentId=codex-acp",
    ] {
        let response = request_with_options(
            &router,
            Method::GET,
            path,
            &websocket_headers,
            remote_socket,
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

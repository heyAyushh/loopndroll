use super::*;

#[tokio::test]
async fn targeted_devin_hook_register_and_clear_only_touch_devin_config() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());

    let response = request_json_with_method(&router, Method::POST, "/hooks/devin/register").await;
    assert_eq!(response["action"], "register-devin-hooks");
    assert_eq!(response["removed_handlers"], 0);
    assert_eq!(response["installed_handlers"], 3);
    assert_eq!(response["hooks_auto_registration"], serde_json::json!(true));

    let devin_config_path = fixture.temp_dir.path().join(".config/devin/config.json");
    let devin_config_json = fs::read_to_string(&devin_config_path).expect("devin config");
    assert!(devin_config_json.contains("LOOPER_DEVIN_HOOK=1"));
    assert!(devin_config_json.contains("SessionStart"));
    assert!(devin_config_json.contains("Stop"));
    assert!(devin_config_json.contains("UserPromptSubmit"));
    assert!(!fixture.codex_home.join("hooks.json").exists());
    assert!(!fixture.grok_home().join("hooks/looper.json").exists());
    assert!(
        !fixture
            .temp_dir
            .path()
            .join(".claude/settings.json")
            .exists()
    );

    let clear_response =
        request_json_with_method(&router, Method::POST, "/hooks/devin/unregister-live").await;
    assert_eq!(clear_response["action"], "unregister-live-devin-hooks");
    assert_eq!(clear_response["removed_handlers"], 3);
    assert_eq!(clear_response["installed_handlers"], 0);
    assert_eq!(
        clear_response["hooks_auto_registration"],
        serde_json::json!(true)
    );

    let cleared_devin_config_json =
        fs::read_to_string(&devin_config_path).expect("cleared devin config");
    assert!(!cleared_devin_config_json.contains("agent-control-plane"));

    let register_again_response =
        request_json_with_method(&router, Method::POST, "/hooks/devin/register").await;
    assert_eq!(register_again_response["installed_handlers"], 3);

    let unregister_response =
        request_json_with_method(&router, Method::POST, "/hooks/devin/unregister").await;
    assert_eq!(unregister_response["action"], "unregister-devin-hooks");
    assert_eq!(unregister_response["removed_handlers"], 3);
    assert_eq!(unregister_response["installed_handlers"], 0);
    assert_eq!(
        unregister_response["hooks_auto_registration"],
        serde_json::json!(false)
    );

    let unregistered_devin_config_json =
        fs::read_to_string(&devin_config_path).expect("unregistered devin config");
    assert!(!unregistered_devin_config_json.contains("agent-control-plane"));
}

#[tokio::test]
async fn desktop_local_only_routes_reject_remote_callers() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());
    let remote_socket = Some("192.168.99.25:49152".parse().expect("remote socket"));
    let routes = vec![
        (Method::GET, "/desktop/connections", None),
        (Method::GET, "/desktop/pairing", None),
        (Method::GET, "/desktop/pairing-orbs/test-orb", None),
        (
            Method::PATCH,
            "/desktop/connections/mobile/mobile-1",
            Some(serde_json::json!({ "label": "Ayush iPhone" })),
        ),
        (Method::DELETE, "/desktop/connections/mobile/mobile-1", None),
        (Method::GET, "/desktop/mobile-state", None),
        (Method::GET, "/desktop/push/devices", None),
        (Method::POST, "/desktop/push/devices/install-1/test", None),
        (
            Method::POST,
            "/desktop/settings/default-prompt",
            Some(serde_json::json!({ "defaultPrompt": "Continue from desktop." })),
        ),
        (
            Method::POST,
            "/desktop/settings/scope",
            Some(serde_json::json!({ "scope": "per-task" })),
        ),
        (
            Method::POST,
            "/desktop/settings/assistant-surface",
            Some(serde_json::json!({ "assistantSurface": "codex" })),
        ),
        (
            Method::POST,
            "/desktop/settings/global-preset",
            Some(serde_json::json!({ "preset": "max-turns-1" })),
        ),
        (
            Method::POST,
            "/desktop/settings/global-notification",
            Some(serde_json::json!({ "notificationId": "route-slack" })),
        ),
        (
            Method::POST,
            "/desktop/settings/global-completion-check",
            Some(serde_json::json!({
                "completionCheckId": "check-test",
                "waitForReplyAfterCompletion": true
            })),
        ),
        (
            Method::POST,
            "/desktop/notifications",
            Some(serde_json::json!({
                "id": "route-slack",
                "label": "Slack alerts",
                "channel": "slack",
                "webhookUrl": "https://hooks.slack.com/services/test"
            })),
        ),
        (Method::DELETE, "/desktop/notifications/route-slack", None),
        (
            Method::POST,
            "/desktop/telegram/chats",
            Some(serde_json::json!({
                "botToken": "test-token",
                "waitForUpdates": false
            })),
        ),
        (
            Method::POST,
            "/desktop/completion-checks",
            Some(serde_json::json!({
                "id": "check-test",
                "label": "Tests",
                "commands": ["cargo test"]
            })),
        ),
        (
            Method::DELETE,
            "/desktop/completion-checks/check-test",
            None,
        ),
        (Method::GET, "/desktop/sessions/thread-main", None),
        (
            Method::POST,
            "/desktop/sessions/thread-main/notifications",
            Some(serde_json::json!({ "notificationIds": ["route-slack"] })),
        ),
        (
            Method::POST,
            "/desktop/sessions/thread-main/completion-check",
            Some(serde_json::json!({
                "completionCheckId": "check-test",
                "waitForReplyAfterCompletion": true
            })),
        ),
        (
            Method::POST,
            "/desktop/sessions/thread-main/archive",
            Some(serde_json::json!({ "archived": true })),
        ),
        (Method::POST, "/desktop/sessions/thread-main/mute", None),
        (Method::DELETE, "/desktop/sessions/thread-main", None),
        (Method::POST, "/desktop/shutdown", None),
    ];

    for (method, path, body) in routes {
        let response = match body {
            Some(body) => {
                request_with_body_options(
                    &router,
                    method,
                    path,
                    serde_json::to_vec(&body).expect("json body"),
                    &[(axum::http::header::CONTENT_TYPE, "application/json")],
                    remote_socket,
                )
                .await
            }
            None => request_with_options(&router, method, path, &[], remote_socket).await,
        };
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

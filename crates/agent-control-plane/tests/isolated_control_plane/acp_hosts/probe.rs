use super::*;

#[tokio::test]

async fn zed_acp_probe_reports_configured_target_blocked_when_zed_is_not_running() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_zed_settings();
    let router = build_router(fixture.control_plane());
    let loopback_socket = Some("127.0.0.1:49152".parse().expect("loopback socket"));

    let zed_probe = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/probe",
        serde_json::json!({ "agentId": "looper" }),
        &[],
        loopback_socket,
    )
    .await;

    assert_eq!(zed_probe["host"]["id"], "zed");
    assert_eq!(zed_probe["probe"]["status"], "blocked");
    assert_eq!(zed_probe["probe"]["probe_kind"], "read-only-visibility");
    assert_eq!(zed_probe["probe"]["ready"], false);
    assert_eq!(zed_probe["probe"]["control_level"], "agent-configured");
    assert_eq!(
        zed_probe["probe"]["detail"],
        "Zed host control is configured for this target, but Zed is not running."
    );
    assert!(
        zed_probe["probe"]["blockers"]
            .as_array()
            .expect("blockers")
            .iter()
            .any(|blocker| blocker == "Start Zed before probing this ACP target.")
    );
}

#[tokio::test]

async fn zed_acp_probe_blocks_running_zed_when_wrapped_command_is_missing() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_zed_settings_without_command();
    let router = build_router(fixture.control_plane_with_running_zed());
    let loopback_socket = Some("127.0.0.1:49152".parse().expect("loopback socket"));

    let zed_probe = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/probe",
        serde_json::json!({ "agentId": "looper" }),
        &[],
        loopback_socket,
    )
    .await;

    assert_eq!(zed_probe["host"]["id"], "zed");
    assert_eq!(zed_probe["probe"]["status"], "blocked");
    assert_eq!(zed_probe["probe"]["probe_kind"], "read-only-visibility");
    assert_eq!(zed_probe["probe"]["ready"], false);
    assert_eq!(zed_probe["probe"]["control_level"], "agent-configured");
    assert_eq!(
        zed_probe["probe"]["detail"],
        "Zed host control is configured for this target, but its launch command is missing."
    );
    assert!(
        zed_probe["probe"]["blockers"]
            .as_array()
            .expect("blockers")
            .iter()
            .any(|blocker| blocker
                == "Reinstall Zed host control to restore the wrapped ACP launch command.")
    );
    let targets = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/acp-targets",
        &[],
        loopback_socket,
    )
    .await;
    assert!(
        targets["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .any(|target| target["id"] == "zed:looper"
                && target["ready"] == false
                && target["status"] == "blocked")
    );
}

use super::*;
use crate::acp::targets::AcpLaunchMetadata;

#[test]
fn maps_zed_status_to_controlled_acp_client_host_for_looper_target() {
    let status = status_with_targets(vec![ZedAcpTarget {
        id: "looper".to_owned(),
        name: "Looper".to_owned(),
        target_type: Some("custom".to_owned()),
        launch_configured: true,
        looper_managed: true,
        launch: AcpLaunchMetadata {
            configured: true,
            methods: vec!["command".to_owned()],
        },
    }]);
    let runtime = LooperAcpRuntimeStatus {
        connected: true,
        connection_count: 1,
        session_count: 0,
        sessions: Vec::new(),
    };

    let host = zed_acp_client_host(&status, &runtime);

    assert_eq!(host.id, "zed");
    assert_eq!(host.label, "Zed");
    assert!(host.running);
    assert!(host.installed);
    assert_eq!(host.registry.path, "/tmp/.zed/settings.json");
    assert_eq!(host.registry.agent_count, 1);
    assert_eq!(host.agents[0].id, "looper");
    assert_eq!(host.agents[0].control_level, "agent-configured");
    assert!(host.agents[0].supports_sessions);
    assert!(host.agents[0].supports_prompt);
    assert!(host.agents[0].supports_cancel);
    assert_eq!(host.actions[0].id, "install");
    assert_eq!(
        host.actions[0].path,
        "/desktop/acp-client-hosts/zed/install"
    );
    assert_eq!(host.actions[1].id, "probe");
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    assert_eq!(host.runtime.expect("runtime").connection_count, 1);
}

#[test]
fn zed_probe_is_ready_for_looper_managed_target() {
    let status = status_with_targets(vec![ZedAcpTarget {
        id: "looper".to_owned(),
        name: "Looper".to_owned(),
        target_type: Some("custom".to_owned()),
        launch_configured: true,
        looper_managed: true,
        launch: AcpLaunchMetadata {
            configured: true,
            methods: vec!["command".to_owned()],
        },
    }]);

    let probe = zed_acp_client_host_probe(&status, Some("looper"));

    assert!(probe.ok);
    assert!(probe.ready);
    assert_eq!(probe.status, "ready");
    assert_eq!(probe.control_level, "agent-configured");
    assert_eq!(probe.probe_kind, "looper-stdio");
    assert_eq!(probe.agent_id.as_deref(), Some("looper"));
    assert_eq!(probe.launch_methods, vec!["command"]);
    assert!(
        probe
            .supported_methods
            .contains(&"session/prompt".to_owned())
    );
    assert!(probe.blockers.is_empty());
}

#[test]
fn zed_probe_blocks_looper_managed_target_when_zed_is_not_running() {
    let mut status = status_with_targets(vec![ZedAcpTarget {
        id: "looper".to_owned(),
        name: "Looper".to_owned(),
        target_type: Some("custom".to_owned()),
        launch_configured: true,
        looper_managed: true,
        launch: AcpLaunchMetadata {
            configured: true,
            methods: vec!["command".to_owned()],
        },
    }]);
    status.running = false;

    let probe = zed_acp_client_host_probe(&status, Some("looper"));

    assert!(!probe.ok);
    assert!(!probe.ready);
    assert_eq!(probe.status, "blocked");
    assert_eq!(probe.control_level, "agent-configured");
    assert_eq!(probe.probe_kind, "read-only-visibility");
    assert_eq!(probe.agent_id.as_deref(), Some("looper"));
    assert_eq!(probe.launch_methods, vec!["command"]);
    assert!(probe.supported_methods.is_empty());
    assert!(
        probe
            .blockers
            .iter()
            .any(|blocker| blocker == "Start Zed before probing this ACP target.")
    );
    assert_eq!(
        probe.detail,
        "Zed host control is configured for this target, but Zed is not running."
    );
}

#[test]
fn zed_probe_blocks_looper_managed_target_when_launch_command_is_missing() {
    let mut status = status_with_targets(vec![ZedAcpTarget {
        id: "looper".to_owned(),
        name: "Looper".to_owned(),
        target_type: Some("custom".to_owned()),
        launch_configured: false,
        looper_managed: true,
        launch: AcpLaunchMetadata::default(),
    }]);
    status.running = true;
    status.installed = true;

    let probe = zed_acp_client_host_probe(&status, Some("looper"));

    assert!(!probe.ok);
    assert!(!probe.ready);
    assert_eq!(probe.status, "blocked");
    assert_eq!(probe.control_level, "agent-configured");
    assert_eq!(probe.probe_kind, "read-only-visibility");
    assert_eq!(probe.agent_id.as_deref(), Some("looper"));
    assert!(probe.launch_methods.is_empty());
    assert!(probe.supported_methods.is_empty());
    assert!(probe.blockers.iter().any(|blocker| {
        blocker == "Reinstall Zed host control to restore the wrapped ACP launch command."
    }));
    assert_eq!(
        probe.detail,
        "Zed host control is configured for this target, but its launch command is missing."
    );
}

#[test]
fn zed_probe_reports_visibility_only_for_unmanaged_targets() {
    let status = status_with_targets(vec![ZedAcpTarget {
        id: "codex".to_owned(),
        name: "codex".to_owned(),
        target_type: Some("custom".to_owned()),
        launch_configured: true,
        looper_managed: false,
        launch: AcpLaunchMetadata {
            configured: true,
            methods: vec!["command".to_owned()],
        },
    }]);

    let probe = zed_acp_client_host_probe(&status, Some("codex-direct"));

    assert!(!probe.ok);
    assert!(!probe.ready);
    assert_eq!(probe.status, "blocked");
    assert_eq!(probe.control_level, "visibility-only");
    assert_eq!(probe.probe_kind, "read-only-visibility");
    assert_eq!(probe.agent_id.as_deref(), Some("codex-direct"));
    assert_eq!(probe.launch_methods, vec!["command"]);
    assert!(probe.supported_methods.is_empty());
    assert!(
        probe
            .blockers
            .iter()
            .any(|blocker| blocker.contains("wrapped by Looper"))
    );
}

#[test]
fn zed_probe_reserves_public_codex_for_wrapped_codex_acp_target() {
    let status = status_with_targets(vec![
        ZedAcpTarget {
            id: "codex".to_owned(),
            name: "Codex Direct".to_owned(),
            target_type: Some("custom".to_owned()),
            launch_configured: true,
            looper_managed: false,
            launch: AcpLaunchMetadata {
                configured: true,
                methods: vec!["command".to_owned()],
            },
        },
        ZedAcpTarget {
            id: "codex-acp".to_owned(),
            name: "Codex CLI".to_owned(),
            target_type: Some("custom".to_owned()),
            launch_configured: true,
            looper_managed: true,
            launch: AcpLaunchMetadata {
                configured: true,
                methods: vec!["command".to_owned()],
            },
        },
    ]);

    let probe = zed_acp_client_host_probe(&status, Some("codex"));

    assert!(probe.ok);
    assert!(probe.ready);
    assert_eq!(probe.agent_id.as_deref(), Some("codex"));
    assert_eq!(probe.name.as_deref(), Some("Codex CLI"));
    assert_eq!(probe.control_level, "agent-configured");
}

#[test]
fn zed_probe_reports_missing_requested_agent() {
    let status = status_with_targets(vec![ZedAcpTarget {
        id: "looper".to_owned(),
        name: "Looper".to_owned(),
        target_type: Some("custom".to_owned()),
        launch_configured: true,
        looper_managed: true,
        launch: AcpLaunchMetadata {
            configured: true,
            methods: vec!["command".to_owned()],
        },
    }]);

    let probe = zed_acp_client_host_probe(&status, Some("missing"));

    assert_eq!(probe.agent_id.as_deref(), Some("missing"));
    assert_eq!(probe.name, None);
    assert!(!probe.launch_configured);
    assert!(probe.launch_methods.is_empty());
    assert!(
        probe
            .blockers
            .iter()
            .any(|blocker| blocker.contains("not configured"))
    );
}

fn status_with_targets(acp_targets: Vec<ZedAcpTarget>) -> ZedStatus {
    ZedStatus {
        settings_path: "/tmp/.zed/settings.json".to_owned(),
        settings_exists: true,
        settings_error: None,
        running: true,
        installed: true,
        summary: "Zed is running with configured ACP External Agent targets".to_owned(),
        acp_target_count: acp_targets.len(),
        acp_targets,
    }
}

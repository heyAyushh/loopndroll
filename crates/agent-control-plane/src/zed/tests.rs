use std::fs;

use crate::acp::targets::AcpLaunchMetadata;

use super::install::install_looper_zed_acp_agent_for_home_with_command;
use super::*;

#[test]
fn zed_status_reads_agent_servers_without_leaking_values() {
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::create_dir_all(settings_path.parent().expect("settings parent"))
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("create settings parent");
    fs::write(
        &settings_path,
        r#"
            // Zed settings are JSONC.
            {
                "agent_servers": {
                    "looper": {
                        "type": "custom",
                        "command": "looper",
                        "args": ["acp", "stdio"],
                        "env": {
                            "TOKEN": "must-not-leak",
                            "URL": "https://example.test/not-a-comment"
                        },
                    },
                }
            }
            "#,
    )
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    .expect("write zed settings");

    let status = inspect_zed_for_home_with_processes(
        temp_dir.path(),
        &["/Applications/Zed.app/Contents/MacOS/zed --foreground".to_owned()],
    );

    assert!(status.running);
    assert!(status.installed);
    assert_eq!(status.settings_error, None);
    assert_eq!(status.acp_target_count, 1);
    assert_eq!(status.acp_targets[0].id, "looper");
    assert!(!status.acp_targets[0].looper_managed);
    assert_eq!(status.acp_targets[0].launch.methods, vec!["command"]);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let json = serde_json::to_string(&status).expect("zed status json");
    assert!(!json.contains("must-not-leak"));
    assert!(!json.contains("\"args\""));
}

#[test]
fn zed_status_reports_invalid_jsonc_settings() {
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::create_dir_all(settings_path.parent().expect("settings parent"))
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("create settings parent");
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::write(&settings_path, "{ invalid json").expect("write zed settings");

    let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);

    assert!(status.settings_exists);
    assert!(!status.installed);
    assert_eq!(status.settings_error.as_deref(), Some("invalid-json"));
    assert_eq!(status.acp_target_count, 0);
    assert_eq!(
        status.summary,
        "Zed settings file is not valid JSON or JSONC"
    );
}

#[test]
fn zed_status_reports_invalid_utf8_settings() {
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::create_dir_all(settings_path.parent().expect("settings parent"))
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("create settings parent");
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::write(&settings_path, [0xff, 0xfe]).expect("write zed settings");

    let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);

    assert_eq!(status.settings_error.as_deref(), Some("invalid-utf8"));
    assert_eq!(status.summary, "Zed settings file is not valid UTF-8");
}

#[test]
fn zed_status_reports_non_object_json_settings() {
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::create_dir_all(settings_path.parent().expect("settings parent"))
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("create settings parent");
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::write(&settings_path, "[]").expect("write zed settings");

    let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);

    assert_eq!(status.settings_error.as_deref(), Some("non-object-json"));
    assert_eq!(status.summary, "Zed settings file is not a JSON object");
}

#[test]
fn zed_acp_targets_report_configured_targets_read_only() {
    let status = ZedStatus {
        settings_path: "/tmp/.zed/settings.json".to_owned(),
        settings_exists: true,
        settings_error: None,
        running: true,
        installed: true,
        summary: "Zed is running with 1 configured ACP External Agent target".to_owned(),
        acp_target_count: 1,
        acp_targets: vec![ZedAcpTarget {
            id: "looper".to_owned(),
            name: "looper".to_owned(),
            target_type: Some("custom".to_owned()),
            launch_configured: true,
            looper_managed: false,
            launch: AcpLaunchMetadata {
                configured: true,
                methods: vec!["command".to_owned()],
            },
        }],
    };

    let targets = zed_acp_targets(&status);

    assert_eq!(targets[0].id, "zed:looper");
    assert_eq!(targets[0].client, "zed");
    assert_eq!(targets[0].status, "read-only");
    assert!(!targets[0].ready);
    assert!(targets[0].detail.contains("read-only"));
}

#[test]
fn zed_acp_targets_report_blocked_missing_launch_metadata() {
    let status = ZedStatus {
        settings_path: "/tmp/.zed/settings.json".to_owned(),
        settings_exists: true,
        settings_error: None,
        running: false,
        installed: true,
        summary: "Zed has 1 configured ACP External Agent target".to_owned(),
        acp_target_count: 1,
        acp_targets: vec![ZedAcpTarget {
            id: "configured-only".to_owned(),
            name: "configured-only".to_owned(),
            target_type: Some("custom".to_owned()),
            launch_configured: false,
            looper_managed: false,
            launch: AcpLaunchMetadata::default(),
        }],
    };

    let targets = zed_acp_targets(&status);

    assert_eq!(targets[0].id, "zed:configured-only");
    assert_eq!(targets[0].client, "zed");
    assert_eq!(targets[0].status, "blocked");
    assert!(!targets[0].ready);
}

#[test]
fn installs_looper_zed_agent_without_leaking_other_targets() {
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let settings_path = temp_dir.path().join(HOME_ZED_SETTINGS_RELATIVE_PATH);
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    fs::create_dir_all(settings_path.parent().expect("settings parent"))
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("create settings parent");
    fs::write(
        &settings_path,
        r#"{
                "theme": "Ayu Dark",
                "agent_servers": {
                    "codex": {
                        "type": "custom",
                        "command": "codex",
                        "args": ["--secret"],
                        "env": { "TOKEN": "do-not-leak" }
                    }
                }
            }"#,
    )
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    .expect("write zed settings");

    let install = install_looper_zed_acp_agent_for_home_with_command(
        temp_dir.path(),
        "/Applications/looper.app/Contents/MacOS/looper".to_owned(),
    )
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    .expect("install zed agent");

    assert_eq!(install.installed_agent_id, ZED_DEFAULT_CONTROLLED_AGENT_ID);
    assert_eq!(
        install.args,
        vec!["acp", "stdio", "zed", "codex-acp", "--", "codex-acp"]
    );
    assert!(
        install
            .command_line
            .ends_with(" acp stdio zed codex-acp -- codex-acp")
    );

    let status = inspect_zed_for_home_with_processes(temp_dir.path(), &[]);
    let codex = status
        .acp_targets
        .iter()
        .find(|target| target.id == ZED_DEFAULT_CONTROLLED_AGENT_ID)
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("codex target");
    assert!(codex.looper_managed);
    assert_eq!(codex.launch.methods, vec!["command"]);

    let targets = zed_acp_targets(&status);
    let codex_target = targets
        .iter()
        .find(|target| target.id == "zed:codex")
        // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
        .expect("codex acp target");
    assert_eq!(codex_target.agent_id, "codex");
    assert!(!codex_target.ready);
    assert_eq!(codex_target.status, "read-only");
    assert!(codex_target.detail.contains("no running Zed host"));

    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let json = serde_json::to_string(&status).expect("zed status json");
    assert!(!json.contains("do-not-leak"));
    assert!(!json.contains("--secret"));
}

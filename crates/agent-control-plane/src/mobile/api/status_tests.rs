use crate::control_plane::GrokBuildStatus;
use crate::grok_build::{GrokHookOwner, GrokHookStatus};
use crate::mobile::api::status::{mobile_devin_desktop_status, mobile_grok_build_status};

#[test]
fn mobile_grok_build_status_uses_mobile_contract() {
    let status = mobile_grok_build_status(&GrokBuildStatus {
        hooks: GrokHookStatus {
            registered_events: vec!["session".to_owned(), "stop".to_owned()],
            active_command: Some("looper hook".to_owned()),
            owner: GrokHookOwner::LooperRust,
            health: "healthy".to_owned(),
            hooks_path: Some("/Users/test/.grok/hooks/looper.json".to_owned()),
        },
        session_count: 3,
        active_session_count: 2,
    });

    assert_eq!(status["sessionCount"], 3);
    assert_eq!(status["activeSessionCount"], 2);
    assert_eq!(status["hooks"]["health"], "healthy");
    assert_eq!(status["hooks"]["owner"], "looper-rust");
    assert_eq!(
        status["hooks"]["registeredEvents"],
        serde_json::json!(["session", "stop"])
    );
}

#[test]
fn mobile_devin_desktop_status_uses_mobile_contract() {
    // SAFE-EXPECT: test fixture failures should panic with the fixture step that failed.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let devin_desktop = crate::devin::inspect_devin_desktop_with_processes(temp_dir.path(), &[]);

    let status = mobile_devin_desktop_status(&devin_desktop, 3_130, 2, &[]);

    assert_eq!(status["running"], serde_json::json!(false));
    assert_eq!(status["installed"], serde_json::json!(false));
    assert_eq!(status["acpAvailable"], serde_json::json!(false));
    assert_eq!(status["registryExists"], serde_json::json!(false));
    assert_eq!(status["sessionCount"], 3_130);
    assert_eq!(status["activeSessionCount"], 2);
    assert_eq!(status["sessionDiagnostics"], serde_json::json!([]));
}

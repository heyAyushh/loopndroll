use super::normalization::DEFAULT_ASSISTANT_SURFACE;
use super::*;
use tempfile::TempDir;

#[test]
fn mobile_session_state_persists_default_prompt_and_overrides() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .save_default_prompt("Continue this exact task.")
        .expect("save prompt");
    service
        .set_session_preset("thread-1", Some("max-turns-1"))
        .expect("set preset");
    service
        .set_siri_default_session(Some("thread-1"), Some("codex"))
        .expect("set siri default session");
    service
        .queue_prompt("thread-1", "Keep going.")
        .expect("queue prompt");
    service
        .set_session_archived("thread-1", true)
        .expect("archive");
    service.mute_session("thread-1").expect("mute");

    let mut state = service.state().expect("state");
    assert_eq!(state.default_prompt, "Continue this exact task.");
    assert_eq!(state.assistant_surface, DEFAULT_ASSISTANT_SURFACE);
    assert_eq!(state.siri_default_thread_id.as_deref(), Some("thread-1"));
    assert_eq!(
        state.siri_default_assistant_surface.as_deref(),
        Some("codex")
    );
    service
        .set_siri_current_session(Some("thread-1"), Some("codex"))
        .expect("set siri current session");
    state = service.state().expect("state");
    assert_eq!(state.siri_current_thread_id.as_deref(), Some("thread-1"));
    assert_eq!(
        state.siri_current_assistant_surface.as_deref(),
        Some("codex")
    );
    service
        .set_siri_default_session(None, None)
        .expect("clear siri default session");
    service
        .set_siri_current_session(None, None)
        .expect("clear siri current session");
    state = service.state().expect("state");
    assert_eq!(state.assistant_surface, DEFAULT_ASSISTANT_SURFACE);
    assert_eq!(state.siri_default_thread_id, None);
    assert_eq!(state.siri_default_assistant_surface, None);
    assert_eq!(state.siri_current_thread_id, None);
    assert_eq!(state.siri_current_assistant_surface, None);
    let thread = state.sessions.get("thread-1").expect("thread override");
    assert_eq!(thread.preset.as_deref(), Some("max-turns-1"));
    assert_eq!(thread.archived, Some(true));
    assert!(thread.muted);
    assert!(!thread.deleted);
}

#[test]
fn mobile_session_state_owns_mobile_routes_and_checks() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .upsert_notification_route(UpsertMobileNotificationRoute {
            id: Some("route-telegram".to_owned()),
            label: Some("Telegram DM".to_owned()),
            channel: "telegram".to_owned(),
            bot_token: Some("bot-token".to_owned()),
            chat_id: Some("chat-1".to_owned()),
            ..UpsertMobileNotificationRoute::default()
        })
        .expect("upsert notification");
    service
        .upsert_completion_check(
            "check-tests",
            "Cargo test",
            &[
                "cargo test --workspace".to_owned(),
                "cargo clippy".to_owned(),
            ],
        )
        .expect("upsert check");
    service
        .set_global_notification(Some("route-telegram"))
        .expect("set global notification");
    service
        .set_global_completion_check(Some("check-tests"), true)
        .expect("set global check");
    service
        .set_session_notifications("thread-1", &["route-telegram".to_owned()])
        .expect("set session notifications");
    service
        .set_session_completion_check("thread-1", Some("check-tests"), false)
        .expect("set session check");

    let state = service.state().expect("state");
    let thread = state.sessions.get("thread-1").expect("thread override");
    assert_eq!(state.notifications[0].label, "Telegram DM");
    assert_eq!(
        state.notifications[0].bot_token.as_deref(),
        Some("bot-token")
    );
    assert_eq!(state.notifications[0].chat_id.as_deref(), Some("chat-1"));
    assert_eq!(state.completion_checks[0].command_count(), 2);
    assert_eq!(
        state.global_notification_id.as_deref(),
        Some("route-telegram")
    );
    assert_eq!(
        state.default_notification_target_ids,
        vec!["macos".to_owned(), "route-telegram".to_owned()]
    );
    assert_eq!(
        state.global_completion_check_id.as_deref(),
        Some("check-tests")
    );
    assert!(state.global_completion_check_wait_for_reply);
    assert_eq!(thread.notification_ids, vec!["route-telegram"]);
    assert_eq!(thread.completion_check_id.as_deref(), Some("check-tests"));
    assert!(!thread.completion_check_wait_for_reply);
}

#[test]
fn default_notification_targets_drive_route_delivery() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .upsert_notification_route(UpsertMobileNotificationRoute {
            id: Some("route-telegram".to_owned()),
            label: Some("Telegram DM".to_owned()),
            channel: "telegram".to_owned(),
            bot_token: Some("bot-token".to_owned()),
            chat_id: Some("chat-1".to_owned()),
            ..UpsertMobileNotificationRoute::default()
        })
        .expect("upsert telegram");
    service
        .upsert_notification_route(UpsertMobileNotificationRoute {
            id: Some("route-slack".to_owned()),
            label: Some("Slack".to_owned()),
            channel: "slack".to_owned(),
            webhook_url: Some("https://hooks.slack.com/services/test".to_owned()),
            ..UpsertMobileNotificationRoute::default()
        })
        .expect("upsert slack");
    service
        .set_default_notification_targets(&[
            "macos".to_owned(),
            "iphone".to_owned(),
            "route-telegram".to_owned(),
            "route-slack".to_owned(),
        ])
        .expect("set targets");

    let state = service.state().expect("state");
    assert_eq!(
        state.default_notification_target_ids,
        vec![
            "iphone".to_owned(),
            "macos".to_owned(),
            "route-slack".to_owned(),
            "route-telegram".to_owned()
        ]
    );
    assert_eq!(
        service
            .notification_target_ids_for_thread("thread-1")
            .expect("target ids"),
        vec![
            "iphone".to_owned(),
            "macos".to_owned(),
            "route-slack".to_owned(),
            "route-telegram".to_owned()
        ]
    );
    assert_eq!(
        service
            .notification_routes_for_thread("thread-1")
            .expect("routes")
            .into_iter()
            .map(|route| route.id)
            .collect::<Vec<_>>(),
        vec!["route-telegram".to_owned(), "route-slack".to_owned()]
    );

    service
        .set_session_notifications("thread-1", &["route-slack".to_owned()])
        .expect("set thread routes");
    assert_eq!(
        service
            .notification_target_ids_for_thread("thread-1")
            .expect("thread target ids"),
        vec![
            "iphone".to_owned(),
            "macos".to_owned(),
            "route-slack".to_owned()
        ]
    );
    assert_eq!(
        service
            .notification_routes_for_thread("thread-1")
            .expect("thread routes")
            .into_iter()
            .map(|route| route.id)
            .collect::<Vec<_>>(),
        vec!["route-slack".to_owned()]
    );
}

#[test]
fn mobile_session_rejects_unknown_presets() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    let error = service
        .set_session_preset("thread-1", Some("bad-mode"))
        .expect_err("invalid preset");

    assert!(matches!(error, MobileSessionError::InvalidPreset));
}

#[test]
fn mobile_session_infinite_mode_reuses_persistent_mobile_prompt() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .set_session_preset("thread-1", Some("infinite"))
        .expect("set preset");
    service
        .queue_prompt("thread-1", "Keep using this phone prompt.")
        .expect("queue prompt");

    let first_decision = service
        .stop_decision("thread-1")
        .expect("first decision")
        .expect("blocks first");
    let second_decision = service
        .stop_decision("thread-1")
        .expect("second decision")
        .expect("blocks second");

    assert_eq!(first_decision.reason, "Keep using this phone prompt.");
    assert_eq!(second_decision.reason, "Keep using this phone prompt.");
}

#[test]
fn mobile_session_queue_prompt_uses_global_mode() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .set_global_preset(Some("infinite"))
        .expect("set global preset");
    service
        .queue_prompt("thread-1", "Use the global phone prompt.")
        .expect("queue prompt");

    let decision = service
        .stop_decision("thread-1")
        .expect("stop decision")
        .expect("blocks with queued prompt");

    assert_eq!(decision.reason, "Use the global phone prompt.");
}

#[test]
fn mobile_session_records_hook_lifecycle() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    let prompt_payload = MobileHookPayload {
        hook_event_name: "UserPromptSubmit".to_owned(),
        session_id: Some("thread-1".to_owned()),
        turn_id: None,
        cwd: None,
        last_assistant_message: None,
    };
    service
        .record_hook_lifecycle(&prompt_payload, false)
        .expect("record prompt lifecycle");
    let active_state = service.state().expect("active state");
    assert_eq!(active_state.lifecycle["thread-1"].status.as_str(), "active");

    let stop_payload = MobileHookPayload {
        hook_event_name: "Stop".to_owned(),
        session_id: Some("thread-1".to_owned()),
        turn_id: None,
        cwd: None,
        last_assistant_message: None,
    };
    service
        .record_hook_lifecycle(&stop_payload, false)
        .expect("record stopped lifecycle");
    let stopped_state = service.state().expect("stopped state");
    assert_eq!(
        stopped_state.lifecycle["thread-1"].status.as_str(),
        "stopped"
    );

    service
        .record_hook_lifecycle(&stop_payload, true)
        .expect("record continued lifecycle");
    let continued_state = service.state().expect("continued state");
    assert_eq!(
        continued_state.lifecycle["thread-1"].status.as_str(),
        "active"
    );
}

#[test]
fn mobile_session_hook_payload_accepts_codex_snake_case() {
    let payload: MobileHookPayload = serde_json::from_value(serde_json::json!({
        "hook_event_name": "Stop",
        "session_id": "thread-1",
        "turn_id": "turn-1",
        "cwd": "/tmp/looper",
        "last_assistant_message": "Done."
    }))
    .expect("parse snake case hook payload");

    assert_eq!(payload.hook_event_name, "Stop");
    assert_eq!(payload.session_id.as_deref(), Some("thread-1"));
    assert_eq!(payload.turn_id.as_deref(), Some("turn-1"));
    assert_eq!(payload.cwd.as_deref(), Some("/tmp/looper"));
    assert_eq!(payload.last_assistant_message.as_deref(), Some("Done."));
}

#[test]
fn mobile_session_max_turns_counts_down_and_then_allows_stop() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .save_default_prompt("Continue. Remaining: {{remaining_turns}}")
        .expect("save prompt");
    service
        .set_session_preset("thread-1", Some("max-turns-1"))
        .expect("set preset");

    let first_decision = service
        .stop_decision("thread-1")
        .expect("first decision")
        .expect("blocks first");
    let second_decision = service.stop_decision("thread-1").expect("second decision");

    assert_eq!(first_decision.reason, "Continue. Remaining: 0");
    assert!(second_decision.is_none());
}

#[test]
fn mobile_session_await_reply_consumes_queued_prompt_once() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .set_session_preset("thread-1", Some("await-reply"))
        .expect("set preset");
    service
        .queue_prompt("thread-1", "One phone reply.")
        .expect("queue prompt");

    let first_decision = service
        .stop_decision("thread-1")
        .expect("first decision")
        .expect("blocks first");
    let second_decision = service.stop_decision("thread-1").expect("second decision");

    assert_eq!(first_decision.reason, "One phone reply.");
    assert!(second_decision.is_none());
}

#[test]
fn mobile_session_completion_checks_run_configured_commands() {
    let temp_dir = TempDir::new().expect("temp dir");
    let service = MobileSessionService::new(temp_dir.path().join("control-plane.sqlite"));

    service
        .upsert_completion_check(
            "check-failing",
            "Failing check",
            &["printf 'bad output\\n' >&2; exit 7".to_owned()],
        )
        .expect("upsert failing check");
    service
        .set_global_completion_check(Some("check-failing"), false)
        .expect("set global check");
    service
        .set_global_preset(Some("completion-checks"))
        .expect("set global preset");

    let failing_decision = service
        .hook_decision_for_payload(&MobileHookPayload {
            hook_event_name: "Stop".to_owned(),
            session_id: Some("thread-1".to_owned()),
            turn_id: None,
            cwd: Some(temp_dir.path().display().to_string()),
            last_assistant_message: None,
        })
        .expect("failing decision")
        .expect("blocks stop");
    assert!(failing_decision.reason.contains("Exit code: 7"));
    assert!(failing_decision.reason.contains("bad output"));

    service
        .upsert_completion_check(
            "check-passing",
            "Passing check",
            &["printf 'ok\\n'".to_owned()],
        )
        .expect("upsert passing check");
    service
        .set_global_completion_check(Some("check-passing"), false)
        .expect("set passing global check");

    let passing_decision = service
        .hook_decision_for_payload(&MobileHookPayload {
            hook_event_name: "Stop".to_owned(),
            session_id: Some("thread-1".to_owned()),
            turn_id: None,
            cwd: Some(temp_dir.path().display().to_string()),
            last_assistant_message: None,
        })
        .expect("passing decision");
    assert!(passing_decision.is_none());
}

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;

use agent_control_plane::assistant::{
    AssistantKind, AssistantRuntimeKind, discover_assistant_adapters_from_processes,
    discover_assistant_adapters_from_sources,
};
use agent_control_plane::auth::{
    AuthManager, CloudAuthContract, LinkedIdentityMethod, MemorySecretStore,
};
use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig};
use agent_control_plane::grpc::proto::{
    SendSessionPromptRequest, SubscribeEventsRequest, looper_realtime_client::LooperRealtimeClient,
};
use agent_control_plane::http::build_router;
use agent_control_plane::mobile::events::{
    MobileEventInput, MobileEventKind, MobileEventRecord, build_mobile_event, mobile_event_sse_name,
};
use agent_control_plane::mobile::session::MobileHookPayload;
use agent_control_plane::scheduler::AutomationRunner;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use http_body_util::BodyExt;
use rusqlite::Connection;
use tempfile::TempDir;
use tower::ServiceExt;

const MOBILE_SNAPSHOT_VISIBLE_THREAD_LIMIT: usize = 12;
const EXTRA_MOBILE_SNAPSHOT_THREADS: usize = 20;
const EXTRA_THREAD_BASE_TIMESTAMP_MS: i64 = 3_000;
const DEVIN_FIXTURE_EVENT_UPDATED_AT_MS: i64 = 1_780_801_814_955;
const NEWER_THAN_DEVIN_THREAD_BASE_TIMESTAMP_MS: i64 = DEVIN_FIXTURE_EVENT_UPDATED_AT_MS + 1_000;
const GOAL_FIXTURE_TOKEN_BUDGET: i64 = 1_000;
const GOAL_FIXTURE_TOKENS_USED: i64 = 42;
const GOAL_FIXTURE_TIME_USED_SECONDS: i64 = 7;
const GOAL_FIXTURE_CREATED_AT_MS: i64 = 1_000;
const GOAL_FIXTURE_UPDATED_AT_MS: i64 = 2_000;
const SSE_CONNECTED_EVENT_TIMEOUT_SECONDS: u64 = 5;

#[tokio::test]
async fn isolated_status_capabilities_and_automation_flow() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("agent-control-plane --hook --managed-by looper");
    fixture.write_config_toml(true);
    fixture.write_state_db();
    fixture.write_automation(
        "daily-review",
        r#"
id = "daily-review"
kind = "heartbeat"
name = "Daily review"
prompt = "Summarize the local queue."
status = "ACTIVE"
rrule = "FREQ=MINUTELY;INTERVAL=5"
target_thread_id = "thread-main"
"#,
    );

    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

    let status = request_json(&router, "/status/control-plane").await;
    assert_eq!(status["hooks"]["owner"], "looper-rust");
    assert_eq!(status["hooks"]["enabled"], true);
    assert_eq!(status["hooks"]["registered_events"][0], "SessionStart");
    assert!(status["codex_servers"].is_array());

    let codex_servers = request_json(&router, "/codex/servers").await;
    assert!(codex_servers["servers"].is_array());

    let capabilities = request_json(&router, "/threads/thread-main/capabilities").await;
    assert_eq!(capabilities["thread_id"], "thread-main");
    assert_eq!(capabilities["assistant_kind"], "codex");
    assert_eq!(capabilities["tools"][0]["name"], "automation_update");
    assert_eq!(capabilities["tools"][0]["classification"], "automation");
    assert_eq!(capabilities["spawn"]["children"][0], "thread-child");

    let automations = request_json(&router, "/automations").await;
    assert_eq!(automations["automations"][0]["id"], "daily-review");
    assert_eq!(automations["automations"][0]["target_known"], true);

    let hook_contract = request_json(&router, "/integrations/hook/contract").await;
    assert_eq!(hook_contract["profile"], "agent-control-plane-local-relay");
    assert_eq!(hook_contract["payload_policy"]["raw_prompts"], false);
    assert_eq!(
        hook_contract["payload_policy"]["opaque_session_handles"],
        true
    );

    let adapter_response = request_json(&router, "/assistant-adapters").await;
    assert!(
        adapter_response["adapters"]
            .as_array()
            .expect("adapters")
            .iter()
            .any(|adapter| adapter["assistant_kind"] == "superconductor")
    );

    let threads = request_json(&router, "/threads").await;
    assert_eq!(threads["threads"].as_array().expect("threads").len(), 2);

    let thread_detail = request_json(&router, "/threads/thread-main").await;
    assert_eq!(thread_detail["thread"]["thread_id"], "thread-main");
    assert_eq!(
        thread_detail["capabilities"]["spawn"]["children"][0],
        "thread-child"
    );

    let mut runner = AutomationRunner::new(control_plane.clone());
    let fired = runner.tick(1_000_000).expect("tick should fire");
    assert_eq!(fired.len(), 1);
    assert_eq!(fired[0].automation_id, "daily-review");
    assert_eq!(fired[0].target_thread_id.as_deref(), Some("thread-main"));
    assert_eq!(fired[0].delivery_mode, "local-prompt-dispatch");
    assert_eq!(fired[0].result, "resumed");

    let duplicate = runner.tick(1_000_000).expect("dedupe tick");
    assert!(duplicate.is_empty());

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(snapshot["control_plane"]["hooks"]["owner"], "looper-rust");
    assert_eq!(snapshot["thread_count"], 2);
    assert_eq!(snapshot["active_thread_count"], 2);
    assert!(snapshot["control_plane"]["codex_servers"].is_array());
    assert_eq!(snapshot["automations"][0]["id"], "daily-review");
    assert_eq!(snapshot["automation_runs"][0]["result"], "resumed");
    assert_eq!(snapshot["goals"].as_array().expect("goals").len(), 0);
    let main_thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "thread-main")
        .expect("main thread");
    assert_eq!(
        main_thread["capabilities"]["tools"][0]["name"],
        "automation_update"
    );
}

#[tokio::test]
async fn desktop_snapshot_reads_latest_assistant_preview() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-preview.jsonl",
        &[
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Older assistant preview."
                        }
                    ]
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Latest assistant preview for Handoff."
                        }
                    ]
                }
            }),
        ],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    let main_thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "thread-main")
        .expect("main thread");

    assert_eq!(
        main_thread["assistant_preview"],
        "Latest assistant preview for Handoff."
    );
}

#[tokio::test]
async fn handoff_session_page_carries_deep_link_and_preview() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-handoff.jsonl",
        &[serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Continue this exact Looper session."
                    }
                ]
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());

    let response = request_with_options(
        &router,
        Method::GET,
        "/handoff/sessions/thread-main",
        &[(axum::http::header::HOST, "192.168.99.10:8765")],
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .expect("content type");
    assert_eq!(content_type, "text/html; charset=utf-8");
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let html = String::from_utf8(body.to_vec()).expect("html");

    assert!(html.contains("Continue this exact Looper session."));
    assert!(html.contains("looper://session/thread-main"));
    assert!(html.contains("baseURL=http%3A%2F%2F192.168.99.10%3A8765"));
}

#[tokio::test]
async fn goals_and_sync_manifest_are_metadata_only() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_automation(
        "daily-review",
        r#"
id = "daily-review"
kind = "heartbeat"
name = "Daily review"
prompt = "Do not sync this prompt body."
status = "ACTIVE"
rrule = "FREQ=MINUTELY;INTERVAL=5"
target_thread_id = "thread-main"
"#,
    );
    fixture.write_goal(
        "ship-looper",
        r#"
id = "ship-looper"
title = "Ship Looper local sync"
status = "active"
priority = "high"
target_thread_id = "thread-main"
private_notes = "do not sync this private goal body"
"#,
    );
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

    let goals = request_json(&router, "/goal").await;
    assert_eq!(goals["goals"][0]["id"], "ship-looper");
    assert_eq!(goals["goals"][0]["status"], "pursuing");
    assert_eq!(goals["goals"][0]["lifecycle"], "pursuing");
    assert_eq!(goals["goals"][0]["target_known"], true);
    assert_eq!(goals["goals"][0]["sync_safe"], true);
    assert_ne!(goals["goals"][0]["content_hash"], "");
    let plural_goals = request_json(&router, "/goals").await;
    assert_eq!(plural_goals["goals"][0]["id"], "ship-looper");

    let manifest = request_json(&router, "/sync/manifest").await;
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["privacy"]["raw_goal_bodies"], false);
    assert_eq!(manifest["privacy"]["raw_automation_prompts"], false);
    assert_eq!(manifest["privacy"]["credentials"], false);
    assert_eq!(manifest["goals"][0]["id"], "ship-looper");
    assert_eq!(manifest["automations"][0]["id"], "daily-review");
    assert_eq!(manifest["threads"][0]["thread_id"], "thread-child");

    let serialized = serde_json::to_string(&manifest).expect("serialize manifest");
    assert!(!serialized.contains("private goal body"));
    assert!(!serialized.contains("prompt body"));
    assert!(!serialized.contains("\"source_path\":"));

    let stored = control_plane
        .store()
        .latest_sync_manifest_snapshot()
        .expect("latest sync snapshot")
        .expect("sync snapshot recorded");
    assert_eq!(stored.privacy_class, "metadata-only");
    assert!(stored.body_json.contains("ship-looper"));

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(snapshot["goals"][0]["id"], "ship-looper");
    assert_eq!(snapshot["sync_manifest"]["goals"][0]["id"], "ship-looper");
}

#[tokio::test]
async fn goal_lifecycle_states_match_codex_goal_surface() {
    let fixture = IsolatedCodexFixture::new();
    for (id, status) in [
        ("pursuing-goal", "pursuing"),
        ("paused-goal", "paused"),
        ("achieved-goal", "achieved"),
        ("unmet-goal", "unmet"),
        ("budget-goal", "budget-limited"),
    ] {
        fixture.write_goal(
            id,
            &format!(
                r#"
id = "{id}"
title = "{id}"
status = "{status}"
"#
            ),
        );
    }
    let router = build_router(fixture.control_plane());

    let goals = request_json(&router, "/goal").await;
    let statuses = goals["goals"]
        .as_array()
        .expect("goals")
        .iter()
        .map(|goal| goal["status"].as_str().expect("status"))
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec!["achieved", "budget-limited", "paused", "pursuing", "unmet"]
    );

    let manifest = request_json(&router, "/sync/manifest").await;
    assert_eq!(manifest["goals"][0]["status"], "achieved");
    assert_eq!(manifest["goals"][0]["lifecycle"], "achieved");
}

#[tokio::test]
async fn codex_goal_database_marks_running_goal_on_session() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_thread_goal(
        "thread-main",
        "goal-main",
        "Make Looper understand running goals",
        "active",
    );
    let router = build_router(fixture.control_plane());

    let goals = request_json(&router, "/goals").await;
    let goal = goals["goals"]
        .as_array()
        .expect("goals")
        .iter()
        .find(|goal| goal["id"] == "goal-main")
        .expect("sqlite goal");
    assert_eq!(goal["source_kind"], "sqlite");
    assert_eq!(goal["status"], "pursuing");
    assert_eq!(goal["running"], true);
    assert_eq!(goal["target_thread_id"], "thread-main");
    assert_eq!(goal["target_known"], true);
    assert_eq!(goal["tokens_used"], GOAL_FIXTURE_TOKENS_USED);

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    let thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "thread-main")
        .expect("thread-main");
    assert_eq!(thread["goal"]["id"], "goal-main");
    assert_eq!(thread["goal"]["running"], true);

    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];
    let mobile_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let session = mobile_snapshot_session(&mobile_snapshot, "thread-main");
    assert_eq!(session["goal"]["id"], "goal-main");
    assert_eq!(session["goal"]["running"], true);

    let manifest = request_json(&router, "/sync/manifest").await;
    let sync_goal = manifest["goals"]
        .as_array()
        .expect("sync goals")
        .iter()
        .find(|goal| goal["id"] == "goal-main")
        .expect("sync sqlite goal");
    assert_eq!(sync_goal["running"], true);
    assert_eq!(sync_goal["tokens_used"], GOAL_FIXTURE_TOKENS_USED);
}

#[tokio::test]
async fn degraded_source_is_reported_without_touching_user_state() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("bun legacy/bun/managed-hook-script.ts");
    fixture.write_config_toml(true);
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane);

    let status = request_json(&router, "/status/control-plane").await;
    assert_eq!(status["hooks"]["owner"], "unknown");
    assert_eq!(status["source"]["health"], "degraded");
    assert_eq!(
        status["source"]["degraded_reason"],
        "missing Codex state DB"
    );
}

#[tokio::test]
async fn nested_hooks_json_shape_is_supported() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_nested_hooks_json("agent-control-plane hook --managed-by looper");
    fixture.write_config_toml(true);
    let router = build_router(fixture.control_plane());

    let status = request_json(&router, "/status/control-plane").await;
    assert_eq!(status["hooks"]["owner"], "looper-rust");
    assert_eq!(
        status["hooks"]["active_command"],
        "agent-control-plane hook --managed-by looper"
    );
    assert_eq!(status["hooks"]["registered_events"][0], "SessionStart");
    assert_eq!(status["hooks"]["registered_events"][1], "Stop");
    assert_eq!(status["hooks"]["registered_events"][2], "UserPromptSubmit");
}

#[tokio::test]
async fn disabled_owned_hook_state_degrades_health_until_register_repairs_it() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_nested_hooks_json("agent-control-plane hook --managed-by looper");
    fixture.write_config_toml_with_disabled_owned_hook_state();
    let router = build_router(fixture.control_plane());

    let degraded_status = request_json(&router, "/status/control-plane").await;
    assert_eq!(degraded_status["hooks"]["owner"], "looper-rust");
    assert_eq!(degraded_status["hooks"]["health"], "degraded");
    assert!(
        degraded_status["hooks"]["issues"]
            .as_array()
            .expect("issues")
            .iter()
            .any(|issue| issue
                .as_str()
                .expect("issue")
                .contains("user_prompt_submit:0:0"))
    );

    let register_response =
        request_json_with_method(&router, Method::POST, "/hooks/register").await;
    assert_eq!(register_response["status"]["hooks"]["health"], "healthy");
    assert!(
        register_response["status"]["hooks"]["issues"]
            .as_array()
            .expect("issues")
            .is_empty()
    );

    let config_toml = fs::read_to_string(fixture.codex_home.join("config.toml")).expect("config");
    assert!(config_toml.contains(&format!(
        "[hooks.state.\"{}:user_prompt_submit:0:0\"]\nenabled = true",
        fixture.codex_home.join("hooks.json").display()
    )));
    assert!(config_toml.contains(&format!(
        "[hooks.state.\"{}:user_prompt_submit:1:0\"]\nenabled = false",
        fixture.codex_home.join("hooks.json").display()
    )));
}

#[tokio::test]
async fn live_codex_thread_schema_is_supported() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("agent-control-plane hook --managed-by looper");
    fixture.write_config_toml(true);
    fixture.write_live_shape_state_db();
    let router = build_router(fixture.control_plane());

    let detail = request_json(&router, "/threads/live-thread").await;
    assert_eq!(detail["thread"]["thread_id"], "live-thread");
    assert_eq!(detail["thread"]["git_branch"], "main");
    assert_eq!(detail["thread"]["cli_version"], "0.124.0");
    assert_eq!(detail["capabilities"]["diff"]["git_sha"], "abc123");
    assert_eq!(detail["capabilities"]["agent_role"], "explorer");
}

#[tokio::test]
async fn compaction_events_are_exposed_as_local_hook_events() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("agent-control-plane hook --managed-by looper");
    fixture.write_config_toml(true);
    fixture.write_state_db();
    fixture.write_rollout_with_compaction("thread-main", "2026-04-30T11:42:00.123Z");
    let router = build_router(fixture.control_plane());

    let compactions = request_json(&router, "/codex/compactions").await;
    assert_eq!(compactions["events"].as_array().expect("events").len(), 1);
    assert_eq!(compactions["events"][0]["thread_id"], "thread-main");
    assert_eq!(
        compactions["events"][0]["occurred_at"],
        "2026-04-30T11:42:00.123Z"
    );

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(snapshot["compactions"][0]["thread_id"], "thread-main");

    let hook_contract = request_json(&router, "/integrations/hook/contract").await;
    assert!(
        hook_contract["events"]
            .as_array()
            .expect("hook events")
            .iter()
            .any(|event| event["event_type"] == "codex.context_compacted")
    );
}

#[tokio::test]
async fn unregister_hooks_removes_only_owned_rust_handlers() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_config_toml(true);
    fixture.write_mixed_hooks_json();
    let router = build_router(fixture.control_plane());

    let response = request_json_with_method(&router, Method::POST, "/hooks/unregister").await;
    assert_eq!(response["action"], "unregister-hooks");
    assert_eq!(response["removed_handlers"], 4);
    assert_eq!(response["installed_handlers"], 0);
    assert_eq!(response["hooks_auto_registration"], false);
    assert_eq!(response["status"]["hooks"]["owner"], "unknown");

    let hooks_json = fs::read_to_string(fixture.codex_home.join("hooks.json")).expect("hooks");
    assert!(!hooks_json.contains("agent-control-plane"));
    assert!(!hooks_json.contains("managed-hook-script"));
    assert!(hooks_json.contains("/usr/local/bin/custom-user-hook"));

    let config_toml = fs::read_to_string(fixture.codex_home.join("config.toml")).expect("config");
    assert!(config_toml.contains("[features]"));
    assert!(config_toml.contains("hooks = true"));
    assert!(!config_toml.contains("codex_hooks"));

    let status = request_json(&router, "/status/control-plane").await;
    assert_eq!(status["hooks"]["registered_events"][0], "Stop");
}

#[tokio::test]
async fn register_hooks_installs_owned_rust_handlers() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_config_toml_with_model_block();
    fixture.write_user_hooks_json();
    let router = build_router(fixture.control_plane());

    let response = request_json_with_method(&router, Method::POST, "/hooks/register").await;
    assert_eq!(response["action"], "register-hooks");
    assert_eq!(response["removed_handlers"], 0);
    assert_eq!(response["installed_handlers"], 12);
    assert_eq!(response["hooks_auto_registration"], true);
    assert_eq!(response["status"]["hooks"]["enabled"], true);
    assert_eq!(response["status"]["hooks"]["owner"], "looper-rust");

    let hooks_json = fs::read_to_string(fixture.codex_home.join("hooks.json")).expect("hooks");
    assert!(hooks_json.contains("agent-control-plane --hook --managed-by looper"));
    assert!(hooks_json.contains("SessionStart"));
    assert!(hooks_json.contains("Stop"));
    assert!(hooks_json.contains("UserPromptSubmit"));
    assert!(hooks_json.contains("/usr/local/bin/custom-user-hook"));

    let grok_hooks_json =
        fs::read_to_string(fixture.grok_home().join("hooks/looper.json")).expect("grok hooks");
    assert!(grok_hooks_json.contains("agent-control-plane --hook --managed-by looper"));
    assert!(grok_hooks_json.contains("SessionStart"));
    assert!(grok_hooks_json.contains("Stop"));
    assert!(grok_hooks_json.contains("UserPromptSubmit"));

    let devin_config_json =
        fs::read_to_string(fixture.temp_dir.path().join(".config/devin/config.json"))
            .expect("devin config");
    assert!(devin_config_json.contains("LOOPER_DEVIN_HOOK=1"));
    assert!(devin_config_json.contains("SessionStart"));
    assert!(devin_config_json.contains("Stop"));
    assert!(devin_config_json.contains("UserPromptSubmit"));

    let claude_settings_json =
        fs::read_to_string(fixture.temp_dir.path().join(".claude/settings.json"))
            .expect("claude settings");
    assert!(claude_settings_json.contains("LOOPER_CLAUDE_HOOK=1"));
    assert!(claude_settings_json.contains("SessionStart"));
    assert!(claude_settings_json.contains("Stop"));
    assert!(claude_settings_json.contains("UserPromptSubmit"));

    let config_toml = fs::read_to_string(fixture.codex_home.join("config.toml")).expect("config");
    assert!(config_toml.contains("[features]"));
    assert!(config_toml.contains("hooks = true"));
    assert!(!config_toml.contains("codex_hooks"));
    assert!(config_toml.contains("[model]"));
}

#[tokio::test]
async fn targeted_hook_register_and_clear_only_touch_selected_source() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_config_toml_with_model_block();
    let router = build_router(fixture.control_plane());

    let response = request_json_with_method(&router, Method::POST, "/hooks/grok/register").await;
    assert_eq!(response["action"], "register-grok-build-hooks");
    assert_eq!(response["removed_handlers"], 0);
    assert_eq!(response["installed_handlers"], 3);
    assert_eq!(response["hooks_auto_registration"], true);

    let grok_hooks_path = fixture.grok_home().join("hooks/looper.json");
    let grok_hooks_json = fs::read_to_string(&grok_hooks_path).expect("grok hooks");
    assert!(grok_hooks_json.contains("agent-control-plane --hook --managed-by looper"));
    assert!(!fixture.codex_home.join("hooks.json").exists());
    assert!(
        !fixture
            .temp_dir
            .path()
            .join(".claude/settings.json")
            .exists()
    );

    let clear_response =
        request_json_with_method(&router, Method::POST, "/hooks/grok/unregister-live").await;
    assert_eq!(clear_response["action"], "unregister-live-grok-build-hooks");
    assert_eq!(clear_response["removed_handlers"], 3);
    assert_eq!(clear_response["installed_handlers"], 0);
    assert_eq!(clear_response["hooks_auto_registration"], true);

    let cleared_grok_hooks_json = fs::read_to_string(grok_hooks_path).expect("cleared grok hooks");
    assert!(!cleared_grok_hooks_json.contains("agent-control-plane"));
}

#[tokio::test]
async fn unknown_hook_target_is_rejected() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());

    let response = request_with_options(
        &router,
        Method::POST,
        "/hooks/devin/register",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn hook_mutation_routes_reject_remote_callers() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_config_toml(true);
    fixture.write_hooks_json("agent-control-plane --hook --managed-by looper");
    let router = build_router(fixture.control_plane());
    let remote_socket = Some("192.168.99.25:49152".parse().expect("remote socket"));

    for path in [
        "/hooks/clear",
        "/hooks/register",
        "/hooks/grok/register",
        "/hooks/unregister",
        "/hooks/unregister-live",
        "/hooks/grok/unregister-live",
    ] {
        let response = request_with_options(&router, Method::POST, path, &[], remote_socket).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn acp_install_routes_reject_remote_callers() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());
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
async fn live_unregister_preserves_auto_registration_for_next_launch() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_config_toml(true);
    fixture.write_hooks_json("agent-control-plane --hook --managed-by looper");
    let router = build_router(fixture.control_plane());

    let response = request_json_with_method(&router, Method::POST, "/hooks/unregister-live").await;
    assert_eq!(response["action"], "unregister-live-hooks");
    assert_eq!(response["hooks_auto_registration"], true);
    assert_eq!(response["removed_handlers"], 3);
    assert_eq!(response["installed_handlers"], 0);

    let hooks_json = fs::read_to_string(fixture.codex_home.join("hooks.json")).expect("hooks");
    assert!(!hooks_json.contains("agent-control-plane"));

    let register_response =
        request_json_with_method(&router, Method::POST, "/hooks/register").await;
    assert_eq!(register_response["hooks_auto_registration"], true);
    assert_eq!(register_response["installed_handlers"], 12);
    assert_eq!(register_response["status"]["hooks"]["owner"], "looper-rust");
}

#[tokio::test]
async fn mobile_health_prefers_reachable_request_host() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());

    let response = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/health",
        &[(axum::http::header::HOST, "192.168.99.10:8765")],
        None,
    )
    .await;

    assert_eq!(response["ok"], true);
    assert_eq!(response["baseURL"], "http://192.168.99.10:8765");
    assert_eq!(response["baseURLs"][0], "http://192.168.99.10:8765");
}

#[tokio::test]
async fn mobile_connection_code_is_loopback_only_and_issues_rust_pairing() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());

    let remote_response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/connection-code",
        &[],
        Some("192.168.99.25:49152".parse().expect("remote socket")),
    )
    .await;
    assert_eq!(remote_response.status(), StatusCode::FORBIDDEN);

    let local_response = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/connection-code",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_ne!(local_response["pairingTokenId"], "");
    assert_ne!(local_response["pairingToken"], "");
    assert_ne!(local_response["code"], "");
    assert_ne!(local_response["orbId"], "");
    assert!(
        !local_response["baseURLs"]
            .as_array()
            .expect("baseURLs")
            .is_empty()
    );
}

#[tokio::test]
async fn desktop_connections_manage_mobile_pairings_and_codex_rows() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("agent-control-plane --hook --managed-by looper");
    fixture.write_config_toml(true);
    fixture.write_devin_next_settings();
    fixture.write_zed_settings();
    let control_plane = fixture.control_plane();
    let pairing_token = control_plane
        .mobile_auth_service()
        .issue_pairing_token()
        .expect("issue pairing token");
    let router = build_router(control_plane);
    let loopback_socket = Some("127.0.0.1:49152".parse().expect("loopback socket"));

    let remote_response = request_with_options(
        &router,
        Method::GET,
        "/desktop/connections",
        &[],
        Some("192.168.1.10:49152".parse().expect("remote socket")),
    )
    .await;
    assert_eq!(remote_response.status(), StatusCode::FORBIDDEN);

    let connections = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/connections",
        &[],
        loopback_socket,
    )
    .await;
    assert!(
        connections["connections"]
            .as_array()
            .expect("connections")
            .iter()
            .any(
                |connection| connection["id"] == pairing_token.id && connection["kind"] == "mobile"
            )
    );
    assert!(
        connections["connections"]
            .as_array()
            .expect("connections")
            .iter()
            .any(|connection| connection["id"] == "codex-hooks" && connection["kind"] == "codex")
    );
    assert!(
        connections["connections"]
            .as_array()
            .expect("connections")
            .iter()
            .any(|connection| connection["id"] == "devin-desktop-next"
                && connection["kind"] == "devin"
                && (connection["status"] == "configured" || connection["status"] == "connected")
                && connection["detail"]
                    .as_str()
                    .expect("devin detail")
                    .contains("preferred: codex"))
    );
    let grok_connections: Vec<_> = connections["connections"]
        .as_array()
        .expect("connections")
        .iter()
        .filter(|connection| connection["kind"] == "grok-build")
        .collect();
    assert!(
        grok_connections
            .iter()
            .any(|connection| connection["id"] == "grok-build-cli"),
        "expected grok-build-cli connection"
    );
    assert!(
        grok_connections
            .iter()
            .any(|connection| connection["id"] == "grok-build-hooks"),
        "expected grok-build-hooks connection"
    );
    assert!(
        connections["connections"]
            .as_array()
            .expect("connections")
            .iter()
            .any(|connection| connection["id"] == "claude-code-hooks"
                && connection["kind"] == "claude-code"),
        "expected claude-code-hooks connection"
    );
    assert!(
        connections["connections"]
            .as_array()
            .expect("connections")
            .iter()
            .any(|connection| connection["id"] == "zed-acp"
                && connection["kind"] == "zed"
                && (connection["status"] == "configured" || connection["status"] == "connected")),
        "expected zed-acp connection"
    );
    for connection in grok_connections {
        assert!(
            connection["status"] == "connected"
                || connection["status"] == "installed"
                || connection["status"] == "missing"
                || connection["status"] == "healthy"
                || connection["status"] == "configured",
            "unexpected grok-build status: {}",
            connection["status"]
        );
    }

    let devin =
        request_json_with_options(&router, Method::GET, "/desktop/devin", &[], loopback_socket)
            .await;
    assert_eq!(
        devin["status"]["installations"][1]["preferred_agent"],
        "codex"
    );
    assert_eq!(devin["status"]["acp_registry"]["agents"][0]["id"], "codex");
    assert_eq!(
        devin["status"]["acp_bridge"]["control_level"],
        "agent-configured"
    );
    assert_eq!(
        devin["status"]["acp_bridge"]["agents"][0]["launch_configured"],
        true
    );
    let devin_json = serde_json::to_string(&devin).expect("devin json");
    assert!(!devin_json.contains("must-not-leak"));
    assert!(!devin_json.contains("@agentclientprotocol/codex-acp"));

    let zed =
        request_json_with_options(&router, Method::GET, "/desktop/zed", &[], loopback_socket).await;
    assert_eq!(zed["status"]["acp_target_count"], 1);
    assert_eq!(zed["status"]["acp_targets"][0]["id"], "looper");
    let zed_json = serde_json::to_string(&zed).expect("zed json");
    assert!(!zed_json.contains("zed-secret-token"));

    let acp_bridge = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/devin/acp-bridge",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(acp_bridge["bridge"]["agents"][0]["id"], "codex");
    assert_eq!(
        acp_bridge["bridge"]["agents"][0]["control_level"],
        "agent-configured"
    );
    let acp_bridge_actions = acp_bridge["bridge"]["actions"]
        .as_array()
        .expect("bridge actions");
    assert!(acp_bridge_actions.iter().any(|action| {
        action["id"] == "install" && action["path"] == "/desktop/devin/acp-bridge/install"
    }));
    assert!(acp_bridge_actions.iter().any(|action| {
        action["id"] == "probe" && action["path"] == "/desktop/devin/acp-bridge/probe"
    }));
    assert!(
        acp_bridge["bridge"]["limitations"]
            .as_array()
            .expect("limitations")
            .iter()
            .any(|limitation| limitation
                .as_str()
                .expect("limitation")
                .contains("never auto-executes"))
    );
    let acp_bridge_json = serde_json::to_string(&acp_bridge).expect("acp bridge json");
    assert!(!acp_bridge_json.contains("must-not-leak"));
    assert!(!acp_bridge_json.contains("@agentclientprotocol/codex-acp"));

    let acp_targets = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/acp-targets",
        &[],
        loopback_socket,
    )
    .await;
    let acp_targets_json = serde_json::to_string(&acp_targets).expect("acp targets json");
    assert!(
        acp_targets["targets"]
            .as_array()
            .expect("acp targets")
            .iter()
            .any(|target| target["id"] == "devin:codex"
                && target["client"] == "devin"
                && target["ready"] == true)
    );
    assert!(
        acp_targets["targets"]
            .as_array()
            .expect("acp targets")
            .iter()
            .any(|target| target["id"] == "zed:looper"
                && target["client"] == "zed"
                && target["ready"] == false
                && target["status"] == "read-only"
                && target["launch"]["methods"]
                    .as_array()
                    .expect("launch methods")
                    .iter()
                    .any(|method| method == "command"))
    );
    assert!(!acp_targets_json.contains("zed-secret-token"));
    assert!(!acp_targets_json.contains("@agentclientprotocol/codex-acp"));

    let acp_probe = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/devin/acp-bridge/probe",
        serde_json::json!({ "agentId": "codex" }),
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(acp_probe["probe"]["status"], "ready");
    assert_eq!(acp_probe["probe"]["agent_id"], "codex");
    assert_eq!(acp_probe["probe"]["ready"], true);
    assert_eq!(acp_probe["probe"]["probe_kind"], "launch-preflight");
    let probe_action = acp_probe["bridge"]["actions"]
        .as_array()
        .expect("probe actions")
        .iter()
        .find(|action| action["id"] == "probe")
        .expect("probe action");
    assert_eq!(probe_action["default_agent_id"], "codex");
    let acp_probe_json = serde_json::to_string(&acp_probe).expect("acp probe json");
    assert!(!acp_probe_json.contains("must-not-leak"));
    assert!(!acp_probe_json.contains("@agentclientprotocol/codex-acp"));

    let acp_hosts = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/acp-client-hosts",
        &[],
        loopback_socket,
    )
    .await;
    let acp_host_rows = acp_hosts["hosts"].as_array().expect("acp hosts");
    let devin_acp_host = acp_host_rows
        .iter()
        .find(|host| host["id"] == "devin")
        .expect("devin acp host");
    let zed_acp_host = acp_host_rows
        .iter()
        .find(|host| host["id"] == "zed")
        .expect("zed acp host");
    assert_eq!(devin_acp_host["label"], "Devin Desktop");
    assert_eq!(devin_acp_host["registry"]["agent_count"], 1);
    assert_eq!(
        devin_acp_host["actions"][0]["path"],
        "/desktop/acp-client-hosts/devin/install"
    );
    assert_eq!(zed_acp_host["label"], "Zed");
    assert_eq!(zed_acp_host["registry"]["agent_count"], 1);
    assert_eq!(zed_acp_host["agents"][0]["id"], "looper");
    assert_eq!(
        zed_acp_host["agents"][0]["control_level"],
        "visibility-only"
    );
    assert!(
        zed_acp_host["actions"]
            .as_array()
            .expect("zed actions")
            .iter()
            .any(|action| action["id"] == "probe"
                && action["path"] == "/desktop/acp-client-hosts/zed/probe"
                && action["default_agent_id"] == "looper")
    );
    let acp_hosts_json = serde_json::to_string(&acp_hosts).expect("acp hosts json");
    assert!(!acp_hosts_json.contains("must-not-leak"));
    assert!(!acp_hosts_json.contains("@agentclientprotocol/codex-acp"));
    assert!(!acp_hosts_json.contains("zed-secret-token"));

    let acp_host = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/acp-client-hosts/devin",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(acp_host["host"]["id"], "devin");
    assert_eq!(acp_host["host"]["agents"][0]["id"], "codex");
    assert_eq!(
        acp_host["host"]["agents"][0]["control_level"],
        "agent-configured"
    );

    let zed_host = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/acp-client-hosts/zed",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(zed_host["host"]["id"], "zed");
    assert_eq!(zed_host["host"]["agents"][0]["id"], "looper");

    let zed_generic_probe = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/probe",
        serde_json::json!({ "agentId": "looper" }),
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(zed_generic_probe["host"]["id"], "zed");
    assert_eq!(zed_generic_probe["probe"]["status"], "blocked");
    assert_eq!(
        zed_generic_probe["probe"]["probe_kind"],
        "read-only-visibility"
    );
    assert_eq!(zed_generic_probe["probe"]["agent_id"], "looper");

    let zed_install = request_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/install",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(zed_install.status(), StatusCode::METHOD_NOT_ALLOWED);

    let generic_acp_probe = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/devin/probe",
        serde_json::json!({ "agentId": "codex" }),
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(generic_acp_probe["host"]["id"], "devin");
    assert_eq!(generic_acp_probe["probe"]["status"], "ready");
    assert_eq!(generic_acp_probe["probe"]["agent_id"], "codex");
    assert_eq!(
        generic_acp_probe["host"]["actions"][1]["path"],
        "/desktop/acp-client-hosts/devin/probe"
    );

    let acp_install = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/devin/acp-bridge/install",
        serde_json::Value::Null,
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(acp_install["install"]["installed_agent_id"], "looper");
    assert_eq!(
        acp_install["install"]["websocket_url"],
        "ws://127.0.0.1:8765/acp/client-hosts/devin"
    );
    let devin_after_install =
        request_json_with_options(&router, Method::GET, "/desktop/devin", &[], loopback_socket)
            .await;
    assert_eq!(
        devin_after_install["status"]["installations"][1]["preferred_agent"],
        "looper"
    );
    assert!(
        devin_after_install["status"]["acp_registry"]["agents"]
            .as_array()
            .expect("agents")
            .iter()
            .any(|agent| agent["id"] == "looper"
                && agent["launch_configured"] == true
                && agent["launch"]["methods"]
                    .as_array()
                    .expect("launch methods")
                    .iter()
                    .any(|method| method == "websocket"))
    );

    let generic_acp_install = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/devin/install",
        serde_json::Value::Null,
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(generic_acp_install["host"]["id"], "devin");
    assert_eq!(generic_acp_install["install"]["client_id"], "devin");
    assert_eq!(
        generic_acp_install["install"]["installed_agent_id"],
        "looper"
    );
    assert_eq!(
        generic_acp_install["install"]["transport_url"],
        "ws://127.0.0.1:8765/acp/client-hosts/devin"
    );

    let renamed = request_json_body_with_options(
        &router,
        Method::PATCH,
        &format!("/desktop/connections/mobile/{}", pairing_token.id),
        serde_json::json!({ "label": "Desk iPhone" }),
        &[],
        loopback_socket,
    )
    .await;
    let renamed_mobile = renamed["connections"]
        .as_array()
        .expect("connections")
        .iter()
        .find(|connection| connection["id"] == pairing_token.id)
        .expect("renamed mobile");
    assert_eq!(renamed_mobile["label"], "Desk iPhone");

    let revoked = request_json_with_options(
        &router,
        Method::DELETE,
        &format!("/desktop/connections/mobile/{}", pairing_token.id),
        &[],
        loopback_socket,
    )
    .await;
    let revoked_mobile = revoked["connections"]
        .as_array()
        .expect("connections")
        .iter()
        .find(|connection| connection["id"] == pairing_token.id)
        .expect("revoked mobile");
    assert_eq!(revoked_mobile["status"], "revoked");
    assert_eq!(revoked_mobile["can_revoke"], false);
}

#[tokio::test]
async fn mobile_connection_orbs_are_mac_displayed_and_phone_resolved() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());

    let remote_desktop_pairing_response = request_with_options(
        &router,
        Method::GET,
        "/desktop/pairing",
        &[],
        Some("192.168.99.25:49152".parse().expect("remote socket")),
    )
    .await;
    assert_eq!(
        remote_desktop_pairing_response.status(),
        StatusCode::FORBIDDEN
    );

    let desktop_pairing = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/pairing",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    let desktop_orb_id = desktop_pairing["orbId"].as_str().expect("desktop orb id");
    assert_ne!(desktop_orb_id, "");
    assert_eq!(
        desktop_pairing["orbImagePath"]
            .as_str()
            .expect("desktop orb image path"),
        format!("/desktop/pairing-orbs/{desktop_orb_id}")
    );
    assert_eq!(
        desktop_pairing["orbResolvePath"]
            .as_str()
            .expect("desktop orb resolve path"),
        format!("/api/mobile/connection-orbs/{desktop_orb_id}")
    );

    let desktop_orb_image_response = request_with_options(
        &router,
        Method::GET,
        &format!("/desktop/pairing-orbs/{desktop_orb_id}"),
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(desktop_orb_image_response.status(), StatusCode::OK);
    assert_eq!(
        desktop_orb_image_response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );

    let resolved_desktop_pairing = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/connection-orbs/{desktop_orb_id}"),
        &[],
        Some("192.168.99.25:49152".parse().expect("phone socket")),
    )
    .await;
    assert_eq!(resolved_desktop_pairing["code"], desktop_pairing["code"]);

    let remote_image_response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/connection-orb.png",
        &[],
        Some("192.168.99.25:49152".parse().expect("remote socket")),
    )
    .await;
    assert_eq!(remote_image_response.status(), StatusCode::FORBIDDEN);

    let image_response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/connection-orb.png",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(image_response.status(), StatusCode::OK);
    assert_eq!(
        image_response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    let image_body = image_response
        .into_body()
        .collect()
        .await
        .expect("image body")
        .to_bytes();
    assert!(!image_body.is_empty());

    let connection_code = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/connection-code",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    let orb_id = connection_code["orbId"].as_str().expect("orb id");
    let resolved_connection_code = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/connection-orbs/{orb_id}"),
        &[],
        Some("192.168.99.25:49152".parse().expect("phone socket")),
    )
    .await;
    assert_eq!(resolved_connection_code["code"], connection_code["code"]);

    let second_resolve_response = request_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/connection-orbs/{orb_id}"),
        &[],
        Some("192.168.99.25:49152".parse().expect("phone socket")),
    )
    .await;
    assert_eq!(second_resolve_response.status(), StatusCode::GONE);
}

#[tokio::test]
async fn mobile_snapshot_uses_rust_auth_and_codex_threads() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());
    let connection_code = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/connection-code",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    let token_id = connection_code["pairingTokenId"]
        .as_str()
        .expect("pairing token id");
    let token = connection_code["pairingToken"]
        .as_str()
        .expect("pairing token");
    let authorization = format!("Bearer {token_id}.{token}");

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &[
            (axum::http::header::AUTHORIZATION, authorization.as_str()),
            (axum::http::header::HOST, "192.168.99.10:8765"),
        ],
        None,
    )
    .await;

    assert_eq!(snapshot["host"]["address"], "http://192.168.99.10:8765");
    assert_eq!(snapshot["sessions"][0]["id"], "thread-child");
    assert_eq!(snapshot["sessions"][1]["id"], "thread-main");
}

#[tokio::test]
async fn mobile_snapshot_caps_initial_thread_list() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.append_state_threads(EXTRA_MOBILE_SNAPSHOT_THREADS);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    let sessions = snapshot["sessions"].as_array().expect("sessions");
    assert_eq!(sessions.len(), MOBILE_SNAPSHOT_VISIBLE_THREAD_LIMIT);
    assert!(sessions.iter().all(|session| {
        session["id"]
            .as_str()
            .expect("session id")
            .starts_with("thread-extra-")
    }));
}

#[tokio::test]
async fn desktop_menu_snapshot_uses_bounded_codex_rows_with_total_counts() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.append_state_threads(EXTRA_MOBILE_SNAPSHOT_THREADS);
    fixture.write_automation(
        "hidden-main",
        r#"
id = "hidden-main"
kind = "heartbeat"
name = "Hidden main"
prompt = "Review hidden main."
status = "ACTIVE"
rrule = "FREQ=MINUTELY;INTERVAL=5"
target_thread_id = "thread-main"
"#,
    );
    let router = build_router(fixture.control_plane());

    let snapshot = request_json(&router, "/desktop/snapshot?profile=menu").await;

    assert_eq!(snapshot["thread_count"], 22);
    assert_eq!(snapshot["active_thread_count"], 22);
    assert_eq!(
        snapshot["threads"].as_array().expect("threads").len(),
        MOBILE_SNAPSHOT_VISIBLE_THREAD_LIMIT
    );
    assert!(
        snapshot["threads"]
            .as_array()
            .expect("threads")
            .iter()
            .all(|thread| thread["thread_id"]
                .as_str()
                .expect("thread id")
                .starts_with("thread-extra-"))
    );
    assert_eq!(snapshot["automations"][0]["target_known"], true);
}

#[tokio::test]
async fn mobile_snapshot_exposes_rust_owned_routes_and_checks() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let service = control_plane.mobile_session_service();
    service
        .upsert_notification_route(
            agent_control_plane::mobile::session::UpsertMobileNotificationRoute {
                id: Some("route-telegram".to_owned()),
                label: Some("Telegram DM".to_owned()),
                channel: "telegram".to_owned(),
                bot_token: Some("bot-token".to_owned()),
                chat_id: Some("chat-1".to_owned()),
                ..agent_control_plane::mobile::session::UpsertMobileNotificationRoute::default()
            },
        )
        .expect("upsert notification");
    service
        .upsert_completion_check(
            "check-cargo",
            "Cargo checks",
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
        .set_global_completion_check(Some("check-cargo"), true)
        .expect("set global check");
    service
        .set_session_notifications("thread-main", &["route-telegram".to_owned()])
        .expect("set session notification");
    service
        .set_session_completion_check("thread-main", Some("check-cargo"), false)
        .expect("set session check");
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(snapshot["notifications"][0]["id"], "route-telegram");
    assert_eq!(snapshot["notifications"][0]["label"], "Telegram DM");
    assert_eq!(snapshot["completionChecks"][0]["id"], "check-cargo");
    assert_eq!(snapshot["completionChecks"][0]["commandCount"], 2);
    assert_eq!(
        snapshot["globalSettings"]["notificationLabel"],
        "Telegram DM"
    );
    assert_eq!(
        snapshot["globalSettings"]["completionCheckLabel"],
        "Cargo checks"
    );
    assert_eq!(
        snapshot["globalSettings"]["completionCheckWaitForReply"],
        true
    );

    let detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(detail["notificationIds"][0], "route-telegram");
    assert_eq!(detail["completionCheckID"], "check-cargo");
    assert_eq!(detail["completionCheckWaitForReply"], false);
    assert_eq!(detail["availableNotifications"][0]["id"], "route-telegram");
    assert_eq!(detail["availableCompletionChecks"][0]["id"], "check-cargo");
}

#[tokio::test]
async fn mobile_session_detail_reads_latest_assistant_transcript_message() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main.jsonl",
        &[
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Initial assistant reply."
                        }
                    ]
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Latest assistant reply from transcript."
                        }
                    ]
                }
            }),
        ],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;

    assert_eq!(
        detail["latestAssistantMessage"],
        "Latest assistant reply from transcript."
    );
    assert_eq!(
        detail["assistantPreview"],
        "Latest assistant reply from transcript."
    );
    assert_eq!(detail["metadata"]["transcriptAvailable"], true);
    assert!(
        detail["metadata"]["sources"]
            .as_array()
            .expect("sources")
            .iter()
            .any(|source| source["kind"] == "transcript"
                && source["value"] == transcript_path.display().to_string())
    );
}

#[tokio::test]
async fn mobile_session_controls_are_owned_by_rust() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let settings_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/default-prompt",
        serde_json::json!({ "defaultPrompt": "Continue exactly from phone." }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        settings_snapshot["globalSettings"]["defaultPrompt"],
        "Continue exactly from phone."
    );
    assert_eq!(
        settings_snapshot["globalSettings"]["siriDefaultSessionId"],
        serde_json::Value::Null
    );
    assert_eq!(
        settings_snapshot["globalSettings"]["siriDefaultAssistantSurface"],
        serde_json::Value::Null
    );

    let mode_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/mode",
        serde_json::json!({ "preset": "max-turns-1" }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        mobile_snapshot_session(&mode_snapshot, "thread-main")["effectiveMode"],
        "max-turns-1"
    );
    assert_eq!(
        mobile_snapshot_session(&mode_snapshot, "thread-main")["status"],
        "stopped"
    );

    let detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(detail["effectiveMode"], "max-turns-1");
    assert_eq!(detail["status"], "stopped");

    control_plane
        .mobile_session_service()
        .record_hook_lifecycle(
            &MobileHookPayload {
                hook_event_name: "UserPromptSubmit".to_owned(),
                session_id: Some("thread-main".to_owned()),
                turn_id: None,
                cwd: None,
                last_assistant_message: None,
            },
            false,
        )
        .expect("record running lifecycle");
    let running_detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(running_detail["status"], "active");

    control_plane
        .mobile_session_service()
        .record_hook_lifecycle(
            &MobileHookPayload {
                hook_event_name: "Stop".to_owned(),
                session_id: Some("thread-main".to_owned()),
                turn_id: None,
                cwd: None,
                last_assistant_message: None,
            },
            false,
        )
        .expect("record stopped lifecycle");
    let stopped_detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(stopped_detail["status"], "stopped");

    let waiting_mode_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/mode",
        serde_json::json!({ "preset": "await-reply" }),
        &auth_headers,
        None,
    )
    .await;
    let waiting_session = mobile_snapshot_session(&waiting_mode_snapshot, "thread-main");
    assert_eq!(waiting_session["effectiveMode"], "await-reply");
    assert_eq!(waiting_session["status"], "waiting");

    let resumed_prompt_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/prompt",
        serde_json::json!({ "prompt": "Resume from phone." }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        mobile_snapshot_session(&resumed_prompt_snapshot, "thread-main")["id"],
        "thread-main"
    );
    assert!(
        control_plane
            .store()
            .mobile_events_since(0, 10)
            .expect("mobile events")
            .iter()
            .any(|event| event.detail.as_deref() == Some("prompt-resumed"))
    );

    record_thread_active(&control_plane, "thread-main");
    let prompt_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/prompt",
        serde_json::json!({ "prompt": "Keep going." }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        mobile_snapshot_session(&prompt_snapshot, "thread-main")["id"],
        "thread-main"
    );

    let mute_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/mute",
        serde_json::json!({}),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        mobile_snapshot_session(&mute_snapshot, "thread-main")["id"],
        "thread-main"
    );

    let siri_default_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-default-session",
        serde_json::json!({
            "sessionId": "thread-main",
            "assistantSurface": "codex"
        }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        siri_default_snapshot["globalSettings"]["siriDefaultSessionId"],
        "thread-main"
    );
    assert_eq!(
        siri_default_snapshot["globalSettings"]["siriDefaultAssistantSurface"],
        "codex"
    );
    assert_eq!(
        siri_default_snapshot["globalSettings"]["siriCurrentSessionId"],
        serde_json::Value::Null
    );
    assert_eq!(
        siri_default_snapshot["globalSettings"]["siriCurrentAssistantSurface"],
        serde_json::Value::Null
    );

    let mut json_headers = auth_headers.to_vec();
    json_headers.push((axum::http::header::CONTENT_TYPE, "application/json"));
    let missing_siri_default = request_with_body_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-default-session",
        serde_json::to_vec(&serde_json::json!({
            "sessionId": "missing-thread",
            "assistantSurface": "codex"
        }))
        .expect("json body"),
        &json_headers,
        None,
    )
    .await;
    assert_eq!(missing_siri_default.status(), StatusCode::NOT_FOUND);

    let invalid_siri_default_surface = request_with_body_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-default-session",
        serde_json::to_vec(&serde_json::json!({
            "sessionId": "thread-main",
            "assistantSurface": "wrong"
        }))
        .expect("json body"),
        &json_headers,
        None,
    )
    .await;
    assert_eq!(
        invalid_siri_default_surface.status(),
        StatusCode::BAD_REQUEST
    );

    let current_siri_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-current-session",
        serde_json::json!({
            "sessionId": "thread-main",
            "assistantSurface": "codex"
        }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        current_siri_snapshot["globalSettings"]["siriCurrentSessionId"],
        "thread-main"
    );
    assert_eq!(
        current_siri_snapshot["globalSettings"]["siriCurrentAssistantSurface"],
        "codex"
    );

    let missing_siri_current = request_with_body_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-current-session",
        serde_json::to_vec(&serde_json::json!({
            "sessionId": "missing-thread",
            "assistantSurface": "codex"
        }))
        .expect("json body"),
        &json_headers,
        None,
    )
    .await;
    assert_eq!(missing_siri_current.status(), StatusCode::NOT_FOUND);

    let invalid_siri_current_surface = request_with_body_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-current-session",
        serde_json::to_vec(&serde_json::json!({
            "sessionId": "thread-main",
            "assistantSurface": "wrong"
        }))
        .expect("json body"),
        &json_headers,
        None,
    )
    .await;
    assert_eq!(
        invalid_siri_current_surface.status(),
        StatusCode::BAD_REQUEST
    );

    let cleared_siri_current_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-current-session",
        serde_json::json!({
            "sessionId": null,
            "assistantSurface": null
        }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        cleared_siri_current_snapshot["globalSettings"]["siriCurrentSessionId"],
        serde_json::Value::Null
    );
    assert_eq!(
        cleared_siri_current_snapshot["globalSettings"]["siriCurrentAssistantSurface"],
        serde_json::Value::Null
    );

    let cleared_siri_default_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/siri-default-session",
        serde_json::json!({
            "sessionId": null,
            "assistantSurface": null
        }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        cleared_siri_default_snapshot["globalSettings"]["siriDefaultSessionId"],
        serde_json::Value::Null
    );
    assert_eq!(
        cleared_siri_default_snapshot["globalSettings"]["siriDefaultAssistantSurface"],
        serde_json::Value::Null
    );

    let archived_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/archive",
        serde_json::json!({ "archived": true }),
        &auth_headers,
        None,
    )
    .await;
    let archived_session = mobile_snapshot_session(&archived_snapshot, "thread-main");
    assert_eq!(archived_session["isArchived"], true);
    assert_eq!(archived_session["status"], "archived");

    let deleted_snapshot = request_json_with_options(
        &router,
        Method::DELETE,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert!(
        deleted_snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|session| session["id"] != "thread-main")
    );

    let missing_response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);

    let assistant_surface_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "devin" }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        assistant_surface_snapshot["globalSettings"]["assistantSurface"],
        "devin"
    );
    assert!(assistant_surface_snapshot["sessions"].as_array().is_some());

    let grok_surface_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "grok-build" }),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        grok_surface_snapshot["globalSettings"]["assistantSurface"],
        "grok-build"
    );
}

#[tokio::test]
async fn mobile_snapshot_filters_sessions_by_assistant_surface() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let grok_transcript = std::path::PathBuf::from("/Users/test/.grok/sessions/grok-thread.jsonl");
    fixture.attach_transcript_path("thread-main", &grok_transcript);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let codex_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert!(
        codex_snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|session| session["id"] != "thread-main")
    );

    let grok_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "grok-build" }),
        &auth_headers,
        None,
    )
    .await;
    let grok_session = mobile_snapshot_session(&grok_snapshot, "thread-main");
    assert_eq!(grok_session["assistantClient"], "grok-build");

    let hidden_detail = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-child",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(hidden_detail.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mobile_snapshot_includes_every_assistant_surface() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    fixture.write_grok_session("grok-session-1", "/tmp/project", "Ship Grok hooks");
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;

    assert_eq!(snapshot["globalSettings"]["assistantSurface"], "codex");
    let codex_session = mobile_snapshot_session(&snapshot, "thread-main");
    assert_eq!(codex_session["assistantClient"], "codex");

    let surface_sessions = snapshot["surfaceSessions"]
        .as_object()
        .expect("surface sessions");
    assert!(surface_sessions.contains_key("claude-code"));
    assert!(surface_sessions.contains_key("zed"));
    assert_surface_sessions_include(surface_sessions, "codex", "thread-main");
    assert_surface_sessions_include(surface_sessions, "devin", "devin:devin-cli:brindle-cadet");
    assert_surface_sessions_include(surface_sessions, "grok-build", "grok-session-1");
}

#[tokio::test]
async fn mobile_snapshot_marks_running_adapter_sessions_active() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_active_devin_next_session();
    fixture.write_grok_session("grok-session-1", "/tmp/project", "Ship Grok hooks");
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;

    assert_eq!(
        mobile_surface_session(&snapshot, "devin", "devin:devin-cli:brindle-cadet")["status"],
        "active"
    );
    assert_eq!(
        mobile_surface_session(&snapshot, "grok-build", "grok-session-1")["status"],
        "active"
    );
}

#[tokio::test]
async fn mobile_session_detail_accepts_selected_surface_override() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let hidden_detail = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/devin:devin-cli:brindle-cadet",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(hidden_detail.status(), StatusCode::NOT_FOUND);

    let visible_detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/devin:devin-cli:brindle-cadet?assistantSurface=devin",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(visible_detail["id"], "devin:devin-cli:brindle-cadet");
    assert_eq!(visible_detail["assistantClient"], "devin");
}

#[tokio::test]
async fn mobile_snapshot_uses_originator_for_vscode_source_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.set_thread_source("thread-main", "vscode");
    let codex_transcript = fixture.write_transcript(
        "thread-main-codex-originator.jsonl",
        &[serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": "thread-main",
                "originator": "Codex Desktop",
                "source": "vscode"
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &codex_transcript);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let codex_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let codex_session = mobile_snapshot_session(&codex_snapshot, "thread-main");
    assert_eq!(codex_session["assistantClient"], "codex");
    assert_eq!(codex_session["metadata"]["source"], "vscode");
    assert_eq!(codex_session["metadata"]["sourceDisplayName"], "Codex");
    assert!(
        !codex_session["metadata"]["tags"]
            .as_array()
            .expect("tags")
            .iter()
            .any(|tag| tag.as_str() == Some("vscode"))
    );

    request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "devin" }),
        &auth_headers,
        None,
    )
    .await;

    let hidden_devin_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert!(
        hidden_devin_snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|session| session["id"] != "thread-main")
    );

    let devin_transcript = fixture.write_transcript(
        "thread-main-devin-originator.jsonl",
        &[serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": "thread-main",
                "originator": "Devin - Next",
                "source": "vscode"
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &devin_transcript);

    let devin_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let devin_session = mobile_snapshot_session(&devin_snapshot, "thread-main");
    assert_eq!(devin_session["assistantClient"], "devin");
    assert_eq!(devin_session["metadata"]["source"], "vscode");
    assert_eq!(devin_session["metadata"]["sourceDisplayName"], "Devin");
}

#[tokio::test]
async fn mobile_snapshot_identifies_claude_originator_on_claude_surface() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.set_thread_source("thread-main", "vscode");
    let claude_transcript = fixture.write_transcript(
        "thread-main-claude-originator.jsonl",
        &[serde_json::json!({
            "type": "session_meta",
            "payload": {
                "id": "thread-main",
                "originator": "Claude Code",
                "source": "vscode"
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &claude_transcript);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let codex_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert!(
        codex_snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|session| session["id"] != "thread-main")
    );
    let claude_session = mobile_surface_session(&codex_snapshot, "claude-code", "thread-main");
    assert_eq!(claude_session["assistantClient"], "claude-code");
    assert_eq!(claude_session["metadata"]["source"], "vscode");
    assert_eq!(
        claude_session["metadata"]["sourceDisplayName"],
        "Claude Code"
    );

    let claude_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "claude-code" }),
        &auth_headers,
        None,
    )
    .await;
    let visible_claude_session = mobile_snapshot_session(&claude_snapshot, "thread-main");
    assert_eq!(visible_claude_session["assistantClient"], "claude-code");

    request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "devin" }),
        &auth_headers,
        None,
    )
    .await;
    let devin_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert!(
        devin_snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|session| session["id"] != "thread-main")
    );
}

#[tokio::test]
async fn desktop_and_mobile_snapshots_include_claude_code_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_claude_session(
        "claude-session-1",
        "/tmp/claude-project",
        "Build native Claude support",
        "Claude session is visible.",
    );
    let router = build_router(fixture.control_plane());

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    let claude_thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "claude:claude-session-1")
        .expect("claude thread");
    assert_eq!(claude_thread["source"], "claude-code");
    assert_eq!(claude_thread["originator"], "Claude Code");
    assert_eq!(
        claude_thread["capabilities"]["assistant_kind"],
        "claude-code"
    );
    assert_eq!(
        claude_thread["assistant_preview"],
        "Claude session is visible."
    );

    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];
    let mobile_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert!(
        mobile_snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .all(|session| session["id"] != "claude:claude-session-1")
    );
    let claude_session =
        mobile_surface_session(&mobile_snapshot, "claude-code", "claude:claude-session-1");
    assert_eq!(claude_session["assistantClient"], "claude-code");
    assert_eq!(
        claude_session["metadata"]["sourceDisplayName"],
        "Claude Code"
    );
    assert_eq!(
        claude_session["promptDeliveryUnavailableReason"],
        "This session must be running before Looper can queue prompts."
    );
}

#[tokio::test]
async fn desktop_snapshot_includes_grok_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_grok_session("grok-session-1", "/tmp/project", "Ship Grok hooks");
    let router = build_router(fixture.control_plane());

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(snapshot["grok_build"]["session_count"], 1);
    assert_eq!(snapshot["grok_build"]["active_session_count"], 1);
    let grok_thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "grok-session-1")
        .expect("grok session thread");
    assert_eq!(grok_thread["source"], "grok-build");
    assert_eq!(grok_thread["title"], "Ship Grok hooks");
}

#[tokio::test]
async fn desktop_snapshot_includes_devin_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let router = build_router(fixture.control_plane());

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(snapshot["thread_count"], 3);
    let devin_thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "devin:devin-cli:brindle-cadet")
        .expect("devin session thread");
    assert_eq!(devin_thread["source"], "devin-desktop");
    assert_eq!(devin_thread["originator"], "Devin - Next");
    assert_eq!(devin_thread["assistant_preview"], "Hello from Devin");
    assert_eq!(
        devin_thread["capabilities"]["assistant_kind"],
        "devin-desktop"
    );
    assert!(
        !serde_json::to_string(devin_thread)
            .expect("devin thread json")
            .contains("hidden thought")
    );

    let thread_detail = request_json(&router, "/threads/devin:devin-cli:brindle-cadet").await;
    assert_eq!(
        thread_detail["capabilities"]["assistant_kind"],
        "devin-desktop"
    );

    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];
    request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "devin" }),
        &auth_headers,
        None,
    )
    .await;
    let mobile_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let devin_session = mobile_snapshot_session(&mobile_snapshot, "devin:devin-cli:brindle-cadet");
    assert_eq!(devin_session["assistantClient"], "devin");
    assert_eq!(devin_session["assistantPreview"], "Hello from Devin");
}

#[tokio::test]
async fn mobile_devin_surface_survives_menu_snapshot_limit() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    fixture.append_newer_than_devin_state_threads(EXTRA_MOBILE_SNAPSHOT_THREADS);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let devin_snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "devin" }),
        &auth_headers,
        None,
    )
    .await;
    let devin_sessions = devin_snapshot["sessions"].as_array().expect("sessions");

    assert_eq!(devin_sessions.len(), 1);
    assert_eq!(devin_sessions[0]["id"], "devin:devin-cli:brindle-cadet");
    assert_eq!(devin_sessions[0]["assistantClient"], "devin");
}

#[tokio::test]
async fn mobile_snapshot_lists_native_grok_sessions_on_grok_surface() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_grok_session("grok-session-1", "/tmp/project", "Ship Grok hooks");
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "grok-build" }),
        &auth_headers,
        None,
    )
    .await;

    let grok_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let grok_session = mobile_snapshot_session(&grok_snapshot, "grok-session-1");
    assert_eq!(grok_session["assistantClient"], "grok-build");
    assert_eq!(grok_session["title"], "Ship Grok hooks");
    assert_eq!(grok_snapshot["grokBuild"]["sessionCount"], 1);
    assert_eq!(grok_snapshot["grokBuild"]["activeSessionCount"], 1);
    assert!(grok_snapshot["grokBuild"]["hooks"]["health"].is_string());
}

#[test]
fn mobile_event_payload_matches_ios_contract() {
    let event = build_mobile_event(MobileEventInput {
        kind: MobileEventKind::PromptQueued,
        thread_id: Some("thread-main".to_owned()),
        prompt_id: Some("prompt-123".to_owned()),
        detail: None,
    });
    let payload = serde_json::to_string(&event).expect("json");
    assert!(payload.contains("\"eventType\":\"prompt-queued\""));
    assert!(payload.contains("\"threadId\":\"thread-main\""));
    assert!(payload.contains("\"promptId\":\"prompt-123\""));
    assert_eq!(mobile_event_sse_name(event.event_type), "prompt.queued");
}

async fn wait_for_sse_buffer(body: &mut Body, buffer: &mut String, needle: &str, label: &str) {
    let deadline = tokio::time::sleep(std::time::Duration::from_secs(
        SSE_CONNECTED_EVENT_TIMEOUT_SECONDS,
    ));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            frame = body.frame() => {
                match frame {
                    Some(Ok(frame)) => {
                        if let Ok(chunk) = frame.into_data() {
                            buffer.push_str(&String::from_utf8_lossy(&chunk));
                            if buffer.contains(needle) {
                                break;
                            }
                        }
                    }
                    Some(Err(error)) => panic!("sse frame error: {error}"),
                    None => panic!("sse stream ended early waiting for {label}. buffer={buffer}"),
                }
            }
            _ = &mut deadline => {
                panic!("timed out waiting for {label}. buffer={buffer}");
            }
        }
    }
}

#[tokio::test]
async fn desktop_events_sse_streams_without_mobile_auth() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());

    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/desktop/events")
        .header(axum::http::header::ACCEPT, "text/event-stream")
        .body(Body::empty())
        .expect("request");
    let response = router.oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.contains("text/event-stream"));

    let mut body = response.into_body();
    let mut buffer = String::new();
    let connected_deadline = tokio::time::sleep(std::time::Duration::from_secs(
        SSE_CONNECTED_EVENT_TIMEOUT_SECONDS,
    ));
    tokio::pin!(connected_deadline);

    loop {
        tokio::select! {
            frame = body.frame() => {
                match frame {
                    Some(Ok(frame)) => {
                        if let Ok(chunk) = frame.into_data() {
                            buffer.push_str(&String::from_utf8_lossy(&chunk));
                            if buffer.contains("event: connected") {
                                break;
                            }
                        }
                    }
                    Some(Err(error)) => panic!("sse frame error: {error}"),
                    None => panic!("sse stream ended early: {buffer}"),
                }
            }
            _ = &mut connected_deadline => {
                panic!("timed out waiting for desktop SSE connection. buffer={buffer}");
            }
        }
    }
}

#[tokio::test]
async fn mobile_events_sse_streams_broadcast_prompt_resumed_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let service = control_plane.mobile_session_service();
    service
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set mode");
    record_thread_active(&control_plane, "thread-main");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/api/mobile/events")
        .header(axum::http::header::AUTHORIZATION, authorization.as_str())
        .header(axum::http::header::ACCEPT, "text/event-stream")
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.contains("text/event-stream"));

    let mut body = response.into_body();
    let mut buffer = String::new();
    let connected_deadline = tokio::time::sleep(std::time::Duration::from_secs(
        SSE_CONNECTED_EVENT_TIMEOUT_SECONDS,
    ));
    tokio::pin!(connected_deadline);

    loop {
        tokio::select! {
            frame = body.frame() => {
                match frame {
                    Some(Ok(frame)) => {
                        if let Ok(chunk) = frame.into_data() {
                            buffer.push_str(&String::from_utf8_lossy(&chunk));
                            if buffer.contains("event: connected")
                            {
                                break;
                            }
                        }
                    }
                    Some(Err(error)) => panic!("sse frame error: {error}"),
                    None => panic!("sse stream ended early: {buffer}"),
                }
            }
            _ = &mut connected_deadline => {
                panic!("timed out waiting for connected SSE event. buffer={buffer}");
            }
        }
    }

    let _snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/prompt",
        serde_json::json!({ "prompt": "Keep going from phone." }),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    let resumed_deadline = tokio::time::sleep(std::time::Duration::from_secs(
        SSE_CONNECTED_EVENT_TIMEOUT_SECONDS,
    ));
    tokio::pin!(resumed_deadline);

    loop {
        tokio::select! {
            frame = body.frame() => {
                match frame {
                    Some(Ok(frame)) => {
                        if let Ok(chunk) = frame.into_data() {
                            buffer.push_str(&String::from_utf8_lossy(&chunk));
                            if buffer.contains("event: session.changed")
                                && buffer.contains("\"detail\":\"prompt-resumed\"")
                            {
                                break;
                            }
                        }
                    }
                    Some(Err(error)) => panic!("sse frame error: {error}"),
                    None => panic!("sse stream ended early: {buffer}"),
                }
            }
            _ = &mut resumed_deadline => {
                panic!("timed out waiting for prompt-resumed SSE event. buffer={buffer}");
            }
        }
    }

    assert!(buffer.contains("event: connected"));
    assert!(buffer.contains("\"threadId\":\"thread-main\""));
}

#[tokio::test]
async fn mobile_events_sse_replays_same_millisecond_backfill_after_live_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    control_plane
        .store()
        .initialize()
        .expect("initialize events");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    let request = axum::http::Request::builder()
        .method(Method::GET)
        .uri("/api/mobile/events")
        .header(axum::http::header::AUTHORIZATION, authorization.as_str())
        .header(axum::http::header::ACCEPT, "text/event-stream")
        .body(Body::empty())
        .expect("request");
    let response = router.clone().oneshot(request).await.expect("response");
    assert_eq!(response.status(), StatusCode::OK);

    let mut body = response.into_body();
    let mut buffer = String::new();
    wait_for_sse_buffer(
        &mut body,
        &mut buffer,
        "event: connected",
        "connected SSE event",
    )
    .await;

    let connection = Connection::open(control_plane.store().path()).expect("open events");
    for (event_id, detail) in [
        ("event-a", "same-ms-live"),
        ("event-b", "same-ms-backfill-b"),
        ("event-c", "same-ms-backfill-c"),
    ] {
        connection
            .execute(
                "insert into mobile_event_log (
                    event_id, event_type, thread_id, prompt_id, detail, created_at_ms
                ) values (?1, 'session.changed', 'thread-main', null, ?2, 42)",
                rusqlite::params![event_id, detail],
            )
            .expect("insert mobile event");
    }
    control_plane
        .mobile_event_hub()
        .publish_persisted(MobileEventRecord {
            event_id: "event-a".to_owned(),
            event_type: MobileEventKind::SessionChanged,
            thread_id: Some("thread-main".to_owned()),
            prompt_id: None,
            detail: Some("same-ms-live".to_owned()),
            created_at_ms: 42,
        });

    wait_for_sse_buffer(
        &mut body,
        &mut buffer,
        "same-ms-backfill-c",
        "same millisecond backfill",
    )
    .await;

    assert!(buffer.contains("same-ms-live"));
    assert!(buffer.contains("same-ms-backfill-b"));
    assert!(buffer.contains("same-ms-backfill-c"));
}

#[tokio::test]
async fn mobile_events_endpoint_requires_mobile_auth() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());

    let response =
        request_with_options(&router, Method::GET, "/api/mobile/events", &[], None).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn grpc_desktop_events_streams_without_mobile_auth() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let (_server, mut client) = spawn_grpc_client(control_plane).await;

    let mut stream = client
        .subscribe_desktop_events(SubscribeEventsRequest {})
        .await
        .expect("desktop event stream")
        .into_inner();
    let event = stream
        .message()
        .await
        .expect("stream message")
        .expect("connected event");

    assert_eq!(event.event_name, "connected");
}

#[tokio::test]
async fn grpc_mobile_events_endpoint_requires_mobile_auth() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let (_server, mut client) = spawn_grpc_client(control_plane).await;

    let error = client
        .subscribe_mobile_events(SubscribeEventsRequest {})
        .await
        .expect_err("mobile gRPC event stream should require auth");

    assert_eq!(error.code(), tonic::Code::Unauthenticated);
}

#[tokio::test]
async fn grpc_mobile_prompt_records_prompt_resumed_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let service = control_plane.mobile_session_service();
    service
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set mode");
    record_thread_active(&control_plane, "thread-main");

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;

    let mut request = tonic::Request::new(SendSessionPromptRequest {
        thread_id: "thread-main".to_owned(),
        prompt: "Keep going from gRPC.".to_owned(),
        assistant_surface: String::new(),
    });
    request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );

    let response = client
        .send_session_prompt(request)
        .await
        .expect("send session prompt")
        .into_inner();

    assert!(response.accepted);
    assert_eq!(response.dispatch_kind, "resumed");

    let events = control_plane
        .store()
        .mobile_events_since(0, 32)
        .expect("mobile events");
    assert!(events.iter().any(|event| {
        event.thread_id.as_deref() == Some("thread-main")
            && event.event_type == MobileEventKind::SessionChanged
            && event.detail.as_deref() == Some("prompt-resumed")
    }));
}

#[tokio::test]
async fn codex_mobile_prompt_records_prompt_resumed_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let service = control_plane.mobile_session_service();
    service
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set mode");
    record_thread_active(&control_plane, "thread-main");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    let _snapshot = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/thread-main/prompt",
        serde_json::json!({ "prompt": "Keep going from phone." }),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    let events = control_plane
        .store()
        .mobile_events_since(0, 10)
        .expect("mobile events");
    assert!(
        events
            .iter()
            .any(|event| event.event_type == MobileEventKind::SessionChanged
                && event.detail.as_deref() == Some("prompt-resumed"))
    );
}

#[tokio::test]
async fn devin_mobile_prompt_queues_prompt_for_local_devin_hook_delivery() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
        .set_assistant_surface("devin")
        .expect("set Devin surface");
    control_plane
        .mobile_session_service()
        .set_session_preset("devin:devin-cli:brindle-cadet", Some("await-reply"))
        .expect("set Devin session mode");
    record_thread_active(&control_plane, "devin:devin-cli:brindle-cadet");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    let queued = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/devin:devin-cli:brindle-cadet/prompt",
        serde_json::json!({ "prompt": "Keep going from phone." }),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;
    assert!(
        queued["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["id"] == "devin:devin-cli:brindle-cadet")
    );

    let events = control_plane
        .store()
        .mobile_events_since(0, 10)
        .expect("mobile events");
    let queued_event = events
        .iter()
        .find(|event| {
            event.event_type == MobileEventKind::PromptQueued
                && event.thread_id.as_deref() == Some("devin:devin-cli:brindle-cadet")
        })
        .expect("prompt should queue");
    let queued_prompt_id = queued_event.prompt_id.as_deref().expect("queued prompt id");

    let outcome = control_plane
        .mobile_session_service()
        .hook_outcome_for_payload(&MobileHookPayload {
            hook_event_name: "Stop".to_owned(),
            session_id: Some("devin:devin-cli:brindle-cadet".to_owned()),
            turn_id: None,
            cwd: Some("/tmp/project".to_owned()),
            last_assistant_message: None,
        })
        .expect("Devin hook outcome");
    let decision = outcome.decision.expect("stop should deliver queued prompt");
    assert_eq!(decision.decision, "block");
    assert_eq!(decision.reason, "Keep going from phone.");
    assert_eq!(
        outcome.delivered_prompt_id.as_deref(),
        Some(queued_prompt_id)
    );
}

#[tokio::test]
async fn devin_mobile_prompt_rejects_stopped_local_devin_hook_delivery() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
        .set_assistant_surface("devin")
        .expect("set Devin surface");
    control_plane
        .mobile_session_service()
        .set_session_preset("devin:devin-cli:brindle-cadet", Some("await-reply"))
        .expect("set Devin session mode");
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;

    let body = serde_json::to_vec(&serde_json::json!({
        "prompt": "Keep going from phone."
    }))
    .expect("json body");
    let response = request_with_body_options(
        &router,
        Method::POST,
        "/api/mobile/sessions/devin:devin-cli:brindle-cadet/prompt",
        body,
        &[
            (axum::http::header::AUTHORIZATION, authorization.as_str()),
            (axum::http::header::CONTENT_TYPE, "application/json"),
        ],
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload["message"],
        "This Devin Local session must be running before Looper can deliver prompts through hooks."
    );
}

#[tokio::test]
async fn devin_acp_attach_route_is_not_supported() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_settings();
    let router = build_router(fixture.control_plane());
    let loopback = Some("127.0.0.1:49153".parse().expect("loopback socket"));

    let attach_response = request_with_body_options(
        &router,
        Method::POST,
        "/desktop/devin/acp-bridge/attach",
        serde_json::to_vec(&serde_json::json!({ "agentId": "codex" })).expect("json body"),
        &[(axum::http::header::CONTENT_TYPE, "application/json")],
        loopback,
    )
    .await;

    assert_eq!(attach_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn desktop_mobile_state_mutations_replace_renderer_rpc() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    let loopback = Some("127.0.0.1:49153".parse().expect("loopback socket"));

    let state = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/settings/default-prompt",
        serde_json::json!({ "defaultPrompt": "Continue from TUI." }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(state["defaultPrompt"], "Continue from TUI.");

    let scoped = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/settings/scope",
        serde_json::json!({ "scope": "per-task" }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(scoped["scope"], "per-task");

    let assistant_surface = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/settings/assistant-surface",
        serde_json::json!({ "assistantSurface": "devin" }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(assistant_surface["assistantSurface"], "devin");

    let routed = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/notifications",
        serde_json::json!({
            "id": "route-slack",
            "label": "Slack alerts",
            "channel": "slack",
            "webhookUrl": "https://hooks.slack.com/services/test"
        }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(routed["notifications"][0]["id"], "route-slack");

    let checked = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/completion-checks",
        serde_json::json!({
            "id": "check-test",
            "label": "Tests",
            "commands": ["cargo test"]
        }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(checked["completionChecks"][0]["id"], "check-test");

    let global = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/settings/global-completion-check",
        serde_json::json!({
            "completionCheckId": "check-test",
            "waitForReplyAfterCompletion": true
        }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(global["globalCompletionCheckId"], "check-test");
    assert_eq!(global["globalCompletionCheckWaitForReply"], true);

    let session = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/sessions/thread-main/notifications",
        serde_json::json!({ "notificationIds": ["route-slack"] }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(
        session["sessions"]["thread-main"]["notificationIds"][0],
        "route-slack"
    );

    let archived = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/sessions/thread-main/archive",
        serde_json::json!({ "archived": true }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(archived["sessions"]["thread-main"]["archived"], true);

    record_thread_active(&control_plane, "thread-child");
    let prompted = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/session-prompts",
        serde_json::json!({
            "threadIds": ["thread-child"],
            "preset": "max-turns-1",
            "prompt": "Continue from the cockpit."
        }),
        &[],
        loopback,
    )
    .await;
    assert_eq!(prompted["prompted"], 1);
    assert_eq!(prompted["threadIds"][0], "thread-child");
    assert_eq!(
        prompted["promptIds"].as_array().expect("prompt ids").len(),
        0
    );
    assert_eq!(prompted["resumedThreadIds"][0], "thread-child");

    let deleted_route = request_json_with_options(
        &router,
        Method::DELETE,
        "/desktop/notifications/route-slack",
        &[],
        loopback,
    )
    .await;
    assert!(
        deleted_route["notifications"]
            .as_array()
            .expect("notifications")
            .is_empty()
    );
}

#[tokio::test]
async fn mobile_push_registration_is_stored_in_rust() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;

    let registration = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/push/register",
        serde_json::json!({
            "installationId": "install-1",
            "deviceToken": "token-1",
            "bundleId": "dev.looper.app.ios",
            "environment": "development",
            "deviceName": "Test iPhone"
        }),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(registration["state"], "stored-awaiting-provider");
    assert_eq!(registration["environment"], "development");
    assert_ne!(registration["registeredAt"], "");

    let desktop_push_devices = request_json_with_options(
        &router,
        Method::GET,
        "/desktop/push/devices",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(
        desktop_push_devices["devices"][0]["installationId"],
        "install-1"
    );
    assert_eq!(
        desktop_push_devices["devices"][0]["state"],
        "stored-awaiting-provider"
    );
    assert_eq!(desktop_push_devices["devices"][0]["canTest"], false);

    let desktop_test_response = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/push/devices/install-1/test",
        serde_json::json!({}),
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(desktop_test_response["delivered"], false);
    assert_ne!(desktop_test_response["message"], "");

    let test_response = request_json_body_with_options(
        &router,
        Method::POST,
        "/api/mobile/push/test",
        serde_json::json!({ "installationId": "install-1" }),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(test_response["delivered"], false);
    assert_ne!(test_response["message"], "");
}

#[test]
fn auth_manager_keeps_secrets_out_of_cloud_contracts() {
    let secret_store = MemorySecretStore::default();
    let mut manager = AuthManager::new(secret_store);
    manager
        .register_device("device-local", "macos", "public-key-material")
        .expect("register device");
    manager
        .link_method(
            LinkedIdentityMethod::apple("apple-subject", Some("relay@example.com")),
            Some("refresh-token-secret"),
        )
        .expect("link apple");
    manager
        .link_method(LinkedIdentityMethod::passkey("credential-id"), None)
        .expect("link passkey");

    let contract = CloudAuthContract::from_manager(&manager, "account-local");
    let json = serde_json::to_string(&contract).expect("serialize contract");

    assert!(json.contains("relay@example.com"));
    assert!(json.contains("credential-id"));
    assert!(!json.contains("refresh-token-secret"));
    assert!(!json.contains("public-key-material"));
}

#[test]
fn assistant_adapters_detect_gui_and_cli_surfaces() {
    let adapters = discover_assistant_adapters_from_processes(&[
        "/Applications/Devin - Next.app/Contents/MacOS/Devin - Next --type=main".to_owned(),
        "/Applications/Superconductor.app/Contents/MacOS/Superconductor --host".to_owned(),
        "/Applications/Cursor.app/Contents/MacOS/Cursor --type=renderer".to_owned(),
        "/Applications/Claude.app/Contents/MacOS/Claude --type=renderer com.anthropic.claudefordesktop".to_owned(),
        "/opt/homebrew/bin/claude --print test".to_owned(),
        "/Applications/Zed.app/Contents/MacOS/zed --foreground".to_owned(),
        "/opt/homebrew/bin/opencode run --json".to_owned(),
        "/Users/test/.grok/bin/grok agent stdio --model grok-build".to_owned(),
    ]);

    let devin = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::DevinDesktop)
        .expect("devin adapter");
    assert!(devin.live_sessions);
    assert!(
        devin
            .runtimes
            .iter()
            .any(|runtime| runtime.kind == AssistantRuntimeKind::Gui && runtime.running)
    );

    let superconductor = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::Superconductor)
        .expect("superconductor adapter");
    assert!(
        superconductor
            .runtimes
            .iter()
            .any(|runtime| runtime.kind == AssistantRuntimeKind::Gui && runtime.running)
    );

    let cursor = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::Cursor)
        .expect("cursor adapter");
    assert!(
        cursor
            .runtimes
            .iter()
            .any(|runtime| runtime.kind == AssistantRuntimeKind::Gui && runtime.running)
    );

    let claude = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::ClaudeCode)
        .expect("claude adapter");
    assert!(claude.live_sessions);
    assert!(
        claude
            .runtimes
            .iter()
            .any(|runtime| runtime.kind == AssistantRuntimeKind::Gui && runtime.running)
    );
    assert!(
        claude
            .runtimes
            .iter()
            .any(|runtime| runtime.label == "Claude Code CLI"
                && runtime.kind == AssistantRuntimeKind::Cli
                && runtime.running)
    );

    let opencode = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::OpenCode)
        .expect("opencode adapter");
    assert!(
        opencode
            .runtimes
            .iter()
            .any(|runtime| runtime.kind == AssistantRuntimeKind::Cli && runtime.running)
    );

    let grok_build = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::GrokBuild)
        .expect("grok build adapter");
    assert!(
        grok_build
            .runtimes
            .iter()
            .any(|runtime| runtime.label == "Grok Build session" && runtime.running)
    );

    let zed = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::Zed)
        .expect("zed adapter");
    assert!(
        zed.runtimes
            .iter()
            .any(|runtime| runtime.kind == AssistantRuntimeKind::Gui && runtime.running)
    );
}

#[test]
fn assistant_adapters_separate_cli_installed_from_running() {
    let mut cli_paths = BTreeMap::new();
    cli_paths.insert(
        "cursor".to_owned(),
        "/Users/test/.local/bin/cursor".to_owned(),
    );
    cli_paths.insert(
        "devin-desktop-next".to_owned(),
        "/usr/local/bin/devin-desktop-next".to_owned(),
    );
    cli_paths.insert(
        "opencode".to_owned(),
        "/Users/test/.opencode/bin/opencode".to_owned(),
    );
    cli_paths.insert("grok".to_owned(), "/Users/test/.grok/bin/grok".to_owned());
    cli_paths.insert("zed".to_owned(), "/usr/local/bin/zed".to_owned());

    let adapters = discover_assistant_adapters_from_sources(
        &[
            "/Applications/Cursor.app/Contents/MacOS/Cursor".to_owned(),
            "Cursor Helper: terminal pty-host".to_owned(),
        ],
        &cli_paths,
    );

    let cursor = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::Cursor)
        .expect("cursor adapter");
    let cursor_cli = cursor
        .runtimes
        .iter()
        .find(|runtime| runtime.kind == AssistantRuntimeKind::Cli)
        .expect("cursor cli");
    assert!(cursor_cli.installed);
    assert!(!cursor_cli.running);

    let devin = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::DevinDesktop)
        .expect("devin adapter");
    let devin_cli = devin
        .runtimes
        .iter()
        .find(|runtime| runtime.kind == AssistantRuntimeKind::Cli)
        .expect("devin cli");
    assert!(devin_cli.installed);
    assert!(!devin_cli.running);

    let opencode = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::OpenCode)
        .expect("opencode adapter");
    let opencode_cli = opencode
        .runtimes
        .iter()
        .find(|runtime| runtime.kind == AssistantRuntimeKind::Cli)
        .expect("opencode cli");
    assert!(opencode_cli.installed);
    assert!(!opencode_cli.running);

    let grok_build = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::GrokBuild)
        .expect("grok build adapter");
    let grok_cli = grok_build
        .runtimes
        .iter()
        .find(|runtime| runtime.label == "Grok Build CLI")
        .expect("grok cli");
    assert!(grok_cli.installed);
    assert!(!grok_cli.running);

    let zed = adapters
        .iter()
        .find(|adapter| adapter.assistant_kind == AssistantKind::Zed)
        .expect("zed adapter");
    let zed_cli = zed
        .runtimes
        .iter()
        .find(|runtime| runtime.label == "Zed CLI")
        .expect("zed cli");
    assert!(zed_cli.installed);
    assert!(!zed_cli.running);
}

fn mobile_snapshot_session<'a>(
    snapshot: &'a serde_json::Value,
    session_id: &str,
) -> &'a serde_json::Value {
    snapshot["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["id"] == session_id)
        .expect("session")
}

fn mobile_surface_session<'a>(
    snapshot: &'a serde_json::Value,
    surface: &str,
    session_id: &str,
) -> &'a serde_json::Value {
    snapshot["surfaceSessions"][surface]
        .as_array()
        .expect("surface sessions")
        .iter()
        .find(|session| session["id"] == session_id)
        .expect("surface session")
}

fn assert_surface_sessions_include(
    surface_sessions: &serde_json::Map<String, serde_json::Value>,
    surface: &str,
    session_id: &str,
) {
    assert!(
        surface_sessions[surface]
            .as_array()
            .expect("surface session list")
            .iter()
            .any(|session| session["id"] == session_id),
        "expected {surface} sessions to include {session_id}"
    );
}

async fn request_json(router: &axum::Router, path: &str) -> serde_json::Value {
    request_json_with_method(router, Method::GET, path).await
}

async fn request_json_with_method(
    router: &axum::Router,
    method: Method,
    path: &str,
) -> serde_json::Value {
    request_json_with_options(
        router,
        method,
        path,
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await
}

fn record_thread_active(control_plane: &ControlPlane, thread_id: &str) {
    control_plane
        .mobile_session_service()
        .record_hook_lifecycle(
            &MobileHookPayload {
                hook_event_name: "UserPromptSubmit".to_owned(),
                session_id: Some(thread_id.to_owned()),
                turn_id: None,
                cwd: None,
                last_assistant_message: None,
            },
            false,
        )
        .expect("record active mobile lifecycle");
}

async fn request_json_with_options(
    router: &axum::Router,
    method: Method,
    path: &str,
    headers: &[(HeaderName, &str)],
    remote_address: Option<std::net::SocketAddr>,
) -> serde_json::Value {
    let response = request_with_options(router, method, path, headers, remote_address).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

async fn request_json_body_with_options(
    router: &axum::Router,
    method: Method,
    path: &str,
    body: serde_json::Value,
    headers: &[(HeaderName, &str)],
    remote_address: Option<std::net::SocketAddr>,
) -> serde_json::Value {
    let body = serde_json::to_vec(&body).expect("json body");
    let mut json_headers = headers.to_vec();
    json_headers.push((axum::http::header::CONTENT_TYPE, "application/json"));
    let response =
        request_with_body_options(router, method, path, body, &json_headers, remote_address).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
}

async fn request_with_options(
    router: &axum::Router,
    method: Method,
    path: &str,
    headers: &[(HeaderName, &str)],
    remote_address: Option<std::net::SocketAddr>,
) -> axum::response::Response {
    request_with_body_options(router, method, path, Vec::new(), headers, remote_address).await
}

async fn request_with_body_options(
    router: &axum::Router,
    method: Method,
    path: &str,
    body: Vec<u8>,
    headers: &[(HeaderName, &str)],
    remote_address: Option<std::net::SocketAddr>,
) -> axum::response::Response {
    let mut builder = axum::http::Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(name, HeaderValue::from_str(value).expect("header value"));
    }
    let mut request = builder.body(Body::from(body)).expect("request");
    if let Some(remote_address) = remote_address {
        request.extensions_mut().insert(ConnectInfo(remote_address));
    }

    router.clone().oneshot(request).await.expect("response")
}

async fn issue_mobile_authorization_header(router: &axum::Router) -> String {
    let connection_code = request_json_with_options(
        router,
        Method::GET,
        "/api/mobile/connection-code",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    let token_id = connection_code["pairingTokenId"]
        .as_str()
        .expect("pairing token id");
    let token = connection_code["pairingToken"]
        .as_str()
        .expect("pairing token");
    format!("Bearer {token_id}.{token}")
}

async fn spawn_grpc_client(
    control_plane: ControlPlane,
) -> (
    tokio::task::JoinHandle<()>,
    LooperRealtimeClient<tonic::transport::Channel>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gRPC test listener");
    let address = listener.local_addr().expect("gRPC listener address");
    let server = tokio::spawn(async move {
        agent_control_plane::grpc::serve_with_listener(
            control_plane,
            listener,
            std::future::pending(),
        )
        .await
        .expect("gRPC server");
    });
    let endpoint = format!("http://{address}");
    let client = LooperRealtimeClient::connect(endpoint)
        .await
        .expect("connect gRPC client");
    (server, client)
}

struct IsolatedCodexFixture {
    temp_dir: TempDir,
    codex_home: std::path::PathBuf,
}

impl IsolatedCodexFixture {
    fn new() -> Self {
        let temp_dir = TempDir::new().expect("temp dir");
        let codex_home = temp_dir.path().join(".codex");
        fs::create_dir_all(codex_home.join("sessions")).expect("codex dirs");
        fs::create_dir_all(temp_dir.path().join(".grok/sessions")).expect("grok dirs");
        Self {
            temp_dir,
            codex_home,
        }
    }

    fn grok_home(&self) -> std::path::PathBuf {
        self.temp_dir.path().join(".grok")
    }

    fn codex_resume_stub(&self) -> std::path::PathBuf {
        let executable = self.temp_dir.path().join("codex-resume-stub");
        if executable.is_file() {
            return executable;
        }
        fs::write(
            &executable,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"id":"looper-initialize"'*)
      printf '%s\n' '{"id":"looper-initialize","result":{}}'
      ;;
    *'"id":"looper-thread-resume"'*)
      printf '%s\n' '{"id":"looper-thread-resume","result":{"thread":{"id":"thread-stub"}}}'
      ;;
    *'"id":"looper-turn-start"'*)
      thread_id=$(printf '%s\n' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
      if [ -z "$thread_id" ]; then
        thread_id="thread-stub"
      fi
      printf '%s\n' '{"id":"looper-turn-start","result":{"turn":{"id":"turn-stub","status":"inProgress"}}}'
      printf '%s\n' '{"method":"turn/completed","params":{"threadId":"'"$thread_id"'","turn":{"id":"turn-stub","status":"completed"}}}'
      ;;
  esac
done
"#,
        )
        .expect("write codex resume stub");
        let mut permissions = fs::metadata(&executable)
            .expect("codex resume stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("chmod codex resume stub");
        executable
    }

    fn control_plane(&self) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: self.codex_home.clone(),
            codex_executable: Some(self.codex_resume_stub().display().to_string()),
            grok_home: self.grok_home(),
            store_path: self.temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
            home_path: self.temp_dir.path().to_path_buf(),
        })
    }

    fn write_devin_next_settings(&self) {
        let settings_path = self
            .temp_dir
            .path()
            .join("Library/Application Support/Devin - Next/User/settings.json");
        fs::create_dir_all(settings_path.parent().expect("settings parent"))
            .expect("create settings parent");
        fs::write(
            settings_path,
            serde_json::json!({
                "devin.acp.enabled": true,
                "devin.acp.preferredAgent": "codex",
                "devin.acp.enabledAgents": {
                    "codex": true,
                    "devin-cli": true,
                    "disabled-agent": false
                },
                "devin.acp.agentEnv": {
                    "TOKEN": "must-not-leak"
                }
            })
            .to_string(),
        )
        .expect("write devin settings");
        let registry_path = self.temp_dir.path().join(".devin-next/acp/registry.json");
        fs::create_dir_all(registry_path.parent().expect("registry parent"))
            .expect("create registry parent");
        fs::write(
            registry_path,
            serde_json::json!({
                "version": "1.0.0",
                "agents": [
                    {
                        "id": "codex",
                        "name": "Codex",
                        "version": "0.0.44",
                        "distribution": {
                            "npx": {
                                "package": "@agentclientprotocol/codex-acp"
                            }
                        }
                    }
                ]
            })
            .to_string(),
        )
        .expect("write devin registry");
    }

    fn write_zed_settings(&self) {
        let settings_path = self.temp_dir.path().join(".zed/settings.json");
        fs::create_dir_all(settings_path.parent().expect("zed settings parent"))
            .expect("create zed settings parent");
        fs::write(
            settings_path,
            serde_json::json!({
                "agent_servers": {
                    "looper": {
                        "type": "custom",
                        "command": "looper",
                        "args": ["acp", "stdio"],
                        "env": {
                            "TOKEN": "zed-secret-token"
                        }
                    }
                }
            })
            .to_string(),
        )
        .expect("write zed settings");
    }

    fn write_devin_next_session(&self) {
        self.write_devin_next_session_with_status("end_turn");
    }

    fn write_active_devin_next_session(&self) {
        self.write_devin_next_session_with_status("running");
    }

    fn write_devin_next_session_with_status(&self, status: &str) {
        let app_support = self
            .temp_dir
            .path()
            .join("Library/Application Support/Devin - Next");
        let state_db_path = app_support
            .join("User")
            .join("globalStorage")
            .join("state.vscdb");
        fs::create_dir_all(state_db_path.parent().expect("state parent"))
            .expect("create devin state parent");
        let events_path = app_support.join("User").join("acp-events");
        fs::create_dir_all(&events_path).expect("create devin events path");
        fs::write(
            events_path.join("event-1.ndjson"),
            [
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "agent_thought_chunk",
                        "content": {
                            "type": "text",
                            "text": "hidden thought"
                        },
                        "_meta": {
                            "cognition.ai/streamingMessageId": "thought-1"
                        }
                    }
                })
                .to_string(),
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {
                            "type": "text",
                            "text": "Hello "
                        },
                        "_meta": {
                            "cognition.ai/streamingMessageId": "assistant-1"
                        }
                    }
                })
                .to_string(),
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "agent_message_chunk",
                        "content": {
                            "type": "text",
                            "text": "from Devin"
                        },
                        "_meta": {
                            "cognition.ai/streamingMessageId": "assistant-1"
                        }
                    }
                })
                .to_string(),
            ]
            .join("\n"),
        )
        .expect("write devin event log");
        let connection = Connection::open(&state_db_path).expect("devin state");
        connection
            .execute("create table ItemTable (key text, value blob)", [])
            .expect("create devin item table");
        connection
            .execute(
                "insert into ItemTable (key, value) values (?1, ?2)",
                (
                    "windsurf.acp.metadataCache",
                    serde_json::to_vec(&serde_json::json!({
                        "sessions": [
                            {
                                "sessionId": "acp/devin-cli/brindle-cadet",
                                "providerId": "devin-cli",
                                "title": "Devin task",
                                "cwd": "/tmp/devin-project",
                                "status": status,
                                "updatedAt": "2026-06-07T03:10:03+00:00",
                                "_meta": {
                                    "cognition.ai/createdAt": "2026-06-07T03:09:52.477Z",
                                    "cognition.ai/isArchived": false
                                }
                            }
                        ]
                    }))
                    .expect("metadata json"),
                ),
            )
            .expect("insert devin metadata");
        connection
            .execute(
                "insert into ItemTable (key, value) values (?1, ?2)",
                (
                    "windsurf.acp.eventLog.index",
                    serde_json::to_vec(&serde_json::json!({
                        "acp/devin-cli/brindle-cadet": {
                            "uuid": "event-1",
                            "eventCount": 3,
                            "lastUpdated": DEVIN_FIXTURE_EVENT_UPDATED_AT_MS
                        }
                    }))
                    .expect("event index json"),
                ),
            )
            .expect("insert devin event index");
    }

    fn write_hooks_json(&self, command: &str) {
        let hooks = serde_json::json!({
            "SessionStart": [{"command": command}],
            "Stop": [{"command": command}],
            "UserPromptSubmit": [{"command": command}]
        });
        fs::write(
            self.codex_home.join("hooks.json"),
            serde_json::to_vec_pretty(&hooks).expect("hooks"),
        )
        .expect("write hooks");
    }

    fn write_nested_hooks_json(&self, command: &str) {
        let hooks = serde_json::json!({
            "hooks": {
                "SessionStart": [{"command": command}],
                "Stop": [{"command": command}],
                "UserPromptSubmit": [{"command": command}]
            }
        });
        fs::write(
            self.codex_home.join("hooks.json"),
            serde_json::to_vec_pretty(&hooks).expect("hooks"),
        )
        .expect("write hooks");
    }

    fn write_mixed_hooks_json(&self) {
        let hooks = serde_json::json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "startup|resume",
                        "hooks": [
                            {
                                "type": "command",
                                "command": "'/tmp/agent-control-plane' --hook --managed-by looper",
                                "timeout": 30
                            },
                            {
                                "type": "command",
                                "command": "bun legacy/bun/managed-hook-script.ts",
                                "timeout": 30
                            }
                        ]
                    }
                ],
                "Stop": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "'/tmp/agent-control-plane' --hook --managed-by looper",
                                "timeout": 86400
                            },
                            {
                                "type": "command",
                                "command": "/usr/local/bin/custom-user-hook",
                                "timeout": 30
                            }
                        ]
                    }
                ],
                "UserPromptSubmit": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "'/tmp/agent-control-plane' --hook --managed-by looper",
                                "timeout": 30
                            }
                        ]
                    }
                ]
            }
        });
        fs::write(
            self.codex_home.join("hooks.json"),
            serde_json::to_vec_pretty(&hooks).expect("hooks"),
        )
        .expect("write hooks");
    }

    fn write_user_hooks_json(&self) {
        let hooks = serde_json::json!({
            "hooks": {
                "Stop": [
                    {
                        "hooks": [
                            {
                                "type": "command",
                                "command": "/usr/local/bin/custom-user-hook",
                                "timeout": 30
                            }
                        ]
                    }
                ]
            }
        });
        fs::write(
            self.codex_home.join("hooks.json"),
            serde_json::to_vec_pretty(&hooks).expect("hooks"),
        )
        .expect("write hooks");
    }

    fn write_config_toml(&self, enabled: bool) {
        fs::write(
            self.codex_home.join("config.toml"),
            format!("[features]\nhooks = {enabled}\n"),
        )
        .expect("write config");
    }

    fn write_config_toml_with_model_block(&self) {
        fs::write(
            self.codex_home.join("config.toml"),
            "[model]\ndefault = \"gpt-5.5\"\n\n[features]\ncodex_hooks = false\n",
        )
        .expect("write config");
    }

    fn write_config_toml_with_disabled_owned_hook_state(&self) {
        fs::write(
            self.codex_home.join("config.toml"),
            format!(
                r#"[features]
hooks = true

[hooks.state."{}:user_prompt_submit:0:0"]
enabled = false
trusted_hash = "sha256:owned"

[hooks.state."{}:user_prompt_submit:1:0"]
enabled = false
trusted_hash = "sha256:user"
"#,
                self.codex_home.join("hooks.json").display(),
                self.codex_home.join("hooks.json").display(),
            ),
        )
        .expect("write config");
    }

    fn write_grok_session(&self, session_id: &str, cwd: &str, title: &str) {
        let session_dir = self
            .grok_home()
            .join("sessions")
            .join("%2Ftmp%2Fproject")
            .join(session_id);
        fs::create_dir_all(&session_dir).expect("create grok session dir");
        fs::write(
            session_dir.join("summary.json"),
            serde_json::json!({
                "info": {
                    "id": session_id,
                    "cwd": cwd
                },
                "session_summary": title,
                "generated_title": title,
                "updated_at": "2026-06-06T12:00:00Z"
            })
            .to_string(),
        )
        .expect("write grok summary");
        fs::write(session_dir.join("updates.jsonl"), "{}\n").expect("write grok updates");
        fs::write(
            self.grok_home().join("active_sessions.json"),
            serde_json::json!([{
                "session_id": session_id,
                "cwd": cwd
            }])
            .to_string(),
        )
        .expect("write active sessions");
    }

    fn write_claude_session(
        &self,
        session_id: &str,
        cwd: &str,
        user_message: &str,
        assistant_message: &str,
    ) {
        let project_dir = self
            .temp_dir
            .path()
            .join(".claude")
            .join("projects")
            .join("-tmp-claude-project");
        fs::create_dir_all(&project_dir).expect("claude project dir");
        let content = format!(
            r#"{{"type":"user","sessionId":"{session_id}","cwd":"{cwd}","timestamp":"2026-06-08T10:00:00.000Z","message":{{"role":"user","content":"{user_message}"}}}}
{{"type":"assistant","sessionId":"{session_id}","cwd":"{cwd}","timestamp":"2026-06-08T10:00:01.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"{assistant_message}"}}]}}}}
"#
        );
        fs::write(project_dir.join(format!("{session_id}.jsonl")), content)
            .expect("write claude session");
    }

    fn write_state_db(&self) {
        let connection = Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
        connection
            .execute_batch(
                r#"
create table threads (
  thread_id text primary key,
  title text,
  cwd text,
  source text,
  model text,
  reasoning_effort text,
  created_at_ms integer,
  updated_at_ms integer,
  archived integer
);
insert into threads values
  ('thread-main', 'Main task', '/tmp/project', 'desktop', 'gpt-5.5', 'high', 1000, 2000, 0),
  ('thread-child', 'Child task', '/tmp/project', 'subagent', 'gpt-5.5', 'medium', 1100, 2100, 0);

create table thread_spawn_edges (
  parent_thread_id text,
  child_thread_id text,
  status text
);
insert into thread_spawn_edges values ('thread-main', 'thread-child', 'running');

create table thread_dynamic_tools (
  thread_id text,
  position integer,
  name text,
  namespace text,
  description text,
  defer_loading integer
);
insert into thread_dynamic_tools values
  ('thread-main', 0, 'automation_update', 'functions', 'Manage app automations', 0),
  ('thread-main', 1, '_search_vercel_documentation', 'mcp__codex_apps__vercel', 'Search Vercel docs', 1);
"#,
            )
            .expect("seed state");
        Connection::open(self.codex_home.join("logs_1.sqlite")).expect("logs");
    }

    fn append_state_threads(&self, count: usize) {
        self.append_state_threads_with_base(count, EXTRA_THREAD_BASE_TIMESTAMP_MS);
    }

    fn append_newer_than_devin_state_threads(&self, count: usize) {
        self.append_state_threads_with_base(count, NEWER_THAN_DEVIN_THREAD_BASE_TIMESTAMP_MS);
    }

    fn append_state_threads_with_base(&self, count: usize, base_timestamp_ms: i64) {
        let mut connection =
            Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
        let transaction = connection.transaction().expect("state transaction");
        for index in 0..count {
            let thread_id = format!("thread-extra-{index:02}");
            let title = format!("Extra task {index}");
            let timestamp =
                base_timestamp_ms + i64::try_from(index).expect("thread index fits timestamp");
            transaction
                .execute(
                    r#"
insert into threads (
  thread_id,
  title,
  cwd,
  source,
  model,
  reasoning_effort,
  created_at_ms,
  updated_at_ms,
  archived
) values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)
"#,
                    rusqlite::params![
                        thread_id,
                        title,
                        "/tmp/project",
                        "desktop",
                        "gpt-5.5",
                        "medium",
                        timestamp,
                        timestamp,
                    ],
                )
                .expect("insert extra thread");
        }
        transaction.commit().expect("commit extra threads");
    }

    fn write_transcript(
        &self,
        file_name: &str,
        records: &[serde_json::Value],
    ) -> std::path::PathBuf {
        let transcript_path = self.codex_home.join("sessions").join(file_name);
        let contents = records
            .iter()
            .map(|record| serde_json::to_string(record).expect("record json"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&transcript_path, contents).expect("write transcript");
        transcript_path
    }

    fn attach_transcript_path(&self, thread_id: &str, transcript_path: &std::path::Path) {
        let connection = Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
        let has_rollout_path = connection
            .query_row(
                "select exists(select 1 from pragma_table_info('threads') where name = 'rollout_path')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .expect("check rollout path column");
        if !has_rollout_path {
            connection
                .execute("alter table threads add column rollout_path text", [])
                .expect("add rollout path");
        }
        connection
            .execute(
                "update threads set rollout_path = ?1 where thread_id = ?2",
                rusqlite::params![transcript_path.display().to_string(), thread_id],
            )
            .expect("attach transcript");
    }

    fn set_thread_source(&self, thread_id: &str, source: &str) {
        let connection = Connection::open(self.codex_home.join("state_1.sqlite")).expect("state");
        connection
            .execute(
                "update threads set source = ?1 where thread_id = ?2",
                rusqlite::params![source, thread_id],
            )
            .expect("set thread source");
    }

    fn write_live_shape_state_db(&self) {
        let connection = Connection::open(self.codex_home.join("state_9.sqlite")).expect("state");
        connection
            .execute_batch(
                r#"
create table threads (
  id text primary key,
  rollout_path text not null,
  created_at integer not null,
  updated_at integer not null,
  source text not null,
  model_provider text not null,
  cwd text not null,
  title text not null,
  sandbox_policy text not null,
  approval_mode text not null,
  tokens_used integer not null default 0,
  has_user_event integer not null default 0,
  archived integer not null default 0,
  archived_at integer,
  git_sha text,
  git_branch text,
  git_origin_url text,
  cli_version text not null default '',
  first_user_message text not null default '',
  agent_nickname text,
  agent_role text,
  memory_mode text not null default 'enabled',
  model text,
  reasoning_effort text,
  agent_path text,
  created_at_ms integer,
  updated_at_ms integer
);
insert into threads (
  id, rollout_path, created_at, updated_at, source, model_provider, cwd, title,
  sandbox_policy, approval_mode, git_sha, git_branch, cli_version, agent_nickname,
  agent_role, model, reasoning_effort, agent_path, created_at_ms, updated_at_ms
) values (
  'live-thread', '/tmp/rollout.jsonl', 1, 2, 'desktop', 'openai', '/tmp/project',
  'Live thread', 'workspace-write', 'on-request', 'abc123', 'main', '0.124.0',
  'Hubble', 'explorer', 'gpt-5.5', 'high', '/agents/explorer', 1000, 2000
);
create table thread_dynamic_tools (
  thread_id text not null,
  position integer not null,
  name text not null,
  description text not null,
  input_schema text not null,
  defer_loading integer not null default 0,
  namespace text
);
insert into thread_dynamic_tools values (
  'live-thread', 0, '_fetch', 'Fetch Notion', '{}', 1, 'mcp__codex_apps__notion'
);
create table thread_spawn_edges (
  parent_thread_id text not null,
  child_thread_id text not null primary key,
  status text not null
);
"#,
            )
            .expect("seed live state");
        Connection::open(self.codex_home.join("logs_9.sqlite")).expect("logs");
    }

    fn write_automation(&self, id: &str, content: &str) {
        let directory = self.codex_home.join("automations").join(id);
        fs::create_dir_all(&directory).expect("automation dir");
        fs::write(directory.join("automation.toml"), content.trim()).expect("automation toml");
    }

    fn write_goal(&self, id: &str, content: &str) {
        let directory = self.codex_home.join("goals").join(id);
        fs::create_dir_all(&directory).expect("goal dir");
        fs::write(directory.join("goal.toml"), content.trim()).expect("goal toml");
    }

    fn write_thread_goal(&self, thread_id: &str, goal_id: &str, objective: &str, status: &str) {
        let connection = Connection::open(self.codex_home.join("goals_1.sqlite")).expect("goals");
        connection
            .execute_batch(
                r#"
create table thread_goals (
    thread_id text primary key not null,
    goal_id text not null,
    objective text not null,
    status text not null,
    token_budget integer,
    tokens_used integer not null default 0,
    time_used_seconds integer not null default 0,
    created_at_ms integer not null,
    updated_at_ms integer not null
);
"#,
            )
            .expect("create thread goals");
        connection
            .execute(
                "insert into thread_goals values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    thread_id,
                    goal_id,
                    objective,
                    status,
                    GOAL_FIXTURE_TOKEN_BUDGET,
                    GOAL_FIXTURE_TOKENS_USED,
                    GOAL_FIXTURE_TIME_USED_SECONDS,
                    GOAL_FIXTURE_CREATED_AT_MS,
                    GOAL_FIXTURE_UPDATED_AT_MS
                ],
            )
            .expect("insert thread goal");
    }

    fn write_rollout_with_compaction(&self, thread_id: &str, compacted_at: &str) {
        let directory = self
            .codex_home
            .join("sessions")
            .join("2026")
            .join("04")
            .join("30");
        fs::create_dir_all(&directory).expect("session dir");
        let content = format!(
            r#"{{"timestamp":"2026-04-30T11:40:00.000Z","type":"session_meta","payload":{{"id":"{thread_id}"}}}}
{{"timestamp":"{compacted_at}","type":"event_msg","payload":{{"type":"context_compacted"}}}}
"#
        );
        fs::write(
            directory.join(format!("rollout-2026-04-30T11-40-00-{thread_id}.jsonl")),
            content,
        )
        .expect("rollout");
    }
}

// allow: SIZE_OK — integration-test harness root owns shared fixtures while scenario groups are split under tests/isolated_control_plane/.
use std::collections::{BTreeMap, BTreeSet};
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
use agent_control_plane::events::MobileSessionMiniProjectionInput;
use agent_control_plane::grpc::proto::{
    ClientFrame, Command, DeleteSessionRequest, HealthRequest, MuteSessionRequest, Resume,
    SaveDefaultPromptRequest, SendSessionPromptRequest, ServerFrame, SetAssistantSurfaceRequest,
    SetDefaultNotificationTargetsRequest, SetGlobalCompletionCheckRequest, SetScopeRequest,
    SetSessionArchivedRequest, SetSessionCompletionCheckRequest, SetSessionModeRequest,
    SetSessionNotificationsRequest, SetSiriDefaultSessionRequest, SubmitNotificationReplyRequest,
    UpsertCompletionCheckRequest, UpsertNotificationRouteRequest, client_frame, command,
    looper_realtime_client::LooperRealtimeClient, server_frame,
};
use agent_control_plane::http::build_router;
use agent_control_plane::mobile::api::{
    latest_session_mini_revision, session_mini_projection_inputs,
};
use agent_control_plane::mobile::events::{MobileEventKind, snapshot_revision_changed_event};
use agent_control_plane::mobile::session::MobileHookPayload;
use agent_control_plane::scheduler::AutomationRunner;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use futures_util::{SinkExt, StreamExt};
use http_body_util::BodyExt;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WebSocketMessage;
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
const SESSION_COMMAND_ACK_POLL_LIMIT: usize = 8;
const TEST_CONTENT_CHUNK_LIMIT_BYTES: usize = 65_536;

#[path = "isolated_control_plane/acp_hosts/mod.rs"]
mod acp_hosts;
#[path = "isolated_control_plane/hooks_routes.rs"]
mod hooks_routes;
#[path = "isolated_control_plane/mobile_classification.rs"]
mod mobile_classification;
#[path = "isolated_control_plane/mobile_events.rs"]
mod mobile_events;

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
    assert_eq!(status["hooks"]["enabled"], serde_json::json!(true));
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
    assert_eq!(
        automations["automations"][0]["target_known"],
        serde_json::json!(true)
    );

    let hook_contract = request_json(&router, "/integrations/hook/contract").await;
    assert_eq!(
        hook_contract["profile"],
        "agent-control-plane-local-hook-command-relay"
    );
    assert_eq!(
        hook_contract["ingress"]["path"],
        "managed local hook command"
    );
    assert_eq!(
        hook_contract["ingress"]["auth"],
        "local-user-owned-config-file"
    );
    assert_eq!(hook_contract["ingress"]["signature_header"], "not-used");
    assert_eq!(
        hook_contract["payload_policy"]["raw_prompts"],
        serde_json::json!(false)
    );
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
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "First user prompt for Handoff."
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

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
    assert_eq!(
        main_thread["first_user_prompt"],
        "First user prompt for Handoff."
    );
}

#[tokio::test]
async fn desktop_limited_snapshot_skips_request_time_transcript_preview() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_grok_session("grok-live-leak", "/tmp/project", "Grok live leak");
    fixture.write_claude_session(
        "claude-live-leak",
        "/tmp/claude-project",
        "Claude live prompt",
        "Claude live leak",
    );
    fixture.write_devin_next_session();
    let transcript_path = fixture.write_transcript(
        "thread-main-limited-preview.jsonl",
        &[
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Limited snapshot must not scan me."
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
                            "text": "Request-time preview leak."
                        }
                    ]
                }
            }),
        ],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());

    let snapshot = request_json(&router, "/desktop/snapshot?limit=30").await;
    let main_thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "thread-main")
        .expect("main thread");

    assert_eq!(main_thread["assistant_preview"], serde_json::Value::Null);
    assert_eq!(main_thread["first_user_prompt"], serde_json::Value::Null);
    assert_eq!(snapshot["grok_build"]["session_count"], 0);
    assert_eq!(snapshot["grok_build"]["active_session_count"], 0);
    assert_eq!(snapshot["grok_build"]["hooks"]["health"], "stale");
    assert_eq!(snapshot["devin_session_count"], 0);
    assert_eq!(snapshot["devin_active_session_count"], 0);

    let live_discovery_thread_ids = [
        "grok-live-leak",
        "claude:claude-live-leak",
        "devin:devin-cli:brindle-cadet",
    ];
    let threads = snapshot["threads"].as_array().expect("threads");
    for thread_id in live_discovery_thread_ids {
        assert!(
            threads
                .iter()
                .all(|thread| thread["thread_id"] != thread_id),
            "limited snapshot leaked live discovery thread {thread_id}"
        );
    }
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

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
    assert_eq!(goals["goals"][0]["target_known"], serde_json::json!(true));
    assert_eq!(goals["goals"][0]["sync_safe"], serde_json::json!(true));
    assert_ne!(goals["goals"][0]["content_hash"], "");
    let plural_goals = request_json(&router, "/goals").await;
    assert_eq!(plural_goals["goals"][0]["id"], "ship-looper");

    let manifest = request_json(&router, "/sync/manifest").await;
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(
        manifest["privacy"]["raw_goal_bodies"],
        serde_json::json!(false)
    );
    assert_eq!(
        manifest["privacy"]["raw_automation_prompts"],
        serde_json::json!(false)
    );
    assert_eq!(manifest["privacy"]["credentials"], serde_json::json!(false));
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
        ("blocked-goal", "blocked"),
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

    let goals = request_json(&router, "/goal").await;
    let statuses = goals["goals"]
        .as_array()
        .expect("goals")
        .iter()
        .map(|goal| goal["status"].as_str().expect("status"))
        .collect::<Vec<_>>();
    assert_eq!(
        statuses,
        vec![
            "achieved",
            "blocked",
            "budget-limited",
            "paused",
            "pursuing",
            "unmet"
        ]
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
    fixture.write_automation(
        "daily-review",
        r#"
id = "daily-review"
kind = "heartbeat"
name = "Daily Review"
status = "ACTIVE"
rrule = "FREQ=HOURLY;INTERVAL=1"
target_thread_id = "thread-main"
"#,
    );
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

    let goals = request_json(&router, "/goals").await;
    let goal = goals["goals"]
        .as_array()
        .expect("goals")
        .iter()
        .find(|goal| goal["id"] == "goal-main")
        .expect("sqlite goal");
    assert_eq!(goal["source_kind"], "sqlite");
    assert_eq!(goal["status"], "pursuing");
    assert_eq!(goal["running"], serde_json::json!(true));
    assert_eq!(goal["target_thread_id"], "thread-main");
    assert_eq!(goal["target_known"], serde_json::json!(true));
    assert_eq!(goal["tokens_used"], GOAL_FIXTURE_TOKENS_USED);

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    let thread = snapshot["threads"]
        .as_array()
        .expect("threads")
        .iter()
        .find(|thread| thread["thread_id"] == "thread-main")
        .expect("thread-main");
    assert_eq!(thread["goal"]["id"], "goal-main");
    assert_eq!(thread["goal"]["running"], serde_json::json!(true));

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
    let mobile_revision = mobile_snapshot["revision"]
        .as_str()
        .expect("mobile revision");
    assert!(mobile_revision.contains("goals=goal-main"));
    assert!(mobile_revision.contains("automations=daily-review"));
    assert_eq!(
        mobile_snapshot["workStatus"]["goalCount"],
        serde_json::json!(1)
    );
    assert_eq!(
        mobile_snapshot["workStatus"]["runningGoalCount"],
        serde_json::json!(1)
    );
    assert_eq!(
        mobile_snapshot["workStatus"]["automationCount"],
        serde_json::json!(1)
    );
    assert_eq!(
        mobile_snapshot["workStatus"]["activeAutomationCount"],
        serde_json::json!(1)
    );
    assert_eq!(
        mobile_snapshot["workStatus"]["coveredAutomationCount"],
        serde_json::json!(1)
    );
    assert_eq!(
        mobile_snapshot["workStatus"]["runningGoals"][0]["targetThreadId"],
        "thread-main"
    );
    assert_eq!(
        mobile_snapshot["workStatus"]["activeAutomations"][0]["controlPlaneCovered"],
        serde_json::json!(true)
    );
    let session = mobile_snapshot_session(&mobile_snapshot, "thread-main");
    assert_eq!(session["goal"]["id"], "goal-main");
    assert_eq!(session["goal"]["running"], serde_json::json!(true));

    let manifest = request_json(&router, "/sync/manifest").await;
    let sync_goal = manifest["goals"]
        .as_array()
        .expect("sync goals")
        .iter()
        .find(|goal| goal["id"] == "goal-main")
        .expect("sync sqlite goal");
    assert_eq!(sync_goal["running"], serde_json::json!(true));
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

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
    assert!(
        hook_contract["events"]
            .as_array()
            .expect("hook events")
            .iter()
            .all(|event| event["source"] != "cloud-relay")
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
    assert_eq!(
        response["hooks_auto_registration"],
        serde_json::json!(false)
    );
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
    assert_eq!(response["hooks_auto_registration"], serde_json::json!(true));
    assert_eq!(
        response["status"]["hooks"]["enabled"],
        serde_json::json!(true)
    );
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
    assert_eq!(response["hooks_auto_registration"], serde_json::json!(true));

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
    assert_eq!(
        clear_response["hooks_auto_registration"],
        serde_json::json!(true)
    );

    let cleared_grok_hooks_json = fs::read_to_string(&grok_hooks_path).expect("cleared grok hooks");
    assert!(!cleared_grok_hooks_json.contains("agent-control-plane"));

    let register_again_response =
        request_json_with_method(&router, Method::POST, "/hooks/grok/register").await;
    assert_eq!(register_again_response["installed_handlers"], 3);

    let unregister_response =
        request_json_with_method(&router, Method::POST, "/hooks/grok/unregister").await;
    assert_eq!(unregister_response["action"], "unregister-grok-build-hooks");
    assert_eq!(unregister_response["removed_handlers"], 3);
    assert_eq!(unregister_response["installed_handlers"], 0);
    assert_eq!(
        unregister_response["hooks_auto_registration"],
        serde_json::json!(false)
    );

    let unregistered_grok_hooks_json =
        fs::read_to_string(&grok_hooks_path).expect("unregistered grok hooks");
    assert!(!unregistered_grok_hooks_json.contains("agent-control-plane"));
}

#[tokio::test]
async fn unknown_hook_target_is_rejected() {
    let fixture = IsolatedCodexFixture::new();
    let router = build_router(fixture.control_plane());

    let response = request_with_options(
        &router,
        Method::POST,
        "/hooks/unknown/register",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let unregister_response = request_with_options(
        &router,
        Method::POST,
        "/hooks/unknown/unregister",
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(unregister_response.status(), StatusCode::NOT_FOUND);
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
        "/hooks/codex/register",
        "/hooks/devin/register",
        "/hooks/grok/register",
        "/hooks/claude/register",
        "/hooks/unregister",
        "/hooks/codex/unregister",
        "/hooks/devin/unregister",
        "/hooks/grok/unregister",
        "/hooks/claude/unregister",
        "/hooks/unregister-live",
        "/hooks/codex/unregister-live",
        "/hooks/devin/unregister-live",
        "/hooks/grok/unregister-live",
        "/hooks/claude/unregister-live",
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
    assert_eq!(response["hooks_auto_registration"], serde_json::json!(true));
    assert_eq!(response["removed_handlers"], 3);
    assert_eq!(response["installed_handlers"], 0);

    let hooks_json = fs::read_to_string(fixture.codex_home.join("hooks.json")).expect("hooks");
    assert!(!hooks_json.contains("agent-control-plane"));

    let register_response =
        request_json_with_method(&router, Method::POST, "/hooks/register").await;
    assert_eq!(
        register_response["hooks_auto_registration"],
        serde_json::json!(true)
    );
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

    assert_eq!(response["ok"], serde_json::json!(true));
    assert_eq!(response["baseURL"], "http://192.168.99.10:8765");
    assert_eq!(response["baseURLs"][0], "http://192.168.99.10:8765");
    assert_eq!(response["grpcBaseURL"], "http://192.168.99.10:8766");
    assert_eq!(response["grpcBaseURLs"][0], "http://192.168.99.10:8766");
    assert_eq!(response["grpcH3BaseURL"], "https://192.168.99.10:8766");
    assert_eq!(response["grpcH3BaseURLs"][0], "https://192.168.99.10:8766");
    assert!(
        response["grpcH3CertificateSha256"]
            .as_str()
            .expect("H3 certificate pin")
            .starts_with("sha256:")
    );
    println!(
        "mobile_health_realtime_routes h2={} h3={} pin={}",
        response["grpcBaseURL"].as_str().unwrap_or_default(),
        response["grpcH3BaseURL"].as_str().unwrap_or_default(),
        response["grpcH3CertificateSha256"]
            .as_str()
            .unwrap_or_default()
    );
    assert_eq!(
        response["tailscale"]["grpcH3BaseURL"],
        serde_json::Value::Null
    );
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
    assert_ne!(local_response["grpcBaseURL"], "");
    assert!(
        !local_response["grpcBaseURLs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_ne!(local_response["grpcH3BaseURL"], "");
    assert!(
        !local_response["grpcH3BaseURLs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        local_response["grpcH3CertificateSha256"]
            .as_str()
            .expect("H3 certificate pin")
            .starts_with("sha256:")
    );
    println!(
        "mobile_pairing_realtime_routes h2={} h3={} pin={}",
        local_response["grpcBaseURL"].as_str().unwrap_or_default(),
        local_response["grpcH3BaseURL"].as_str().unwrap_or_default(),
        local_response["grpcH3CertificateSha256"]
            .as_str()
            .unwrap_or_default()
    );
}

#[tokio::test]
async fn desktop_connections_manage_mobile_pairings_and_codex_rows() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("agent-control-plane --hook --managed-by looper");
    fixture.write_config_toml(true);
    fixture.write_devin_next_settings();
    fixture.write_zed_settings();
    let control_plane = fixture.control_plane_with_running_zed();
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
                && target["ready"] == true
                && target["status"] == "ready"
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
    assert_eq!(acp_probe["probe"]["ready"], serde_json::json!(true));
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
        "agent-configured"
    );
    assert_eq!(zed_acp_host["agents"][0]["supports_sessions"], true);
    assert_eq!(zed_acp_host["agents"][0]["supports_prompt"], true);
    assert_eq!(zed_acp_host["agents"][0]["supports_cancel"], true);
    assert!(
        zed_acp_host["actions"]
            .as_array()
            .expect("zed actions")
            .iter()
            .any(|action| action["id"] == "install"
                && action["path"] == "/desktop/acp-client-hosts/zed/install")
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
    assert_eq!(zed_generic_probe["probe"]["status"], "ready");
    assert_eq!(zed_generic_probe["probe"]["probe_kind"], "looper-stdio");
    assert_eq!(zed_generic_probe["probe"]["ready"], true);
    assert_eq!(zed_generic_probe["probe"]["agent_id"], "looper");

    let zed_install = request_json_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/zed/install",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(zed_install["install"]["installed_agent_id"], "codex");
    assert_eq!(zed_install["install"]["preferred_agent"], "codex");

    let missing_acp_host = request_with_options(
        &router,
        Method::GET,
        "/desktop/acp-client-hosts/unknown",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(missing_acp_host.status(), StatusCode::NOT_FOUND);
    let missing_acp_probe = request_with_body_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/unknown/probe",
        serde_json::to_vec(&serde_json::json!({ "agentId": "looper" })).expect("probe body"),
        &[(axum::http::header::CONTENT_TYPE, "application/json")],
        loopback_socket,
    )
    .await;
    assert_eq!(missing_acp_probe.status(), StatusCode::NOT_FOUND);
    let missing_acp_install = request_with_options(
        &router,
        Method::POST,
        "/desktop/acp-client-hosts/unknown/install",
        &[],
        loopback_socket,
    )
    .await;
    assert_eq!(missing_acp_install.status(), StatusCode::NOT_FOUND);

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
    assert_eq!(revoked_mobile["can_revoke"], serde_json::json!(false));
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
    assert!(
        snapshot["revision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert_eq!(snapshot["latestSeq"], snapshot["latest_seq"]);
    assert!(
        snapshot["latestSeq"]
            .as_i64()
            .is_some_and(|latest_seq| latest_seq >= 0)
    );
    assert_eq!(snapshot["snapshotKind"], "bootstrapRecovery");
    assert_eq!(snapshot["snapshot_kind"], "bootstrapRecovery");
    assert_eq!(snapshot["serverTime"], snapshot["host"]["lastSyncedAt"]);
    assert_eq!(snapshot["freshness"]["source"], "desktop-mobile-snapshot");
    assert_eq!(snapshot["freshness"]["latestSeq"], snapshot["latestSeq"]);
    assert_eq!(snapshot["freshness"]["revision"], snapshot["revision"]);
    assert_eq!(snapshot["freshness"]["serverTime"], snapshot["serverTime"]);
    assert_eq!(snapshot["sessions"][0]["id"], "thread-main");
    assert!(
        !snapshot["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .any(|session| session["id"] == "thread-child")
    );
}

#[tokio::test]
async fn mobile_snapshot_includes_full_recent_thread_list() {
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
    assert_eq!(sessions.len(), EXTRA_MOBILE_SNAPSHOT_THREADS + 1);
    assert!(
        sessions
            .iter()
            .any(|session| session["id"] == "thread-main")
    );
    assert!(
        !sessions
            .iter()
            .any(|session| session["id"] == "thread-child")
    );
    assert_eq!(
        sessions
            .iter()
            .filter(|session| session["id"]
                .as_str()
                .expect("session id")
                .starts_with("thread-extra-"))
            .count(),
        EXTRA_MOBILE_SNAPSHOT_THREADS
    );
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
    assert_eq!(
        snapshot["automations"][0]["target_known"],
        serde_json::json!(true)
    );
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
        snapshot["globalSettings"]["defaultNotificationTargetIds"],
        serde_json::json!(["macos", "route-telegram"])
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
    assert_eq!(
        detail["completionCheckWaitForReply"],
        serde_json::json!(false)
    );
    assert_eq!(detail["availableNotifications"][0]["id"], "route-telegram");
    assert_eq!(detail["availableCompletionChecks"][0]["id"], "check-cargo");
}

#[tokio::test]
async fn session_mini_projection_includes_card_blocked_goal_and_notification_state() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_thread_goal(
        "thread-main",
        "goal-blocked-main",
        "Unblock the mobile card",
        "blocked",
    );
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
        .set_default_notification_targets(&["iphone".to_owned(), "route-telegram".to_owned()])
        .expect("set default notification targets");
    service
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set await reply mode");
    service
        .queue_prompt("thread-main", "Reply from phone.")
        .expect("queue prompt");
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(snapshot["latestSeq"], snapshot["latest_seq"]);
    let session = session_mini_snapshot_session(&snapshot, "thread-main");
    assert!(!session_mini_snapshot_has_session(
        &snapshot,
        "thread-child"
    ));
    assert_eq!(session["id"], "thread-main");
    assert_eq!(session["sessionId"], "thread-main");
    assert_eq!(session["title"], "Main task");
    assert_eq!(session["ref"], "T1");
    assert_eq!(session["assistantClient"], "codex");
    assert_eq!(session["assistantSurface"], "codex");
    assert_eq!(session["effectiveMode"], "await-reply");
    assert_eq!(session["canSendPrompt"], serde_json::json!(true));
    assert_eq!(session["replyable"], serde_json::json!(true));
    assert_eq!(
        session["promptDeliveryUnavailableReason"],
        serde_json::Value::Null
    );
    assert_eq!(session["blockedGoal"]["status"], "blocked");
    assert_eq!(session["blockedGoal"]["title"], "Unblock the mobile card");
    assert_eq!(session["queueCount"], 1);
    assert_eq!(session["status"], "waiting");
    assert_eq!(session["lifecycle"], "waiting");
    assert_eq!(session["notificationStatus"]["enabled"], true);
    assert_eq!(
        session["notificationStatus"]["targetIds"],
        serde_json::json!(["iphone", "macos", "route-telegram"])
    );
    assert_eq!(session["notificationStatus"]["usesDefault"], true);
    assert_eq!(session["metadata"]["projectPath"], "/tmp/project");
}

#[tokio::test]
async fn session_mini_projection_advances_seq_on_mode_mutation() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let initial = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let initial_session = session_mini_snapshot_session(&initial, "thread-main");
    let initial_seq = initial["latestSeq"].as_i64().expect("initial latest seq");
    assert!(initial_session.get("revision").is_none());
    let initial_revision = latest_session_mini_revision(
        &control_plane
            .store()
            .mobile_session_minis()
            .expect("initial mini records"),
    )
    .expect("initial revision");

    let mode_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSessionMode(SetSessionModeRequest {
            thread_id: "thread-main".to_owned(),
            preset: "await-reply".to_owned(),
            client_mutation_id: "session-mini-mode-await-reply".to_owned(),
        }),
    )
    .await;
    assert!(mode_ack.accepted);

    let replayed_minis = control_plane
        .store()
        .mobile_session_minis_after_seq(initial_seq, 10)
        .expect("mini replay after seq");
    assert!(replayed_minis.iter().any(|record| {
        record.session_id == "thread-main"
            && record.assistant_surface == "codex"
            && record.seq > initial_seq
            && record
                .body_json
                .contains("\"effectiveMode\":\"await-reply\"")
    }));
    let replayed = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/session-minis?after_seq={initial_seq}&limit=10"),
        &auth_headers,
        None,
    )
    .await;
    let replayed_session = session_mini_snapshot_session(&replayed, "thread-main");
    assert_eq!(replayed["replace"], true);
    assert_eq!(replayed_session["assistantSurface"], "codex");
    assert_eq!(replayed_session["effectiveMode"], "await-reply");
    assert!(
        replayed_session["seq"]
            .as_i64()
            .expect("replayed session seq")
            > initial_seq
    );

    let updated = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let updated_session = session_mini_snapshot_session(&updated, "thread-main");
    let updated_seq = updated["latestSeq"].as_i64().expect("updated latest seq");

    assert!(updated_seq > initial_seq);
    assert!(updated_session["seq"].as_i64().expect("session seq") >= updated_seq);
    assert!(updated_session.get("revision").is_none());
    let updated_revision = latest_session_mini_revision(
        &control_plane
            .store()
            .mobile_session_minis()
            .expect("updated mini records"),
    )
    .expect("updated revision");
    assert_eq!(updated_revision, initial_revision);
    assert_eq!(updated_session["effectiveMode"], "await-reply");
    assert_eq!(updated_session["status"], "stopped");
}

#[tokio::test]
async fn session_mini_reconcile_publishes_fresh_transcript_activity() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-realtime.jsonl",
        &[serde_json::json!({
            "timestamp": "2026-06-16T08:00:00Z",
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
        })],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let control_plane = fixture.control_plane();

    let initial_snapshot = control_plane
        .desktop_mobile_snapshot()
        .expect("initial desktop mobile snapshot");
    let initial_revision = initial_snapshot.revision.clone();
    let initial_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("initial mobile seq");
    let initial_minis = session_mini_projection_inputs(
        &initial_snapshot,
        &control_plane
            .mobile_session_service()
            .state()
            .expect("mobile session state"),
        &control_plane
            .mobile_session_service()
            .queued_prompt_counts()
            .expect("queued prompt counts"),
        initial_seq,
        &initial_revision,
    );
    control_plane
        .store()
        .replace_mobile_session_minis(initial_minis, initial_seq, &initial_revision)
        .expect("seed initial mini projection");
    let initial_records = control_plane
        .store()
        .mobile_session_minis_at_seq(initial_seq)
        .expect("initial projection records");
    let initial_main = initial_records
        .iter()
        .find(|record| record.session_id == "thread-main")
        .expect("initial thread-main mini");
    let initial_payload: serde_json::Value =
        serde_json::from_str(&initial_main.body_json).expect("initial mini payload json");
    let initial_last_activity_at_ms = initial_payload["lastActivityAtMs"]
        .as_i64()
        .expect("initial last activity at ms");

    std::thread::sleep(std::time::Duration::from_millis(2));
    let transcript_path = fixture.write_transcript(
        "thread-main-realtime.jsonl",
        &[
            serde_json::json!({
                "timestamp": "2026-06-16T08:00:00Z",
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
                "timestamp": "2026-06-16T08:06:00Z",
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Fresh assistant reply for Session stream."
                        }
                    ]
                }
            }),
        ],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);

    assert!(
        control_plane
            .reconcile_mobile_session_mini_projection()
            .expect("reconcile projection"),
        "fresh transcript activity should publish a replacement mini event"
    );
    assert!(
        !control_plane
            .reconcile_mobile_session_mini_projection()
            .expect("second reconcile projection"),
        "stored projection revision should make repeated reconcile a no-op"
    );

    let latest_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest mobile seq");
    assert!(latest_seq > initial_seq);
    assert!(
        control_plane
            .store()
            .mobile_session_minis_replaced_at_seq(latest_seq)
            .expect("replacement marker"),
        "reconcile must leave a replacement marker for the Session stream"
    );
    let records = control_plane
        .store()
        .mobile_session_minis_at_seq(latest_seq)
        .expect("latest projection records");
    let main = records
        .iter()
        .find(|record| record.session_id == "thread-main")
        .expect("thread-main mini");
    let payload: serde_json::Value =
        serde_json::from_str(&main.body_json).expect("mini payload json");
    assert_eq!(
        payload["assistantPreview"],
        "Fresh assistant reply for Session stream."
    );
    assert!(
        payload["lastActivityAtMs"]
            .as_i64()
            .expect("latest last activity at ms")
            > initial_last_activity_at_ms
    );
}

#[tokio::test]
async fn session_mini_reconcile_replaces_stale_cache_shape_for_same_revision() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);

    let current_records = control_plane
        .store()
        .mobile_session_minis()
        .expect("current mini records");
    let current_revision =
        latest_session_mini_revision(&current_records).expect("current mini revision");
    let current_keys = current_records
        .iter()
        .map(|record| (record.session_id.clone(), record.assistant_surface.clone()))
        .collect::<BTreeSet<_>>();

    let mut stale_minis = current_records
        .iter()
        .map(|record| MobileSessionMiniProjectionInput {
            session_id: record.session_id.clone(),
            assistant_surface: record.assistant_surface.clone(),
            body_json: serde_json::from_str(&record.body_json).expect("mini body json"),
        })
        .collect::<Vec<_>>();
    let mut stale_extra_body = stale_minis
        .first()
        .expect("at least one mini")
        .body_json
        .clone();
    stale_extra_body["id"] = serde_json::json!("thread-stale-extra");
    stale_extra_body["sessionId"] = serde_json::json!("thread-stale-extra");
    stale_extra_body["ref"] = serde_json::json!("SX");
    stale_extra_body["title"] = serde_json::json!("Stale extra mini");
    stale_minis.push(MobileSessionMiniProjectionInput {
        session_id: "thread-stale-extra".to_owned(),
        assistant_surface: "codex".to_owned(),
        body_json: stale_extra_body,
    });
    let stale_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest mobile seq")
        + 1;
    control_plane
        .store()
        .replace_mobile_session_minis(stale_minis, stale_seq, &current_revision)
        .expect("write stale same-revision minis");

    assert!(
        control_plane
            .reconcile_mobile_session_mini_projection()
            .expect("reconcile stale shape"),
        "same revision must still replace a stale persisted mini keyset"
    );
    let repaired_records = control_plane
        .store()
        .mobile_session_minis()
        .expect("repaired mini records");
    let repaired_keys = repaired_records
        .iter()
        .map(|record| (record.session_id.clone(), record.assistant_surface.clone()))
        .collect::<BTreeSet<_>>();

    assert_eq!(repaired_keys, current_keys);
    assert!(
        !repaired_records
            .iter()
            .any(|record| record.session_id == "thread-stale-extra")
    );
    assert!(
        !control_plane
            .reconcile_mobile_session_mini_projection()
            .expect("second reconcile"),
        "matching current projection should return to no-op"
    );
}

#[tokio::test]
async fn session_mini_snapshot_includes_old_stopped_unarchived_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.append_newer_than_devin_state_threads(1);
    let control_plane = fixture.control_plane();
    record_thread_stopped(&control_plane, "thread-main");
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert!(session_mini_snapshot_has_session(&snapshot, "thread-main"));
    assert!(session_mini_snapshot_has_session(
        &snapshot,
        "thread-extra-00"
    ));
}

#[tokio::test]
async fn session_mini_snapshot_requires_produced_projection() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;

    let response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("recovery body")
        .to_bytes();
    let recovery: serde_json::Value = serde_json::from_slice(&body).expect("recovery json");
    assert_eq!(recovery["error"], "recovery_required");
    assert_eq!(recovery["recovery"], "/api/mobile/session-minis/snapshot");
    assert_eq!(recovery["latestSeq"], recovery["latest_seq"]);
    assert!(
        recovery["revision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert_eq!(recovery["serverTime"], recovery["server_time"]);
    assert_eq!(
        recovery["freshness"]["source"],
        "mobile-session-mini-projection"
    );
    assert_eq!(recovery["freshness"]["latestSeq"], recovery["latestSeq"]);
    assert_eq!(recovery["freshness"]["revision"], recovery["revision"]);
    assert_eq!(recovery["freshness"]["serverTime"], recovery["serverTime"]);
}

#[tokio::test]
async fn session_mini_snapshot_accepts_empty_produced_projection() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    control_plane
        .store()
        .record_mobile_event_replacing_session_minis(
            &snapshot_revision_changed_event("empty-session-mini-projection".to_owned()),
            Vec::<MobileSessionMiniProjectionInput>::new(),
        )
        .expect("empty replacement projection");
    let projection_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("projection seq");
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;

    let response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("snapshot body")
        .to_bytes();
    let snapshot: serde_json::Value = serde_json::from_slice(&body).expect("snapshot json");
    assert_eq!(snapshot["latestSeq"], projection_seq);
    assert_eq!(snapshot["latestSeq"], snapshot["latest_seq"]);
    assert_eq!(snapshot["revision"], "empty-session-mini-projection");
    assert_eq!(snapshot["replace"], true);
    assert_eq!(
        snapshot["sessions"]
            .as_array()
            .expect("snapshot sessions")
            .len(),
        0
    );
    assert_eq!(
        snapshot["freshness"]["source"],
        "mobile-session-mini-projection"
    );
    assert_eq!(snapshot["freshness"]["latestSeq"], snapshot["latestSeq"]);
    assert_eq!(snapshot["freshness"]["revision"], snapshot["revision"]);
}

#[tokio::test]
async fn session_mini_snapshot_requires_recovery_for_partial_cache_without_replacement_marker() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.append_newer_than_devin_state_threads(1);
    let control_plane = fixture.control_plane();
    let snapshot = control_plane
        .desktop_mobile_snapshot()
        .expect("desktop snapshot");
    let session_state = control_plane
        .mobile_session_service()
        .state()
        .expect("mobile session state");
    let queued_prompt_counts = control_plane
        .mobile_session_service()
        .queued_prompt_counts()
        .expect("queued prompt counts");
    let latest_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest mobile seq");
    let minis = session_mini_projection_inputs(
        &snapshot,
        &session_state,
        &queued_prompt_counts,
        latest_seq,
        &snapshot.revision,
    );
    assert!(
        minis.len() > 1,
        "fixture must have more than one mini so a partial cache can be detected"
    );
    let stale_mini = minis
        .into_iter()
        .find(|mini| mini.session_id == "thread-main")
        .expect("thread-main mini");
    control_plane
        .store()
        .upsert_mobile_session_mini(stale_mini, latest_seq, "stale-partial-cache")
        .expect("seed stale partial mini");
    assert!(
        !control_plane
            .store()
            .mobile_session_minis_replaced_at_seq(latest_seq)
            .expect("replacement marker check"),
        "direct mini upserts must not masquerade as a full replacement snapshot"
    );

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("recovery body")
        .to_bytes();
    let recovery: serde_json::Value = serde_json::from_slice(&body).expect("recovery json");
    assert_eq!(recovery["error"], "recovery_required");
    assert_eq!(recovery["recovery"], "/api/mobile/session-minis/snapshot");
    assert_eq!(recovery["latestSeq"], recovery["latest_seq"]);
    assert_eq!(recovery["revision"], "stale-partial-cache");
    assert_eq!(recovery["serverTime"], recovery["server_time"]);
    assert_eq!(
        recovery["freshness"]["source"],
        "mobile-session-mini-projection"
    );
    assert_eq!(recovery["freshness"]["latestSeq"], recovery["latestSeq"]);
    assert_eq!(recovery["freshness"]["revision"], recovery["revision"]);
    assert_eq!(recovery["freshness"]["serverTime"], recovery["serverTime"]);
    assert!(
        !control_plane
            .store()
            .mobile_session_minis_replaced_at_seq(latest_seq)
            .expect("replacement marker after recovery request"),
        "recovery request must not write a complete replacement marker"
    );
    assert_eq!(
        control_plane
            .store()
            .mobile_session_minis()
            .expect("mini records after recovery request")
            .len(),
        1,
        "recovery request must not rebuild the partial cache"
    );
}

#[tokio::test]
async fn session_mini_projection_replays_default_notification_target_mutation() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
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
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let initial = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let initial_seq = initial["latestSeq"].as_i64().expect("initial latest seq");

    let ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetDefaultNotificationTargets(SetDefaultNotificationTargetsRequest {
            notification_target_ids: vec!["iphone".to_owned(), "route-telegram".to_owned()],
            client_mutation_id: "default-notification-targets-session-test".to_owned(),
        }),
    )
    .await;
    assert!(ack.accepted, "Session default target command accepted");

    let replayed = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/session-minis?after_seq={initial_seq}&limit=10"),
        &auth_headers,
        None,
    )
    .await;
    let session = session_mini_snapshot_session(&replayed, "thread-main");

    assert_eq!(replayed["replace"], true);
    assert!(
        replayed["latestSeq"].as_i64().expect("replayed latest seq") > initial_seq,
        "default notification target mutation must advance replacement seq"
    );
    assert_eq!(session["notificationStatus"]["enabled"], true);
    assert_eq!(
        session["notificationStatus"]["targetIds"],
        serde_json::json!(["iphone", "macos", "route-telegram"])
    );
    assert_eq!(session["notificationStatus"]["usesDefault"], true);
}

#[tokio::test]
async fn session_mini_projection_default_notification_targets_ignore_stale_cache() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
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
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let initial = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let initial_seq = initial["latestSeq"].as_i64().expect("initial latest seq");
    fixture.append_newer_than_devin_state_threads(1);

    let ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetDefaultNotificationTargets(SetDefaultNotificationTargetsRequest {
            notification_target_ids: vec!["iphone".to_owned(), "route-telegram".to_owned()],
            client_mutation_id: "default-notification-targets-fresh-projection-test".to_owned(),
        }),
    )
    .await;
    assert!(ack.accepted, "Session default target command accepted");

    let replayed = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/session-minis?after_seq={initial_seq}&limit=10"),
        &auth_headers,
        None,
    )
    .await;

    assert_eq!(replayed["replace"], true);
    assert!(
        session_mini_snapshot_has_session(&replayed, "thread-extra-00"),
        "default notification target mutation must publish a fresh replacement projection, not stale cached minis"
    );
}

#[tokio::test]
async fn session_mini_projection_replays_session_notification_mutation_without_stale_cache() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
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
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let initial = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let initial_seq = initial["latestSeq"].as_i64().expect("initial latest seq");
    let initial_session = session_mini_snapshot_session(&initial, "thread-main");
    assert_eq!(
        initial_session["notificationStatus"]["usesDefault"],
        serde_json::json!(true)
    );

    let ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSessionNotifications(SetSessionNotificationsRequest {
            thread_id: "thread-main".to_owned(),
            notification_ids: vec!["route-telegram".to_owned()],
            client_mutation_id: "session-notifications-fresh-overlay-test".to_owned(),
        }),
    )
    .await;
    assert!(ack.accepted, "Session notification command accepted");

    let replayed = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/session-minis?after_seq={initial_seq}&limit=10"),
        &auth_headers,
        None,
    )
    .await;
    let session = session_mini_snapshot_session(&replayed, "thread-main");
    assert_eq!(session["notificationStatus"]["enabled"], true);
    assert_eq!(
        session["notificationStatus"]["targetIds"],
        serde_json::json!(["route-telegram"])
    );
    assert_eq!(session["notificationStatus"]["usesDefault"], false);
}

#[tokio::test]
async fn session_mini_projection_removes_deleted_and_hidden_sessions_from_replay() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let initial = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert!(session_mini_snapshot_has_session(&initial, "thread-main"));
    let initial_seq = initial["latestSeq"].as_i64().expect("initial latest seq");

    prime_state_mini_cache(&control_plane);
    let delete_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::DeleteSession(DeleteSessionRequest {
            thread_id: "thread-main".to_owned(),
            client_mutation_id: "session-mini-delete-thread-main".to_owned(),
        }),
    )
    .await;
    assert!(delete_ack.accepted);
    wait_for_session_mini_absent(&control_plane, "thread-main").await;

    let after_delete = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/session-minis?after_seq={initial_seq}&limit=10"),
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(after_delete["replace"], true);
    assert!(!session_mini_snapshot_has_session(
        &after_delete,
        "thread-main"
    ));
    assert!(session_mini_snapshot_has_session(
        &after_delete,
        "devin:devin-cli:brindle-cadet"
    ));
}

#[tokio::test]
async fn session_mini_snapshot_is_recovery_only() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let expected_revision = latest_session_mini_revision(
        &control_plane
            .store()
            .mobile_session_minis()
            .expect("mini records"),
    )
    .expect("mini revision");
    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;

    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert!(snapshot.get("sessions").is_some());
    assert!(snapshot.get("latestSeq").is_some());
    assert!(snapshot.get("latest_seq").is_some());
    assert_eq!(snapshot["revision"], expected_revision);
    assert!(
        snapshot["serverTime"]
            .as_str()
            .is_some_and(|server_time| !server_time.is_empty())
    );
    assert_eq!(snapshot["serverTime"], snapshot["server_time"]);
    assert_eq!(
        snapshot["freshness"]["source"],
        "mobile-session-mini-projection"
    );
    assert_eq!(snapshot["freshness"]["latestSeq"], snapshot["latestSeq"]);
    assert_eq!(snapshot["freshness"]["revision"], snapshot["revision"]);
    assert_eq!(snapshot["freshness"]["serverTime"], snapshot["serverTime"]);
    assert_eq!(snapshot["snapshotKind"], "recovery");
    assert_eq!(snapshot["snapshot_kind"], "recovery");
    assert_eq!(snapshot["replace"], true);
    assert!(snapshot.get("surfaceSessions").is_none());
    assert!(snapshot.get("globalSettings").is_none());
    assert!(snapshot.get("notifications").is_none());
    assert!(snapshot.get("completionChecks").is_none());
    let session = session_mini_snapshot_session(&snapshot, "thread-main");
    assert!(session.get("latestAssistantMessage").is_none());
    assert!(session.get("availableNotifications").is_none());
    assert!(session.get("availableCompletionChecks").is_none());

    let gap_response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis?after_seq=999999",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;
    assert_eq!(gap_response.status(), StatusCode::CONFLICT);
    let body = gap_response
        .into_body()
        .collect()
        .await
        .expect("gap body")
        .to_bytes();
    let gap: serde_json::Value = serde_json::from_slice(&body).expect("gap json");
    assert_eq!(gap["error"], "seq_gap");
    assert_eq!(gap["latestSeq"], gap["latest_seq"]);
    assert_eq!(gap["requestedAfterSeq"], gap["requested_after_seq"]);
    assert!(
        gap["revision"]
            .as_str()
            .is_some_and(|revision| !revision.is_empty())
    );
    assert_eq!(gap["serverTime"], gap["server_time"]);
    assert_eq!(gap["freshness"]["source"], "mobile-session-mini-projection");
    assert_eq!(gap["freshness"]["latestSeq"], gap["latestSeq"]);
    assert_eq!(gap["freshness"]["revision"], gap["revision"]);
    assert_eq!(gap["freshness"]["serverTime"], gap["serverTime"]);
    assert_eq!(gap["recovery"], "/api/mobile/session-minis/snapshot");
}

#[tokio::test]
async fn session_mini_snapshot_exposes_projection_seq_when_event_log_is_newer() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let projection_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("projection seq");
    let projection_revision = latest_session_mini_revision(
        &control_plane
            .store()
            .mobile_session_minis()
            .expect("mini records"),
    )
    .expect("mini revision");
    control_plane
        .store()
        .record_mobile_event(&snapshot_revision_changed_event(
            "newer-event-without-mini-projection".to_owned(),
        ))
        .expect("newer mobile event");
    let latest_event_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest event seq");
    assert!(
        latest_event_seq > projection_seq,
        "fixture must leave the event log ahead of the mini projection"
    );

    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;
    let snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/session-minis/snapshot",
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(snapshot["latestSeq"], projection_seq);
    assert!(
        snapshot["latestSeq"].as_i64().expect("snapshot seq") < latest_event_seq,
        "HTTP recovery must expose projection freshness so stream-newer clients reject it"
    );
    assert_eq!(snapshot["freshness"]["latestSeq"], snapshot["latestSeq"]);
    assert_eq!(snapshot["revision"], projection_revision);
    assert_ne!(
        snapshot["revision"], "newer-event-without-mini-projection",
        "HTTP recovery must not advertise a non-projected event revision as snapshot freshness"
    );
    assert_eq!(snapshot["freshness"]["revision"], snapshot["revision"]);
    assert_eq!(snapshot["snapshotKind"], "recovery");
}

#[tokio::test]
async fn session_mini_delta_exposes_projection_seq_when_event_log_is_newer() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    prime_state_mini_cache(&control_plane);
    let projection_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("projection seq");
    let projection_revision = latest_session_mini_revision(
        &control_plane
            .store()
            .mobile_session_minis()
            .expect("mini records"),
    )
    .expect("mini revision");
    control_plane
        .store()
        .record_mobile_event(&snapshot_revision_changed_event(
            "newer-delta-event-without-mini-projection".to_owned(),
        ))
        .expect("newer mobile event");
    let latest_event_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .expect("latest event seq");
    assert!(
        latest_event_seq > projection_seq,
        "fixture must leave the event log ahead of the mini projection"
    );

    let router = build_router(control_plane);
    let authorization = issue_mobile_authorization_header(&router).await;
    let delta = request_json_with_options(
        &router,
        Method::GET,
        &format!("/api/mobile/session-minis?after_seq={projection_seq}&limit=10"),
        &[(axum::http::header::AUTHORIZATION, authorization.as_str())],
        None,
    )
    .await;

    assert_eq!(delta["latestSeq"], projection_seq);
    assert!(
        delta["latestSeq"].as_i64().expect("delta seq") < latest_event_seq,
        "HTTP delta recovery must not advertise event-log freshness beyond projected minis"
    );
    assert_eq!(delta["latest_seq"], delta["latestSeq"]);
    assert_eq!(delta["freshness"]["latestSeq"], delta["latestSeq"]);
    assert_eq!(delta["revision"], projection_revision);
    assert_ne!(
        delta["revision"], "newer-delta-event-without-mini-projection",
        "HTTP delta recovery must not advertise a non-projected event revision as mini freshness"
    );
    assert_eq!(delta["freshness"]["revision"], delta["revision"]);
    assert_eq!(delta["snapshotKind"], "recovery");
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
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "First prompt belongs in details."
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
    assert_eq!(detail["title"], "Main task");
    assert_eq!(
        detail["firstUserPrompt"],
        "First prompt belongs in details."
    );
    assert_eq!(
        detail["assistantPreview"],
        "Latest assistant reply from transcript."
    );
    assert_eq!(
        detail["metadata"]["transcriptAvailable"],
        serde_json::json!(true)
    );
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
async fn mobile_session_content_tail_returns_bounded_chunk_metadata() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-content-tail.jsonl",
        &[
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "a".repeat(TEST_CONTENT_CHUNK_LIMIT_BYTES + 2048)
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
                            "text": "latest-tail"
                        }
                    ]
                }
            }),
        ],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;

    let response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main/content?range=tail&limit=65536",
        &[(axum::http::header::AUTHORIZATION, &authorization)],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    let chunk = &json["chunk"];
    let transcript = fs::read(&transcript_path).expect("transcript bytes");
    let expected_offset = transcript.len() - TEST_CONTENT_CHUNK_LIMIT_BYTES;
    let expected_chunk = &transcript[expected_offset..];

    assert_eq!(json["content_type"], "transcript");
    assert_eq!(chunk["account_id"], "local-account");
    assert_eq!(chunk["node_id"], "local-node");
    assert_eq!(chunk["session_id"], "thread-main");
    assert_eq!(chunk["offset"], expected_offset);
    assert_eq!(chunk["length"], TEST_CONTENT_CHUNK_LIMIT_BYTES);
    assert_eq!(chunk["sha256"], sha256_hex(expected_chunk));
    assert_eq!(
        chunk["content"].as_str().expect("content").as_bytes(),
        expected_chunk
    );
    assert_eq!(
        chunk["next_cursor"],
        format!(
            "after:{}:{}",
            chunk["revision"].as_str().expect("revision"),
            transcript.len()
        )
    );
    assert!(chunk["merkle_root"].is_null());
    assert!(chunk["merkle_proof"].is_null());
}

#[tokio::test]
async fn mobile_session_content_after_returns_chunk_from_valid_cursor() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-content-after.jsonl",
        &[
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "already synced"
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
                            "text": "after cursor payload"
                        }
                    ]
                }
            }),
        ],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;
    let tail_path = format!(
        "/api/mobile/sessions/thread-main/content?range=tail&limit={TEST_CONTENT_CHUNK_LIMIT_BYTES}"
    );

    let tail_response = request_with_options(
        &router,
        Method::GET,
        &tail_path,
        &[(axum::http::header::AUTHORIZATION, &authorization)],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(tail_response.status(), StatusCode::OK);
    let tail_body = tail_response
        .into_body()
        .collect()
        .await
        .expect("tail body")
        .to_bytes();
    let tail_json: serde_json::Value = serde_json::from_slice(&tail_body).expect("tail json");
    let revision = tail_json["chunk"]["revision"]
        .as_str()
        .expect("tail revision");

    let transcript = fs::read(&transcript_path).expect("transcript bytes");
    let expected_offset = transcript
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|separator_index| separator_index + 1)
        .expect("record separator");
    let expected_chunk = &transcript[expected_offset..];
    let cursor = format!("after:{revision}:{expected_offset}");
    let path = format!(
        "/api/mobile/sessions/thread-main/content?range=after&limit={TEST_CONTENT_CHUNK_LIMIT_BYTES}&cursor={cursor}"
    );

    let after_response = request_with_options(
        &router,
        Method::GET,
        &path,
        &[(axum::http::header::AUTHORIZATION, &authorization)],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;

    assert_eq!(after_response.status(), StatusCode::OK);
    let after_body = after_response
        .into_body()
        .collect()
        .await
        .expect("after body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&after_body).expect("after json");
    let chunk = &json["chunk"];

    assert_eq!(json["content_type"], "transcript");
    assert_eq!(
        json["supported_ranges"],
        serde_json::json!(["tail", "after"])
    );
    assert_eq!(chunk["account_id"], "local-account");
    assert_eq!(chunk["node_id"], "local-node");
    assert_eq!(chunk["session_id"], "thread-main");
    assert_eq!(chunk["revision"], revision);
    assert_eq!(chunk["offset"], expected_offset);
    assert_eq!(chunk["length"], expected_chunk.len());
    assert_eq!(chunk["sha256"], sha256_hex(expected_chunk));
    assert_eq!(
        chunk["content"].as_str().expect("content").as_bytes(),
        expected_chunk
    );
    assert_eq!(
        chunk["next_cursor"],
        format!("after:{revision}:{}", transcript.len())
    );
    assert!(chunk["merkle_root"].is_null());
    assert!(chunk["merkle_proof"].is_null());
}

#[tokio::test]
async fn mobile_session_content_rejects_stale_revision() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-content-stale.jsonl",
        &[serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "revision conflict"
                    }
                ]
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;

    let response = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main/content?range=tail&limit=65536&revision=stale-revision",
        &[(axum::http::header::AUTHORIZATION, &authorization)],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(json["code"], "stale_revision");
    assert_eq!(json["requested_revision"], "stale-revision");
    assert!(
        json["current_revision"]
            .as_str()
            .is_some_and(|revision| revision.starts_with("transcript:"))
    );
}

#[tokio::test]
async fn mobile_session_content_rejects_malformed_input() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let transcript_path = fixture.write_transcript(
        "thread-main-content-input.jsonl",
        &[serde_json::json!({
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "input guards"
                    }
                ]
            }
        })],
    );
    fixture.attach_transcript_path("thread-main", &transcript_path);
    let router = build_router(fixture.control_plane());
    let authorization = issue_mobile_authorization_header(&router).await;

    for (path, expected_code) in [
        (
            "/api/mobile/sessions/thread-main/content?range=after&limit=65536&cursor=not-a-cursor",
            "invalid_cursor",
        ),
        (
            "/api/mobile/sessions/thread-main/content?range=tail&limit=524289",
            "invalid_limit",
        ),
        (
            "/api/mobile/sessions/thread-main/content?range=search&limit=65536",
            "unsupported_range",
        ),
    ] {
        let response = request_with_options(
            &router,
            Method::GET,
            path,
            &[(axum::http::header::AUTHORIZATION, &authorization)],
            Some("127.0.0.1:49152".parse().expect("loopback socket")),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["code"], expected_code, "{path}");
    }
}

#[tokio::test]
async fn mobile_session_controls_are_owned_by_rust() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let default_prompt_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SaveDefaultPrompt(SaveDefaultPromptRequest {
            prompt: "Continue exactly from phone.".to_owned(),
            client_mutation_id: "mobile-controls-default-prompt".to_owned(),
        }),
    )
    .await;
    assert!(default_prompt_ack.accepted);
    let settings_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
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

    let assistant_surface_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetAssistantSurface(SetAssistantSurfaceRequest {
            assistant_surface: "zed".to_owned(),
            client_mutation_id: "mobile-controls-assistant-surface".to_owned(),
        }),
    )
    .await;
    assert!(assistant_surface_ack.accepted);
    assert_eq!(assistant_surface_ack.entity_id, "mobile-settings");
    let assistant_surface_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        assistant_surface_snapshot["globalSettings"]["assistantSurface"],
        "zed"
    );
    let codex_surface_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetAssistantSurface(SetAssistantSurfaceRequest {
            assistant_surface: "codex".to_owned(),
            client_mutation_id: "mobile-controls-assistant-surface-codex".to_owned(),
        }),
    )
    .await;
    assert!(codex_surface_ack.accepted);

    control_plane
        .mobile_session_service()
        .set_session_preset("thread-main", Some("max-turns-1"))
        .expect("set session mode through Rust state owner");

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

    control_plane
        .mobile_session_service()
        .set_session_preset("thread-main", Some("await-reply"))
        .expect("set waiting mode through Rust state owner");
    let waiting_detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(waiting_detail["effectiveMode"], "await-reply");
    assert_eq!(waiting_detail["status"], "waiting");

    prime_state_mini_cache(&control_plane);
    let resumed_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SendSessionPrompt(SendSessionPromptRequest {
            thread_id: "thread-main".to_owned(),
            prompt: "Resume from phone.".to_owned(),
            assistant_surface: String::new(),
            client_mutation_id: "mobile-controls-resume-prompt".to_owned(),
            prompt_intent: "queue".to_owned(),
        }),
    )
    .await;
    assert!(resumed_ack.accepted);
    assert_eq!(resumed_ack.entity_id, "thread-main");
    wait_for_mobile_event_detail(&control_plane, "thread-main", "prompt-queued").await;

    record_thread_active(&control_plane, "thread-main");
    prime_state_mini_cache(&control_plane);
    let prompt_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SendSessionPrompt(SendSessionPromptRequest {
            thread_id: "thread-main".to_owned(),
            prompt: "Keep going.".to_owned(),
            assistant_surface: String::new(),
            client_mutation_id: "mobile-controls-active-prompt".to_owned(),
            prompt_intent: "queue".to_owned(),
        }),
    )
    .await;
    assert!(prompt_ack.accepted);
    assert_eq!(prompt_ack.entity_id, "thread-main");

    let mute_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::MuteSession(MuteSessionRequest {
            thread_id: "thread-main".to_owned(),
            client_mutation_id: "mobile-controls-mute".to_owned(),
        }),
    )
    .await;
    assert!(mute_ack.accepted);
    let mute_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(
        mobile_snapshot_session(&mute_snapshot, "thread-main")["id"],
        "thread-main"
    );

    let siri_default_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSiriDefaultSession(SetSiriDefaultSessionRequest {
            thread_id: "thread-main".to_owned(),
            assistant_surface: "codex".to_owned(),
            client_mutation_id: "mobile-controls-siri-default".to_owned(),
        }),
    )
    .await;
    assert!(siri_default_ack.accepted);
    let siri_default_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
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

    let missing_siri_default = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSiriDefaultSession(SetSiriDefaultSessionRequest {
            thread_id: "missing-thread".to_owned(),
            assistant_surface: "codex".to_owned(),
            client_mutation_id: "mobile-controls-siri-missing".to_owned(),
        }),
    )
    .await;
    assert!(!missing_siri_default.accepted);

    let invalid_siri_default_surface = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSiriDefaultSession(SetSiriDefaultSessionRequest {
            thread_id: "thread-main".to_owned(),
            assistant_surface: "wrong".to_owned(),
            client_mutation_id: "mobile-controls-siri-invalid-surface".to_owned(),
        }),
    )
    .await;
    assert!(!invalid_siri_default_surface.accepted);

    let cleared_siri_default_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSiriDefaultSession(SetSiriDefaultSessionRequest {
            thread_id: String::new(),
            assistant_surface: String::new(),
            client_mutation_id: "mobile-controls-siri-clear".to_owned(),
        }),
    )
    .await;
    assert!(cleared_siri_default_ack.accepted);
    let cleared_siri_default_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
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

    let archived_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SetSessionArchived(SetSessionArchivedRequest {
            thread_id: "thread-main".to_owned(),
            archived: true,
            client_mutation_id: "mobile-controls-archive".to_owned(),
        }),
    )
    .await;
    assert!(archived_ack.accepted);
    let archived_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let archived_session = mobile_snapshot_session(&archived_snapshot, "thread-main");
    assert_eq!(archived_session["isArchived"], serde_json::json!(true));
    assert_eq!(archived_session["status"], "archived");

    let deleted_ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::DeleteSession(DeleteSessionRequest {
            thread_id: "thread-main".to_owned(),
            client_mutation_id: "mobile-controls-delete".to_owned(),
        }),
    )
    .await;
    assert!(deleted_ack.accepted);
    let deleted_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
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
}

#[tokio::test]
async fn mobile_snapshot_carries_non_default_surface_sessions_without_server_switch() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let grok_transcript = std::path::PathBuf::from("/Users/test/.grok/sessions/grok-thread.jsonl");
    fixture.attach_transcript_path("thread-main", &grok_transcript);
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
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

    let grok_session = mobile_surface_session(&codex_snapshot, "grok-build", "thread-main");
    assert_eq!(grok_session["assistantClient"], "grok-build");

    let hidden_detail = request_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(hidden_detail.status(), StatusCode::NOT_FOUND);

    let visible_detail = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/sessions/thread-main?assistantSurface=grok-build",
        &auth_headers,
        None,
    )
    .await;
    assert_eq!(visible_detail["assistantClient"], "grok-build");
}

#[tokio::test]
async fn desktop_mobile_state_uses_fresh_snapshot_session_projection() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let grok_transcript = std::path::PathBuf::from("/Users/test/.grok/sessions/grok-thread.jsonl");
    fixture.attach_transcript_path("thread-main", &grok_transcript);
    let router = build_router(fixture.control_plane());

    let mobile_state = request_json(&router, "/desktop/mobile-state").await;

    assert!(
        mobile_state["sessions"]
            .as_array()
            .expect("visible sessions")
            .iter()
            .all(|session| session["id"] != "thread-main")
    );
    let grok_session = mobile_surface_session(&mobile_state, "grok-build", "thread-main");
    assert_eq!(grok_session["assistantClient"], "grok-build");
    assert_eq!(
        mobile_state["lifecycle"]["thread-main"]["status"],
        grok_session["status"]
    );
    assert!(mobile_state["sessionOverrides"].is_object());
    assert!(mobile_state["storedLifecycle"].is_object());
    assert_eq!(
        mobile_state["freshness"]["source"],
        serde_json::json!("desktop-mobile-snapshot")
    );
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
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
    let devin_session = mobile_surface_session(&devin_snapshot, "devin", "thread-main");
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
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
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
    assert_eq!(claude_thread["title"], serde_json::Value::Null);
    assert_eq!(
        claude_thread["first_user_prompt"],
        "Build native Claude support"
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
    assert_eq!(grok_thread["first_user_prompt"], "Build Grok hooks");
}

#[tokio::test]
async fn desktop_snapshot_includes_devin_sessions() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());

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
    assert_eq!(devin_thread["first_user_prompt"], "Fix Devin support");
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
    let mobile_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let devin_session =
        mobile_surface_session(&mobile_snapshot, "devin", "devin:devin-cli:brindle-cadet");
    assert_eq!(devin_session["assistantClient"], "devin");
    assert_eq!(devin_session["assistantPreview"], "Hello from Devin");
}

#[tokio::test]
async fn mobile_snapshot_devin_surface_survives_menu_snapshot_limit() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    fixture.append_newer_than_devin_state_threads(EXTRA_MOBILE_SNAPSHOT_THREADS);
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let devin_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let devin_sessions = devin_snapshot["surfaceSessions"]["devin"]
        .as_array()
        .expect("devin surface sessions");

    assert_eq!(devin_sessions.len(), 1);
    assert_eq!(devin_sessions[0]["id"], "devin:devin-cli:brindle-cadet");
    assert_eq!(devin_sessions[0]["assistantClient"], "devin");
}

#[tokio::test]
async fn mobile_snapshot_lists_native_grok_sessions_on_grok_surface() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_grok_session("grok-session-1", "/tmp/project", "Ship Grok hooks");
    let control_plane = fixture.control_plane();
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let auth_headers = [(axum::http::header::AUTHORIZATION, authorization.as_str())];

    let grok_snapshot = request_json_with_options(
        &router,
        Method::GET,
        "/api/mobile/snapshot",
        &auth_headers,
        None,
    )
    .await;
    let grok_session = mobile_surface_session(&grok_snapshot, "grok-build", "grok-session-1");
    assert_eq!(grok_session["assistantClient"], "grok-build");
    assert_eq!(grok_session["title"], "Ship Grok hooks");
    assert_eq!(grok_snapshot["grokBuild"]["sessionCount"], 1);
    assert_eq!(grok_snapshot["grokBuild"]["activeSessionCount"], 1);
    assert!(grok_snapshot["grokBuild"]["hooks"]["health"].is_string());
}

#[tokio::test]
async fn grpc_health_allows_no_mobile_auth() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let (_server, mut client) = spawn_grpc_client(control_plane).await;

    let response = client
        .health(HealthRequest {})
        .await
        .expect("health")
        .into_inner();

    assert!(response.ok);
    assert_eq!(response.service, "looper-realtime");
}

#[tokio::test]
async fn grpc_session_allows_loopback_without_mobile_auth() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    let (_server, mut client) = spawn_grpc_client(control_plane).await;
    let request = tonic::Request::new(tokio_stream::iter(Vec::<ClientFrame>::new()));

    client
        .session(request)
        .await
        .expect("loopback Session stream should not require mobile auth");
}

#[tokio::test]
async fn grpc_mobile_prompt_records_prompt_resumed_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");

    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;
    let (_server, mut client) = spawn_grpc_client(control_plane.clone()).await;
    prime_state_mini_cache(&control_plane);

    let command_frame = ClientFrame {
        frame: Some(client_frame::Frame::Command(Command {
            command: Some(command::Command::SendSessionPrompt(
                SendSessionPromptRequest {
                    thread_id: "thread-main".to_owned(),
                    prompt: "Keep going from gRPC.".to_owned(),
                    assistant_surface: String::new(),
                    client_mutation_id: "grpc-prompt-records-event-1".to_owned(),
                    prompt_intent: "steer".to_owned(),
                },
            )),
        })),
    };
    let mut request = tonic::Request::new(tokio_stream::iter(vec![command_frame]));
    request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );

    let mut stream = client
        .session(request)
        .await
        .expect("Session stream")
        .into_inner();
    let ack = stream
        .message()
        .await
        .expect("session ACK result")
        .expect("session ACK frame");

    match ack.frame {
        Some(server_frame::Frame::Ack(ack)) => {
            assert!(ack.accepted);
            assert_eq!(ack.client_mutation_id, "grpc-prompt-records-event-1");
        }
        other => panic!("expected Session ACK frame, got {other:?}"),
    }

    for _ in 0..80 {
        let events = control_plane
            .store()
            .mobile_events_since(0, 32)
            .expect("mobile events");
        if events.iter().any(|event| {
            event.thread_id.as_deref() == Some("thread-main")
                && event.event_type == MobileEventKind::SessionChanged
                && event.detail.as_deref() == Some("prompt-resumed")
        }) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for prompt-resumed event");
}

#[tokio::test]
async fn codex_mobile_prompt_records_prompt_resumed_event() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let control_plane = fixture.control_plane();
    record_thread_active(&control_plane, "thread-main");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    prime_state_mini_cache(&control_plane);
    let ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SendSessionPrompt(SendSessionPromptRequest {
            thread_id: "thread-main".to_owned(),
            prompt: "Keep going from phone.".to_owned(),
            assistant_surface: String::new(),
            client_mutation_id: "codex-mobile-prompt-resumed-event".to_owned(),
            prompt_intent: "steer".to_owned(),
        }),
    )
    .await;
    assert!(ack.accepted);

    wait_for_mobile_event_detail(&control_plane, "thread-main", "prompt-resumed").await;
}

#[tokio::test]
async fn devin_mobile_prompt_queues_prompt_for_local_devin_hook_delivery() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
        .set_session_preset("devin:devin-cli:brindle-cadet", Some("await-reply"))
        .expect("set Devin session mode");
    record_thread_active(&control_plane, "devin:devin-cli:brindle-cadet");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    seed_replyable_session_mini_for_thread(
        &control_plane,
        "devin:devin-cli:brindle-cadet",
        "devin",
        "devin-local-hook-mini",
        4,
    );
    let ack = submit_grpc_session_command(
        control_plane.clone(),
        &authorization,
        command::Command::SendSessionPrompt(SendSessionPromptRequest {
            thread_id: "devin:devin-cli:brindle-cadet".to_owned(),
            prompt: "Keep going from phone.".to_owned(),
            assistant_surface: "devin".to_owned(),
            client_mutation_id: "devin-mobile-prompt-queue".to_owned(),
            prompt_intent: "queue".to_owned(),
        }),
    )
    .await;
    assert!(ack.accepted);
    assert_eq!(ack.entity_id, "devin:devin-cli:brindle-cadet");

    let queued_prompt_id =
        wait_for_prompt_queued(&control_plane, "devin:devin-cli:brindle-cadet").await;

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
        Some(queued_prompt_id.as_str())
    );
}

#[tokio::test]
async fn devin_mobile_prompt_rejects_without_hot_local_devin_delivery_cache() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    fixture.write_devin_next_session();
    let control_plane = fixture.control_plane();
    control_plane
        .mobile_session_service()
        .set_session_preset("devin:devin-cli:brindle-cadet", Some("await-reply"))
        .expect("set Devin session mode");
    let router = build_router(control_plane.clone());
    let authorization = issue_mobile_authorization_header(&router).await;

    prime_state_mini_cache(&control_plane);
    let ack = submit_grpc_session_command(
        control_plane,
        &authorization,
        command::Command::SendSessionPrompt(SendSessionPromptRequest {
            thread_id: "devin:devin-cli:brindle-cadet".to_owned(),
            prompt: "Keep going from phone.".to_owned(),
            assistant_surface: "devin".to_owned(),
            client_mutation_id: "devin-mobile-prompt-stopped".to_owned(),
            prompt_intent: "queue".to_owned(),
        }),
    )
    .await;

    assert!(!ack.accepted);
    assert_eq!(ack.error_code, "failed_precondition");
    assert!(
        ack.reject_reason
            .contains("prompt delivery action cache is cold")
    );
}

#[tokio::test]
async fn http_session_state_mutation_routes_are_disabled() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db();
    let router = build_router(fixture.control_plane());
    let loopback = Some("127.0.0.1:49153".parse().expect("loopback socket"));
    let snapshot_before_disabled_routes = request_json(&router, "/desktop/snapshot").await;

    let disabled_routes = vec![
        (
            Method::POST,
            "/desktop/settings/default-notification-targets",
            Some(serde_json::json!({
                "notificationTargetIds": ["macos"]
            })),
        ),
        (
            Method::POST,
            "/desktop/settings/default-prompt",
            Some(serde_json::json!({ "defaultPrompt": "Continue from TUI." })),
        ),
        (
            Method::POST,
            "/desktop/settings/scope",
            Some(serde_json::json!({ "scope": "per-task" })),
        ),
        (
            Method::POST,
            "/desktop/settings/assistant-surface",
            Some(serde_json::json!({ "assistantSurface": "devin" })),
        ),
        (
            Method::POST,
            "/desktop/settings/global-preset",
            Some(serde_json::json!({ "preset": "await-reply" })),
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
    ];

    for (method, path, body) in disabled_routes {
        let response = match body {
            Some(body) => {
                request_with_body_options(
                    &router,
                    method,
                    path,
                    serde_json::to_vec(&body).expect("json body"),
                    &[(axum::http::header::CONTENT_TYPE, "application/json")],
                    loopback,
                )
                .await
            }
            None => request_with_options(&router, method, path, &[], loopback).await,
        };
        assert_eq!(response.status(), StatusCode::GONE, "{path}");
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let payload: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload["error"], "http_session_state_mutation_disabled");
        assert_eq!(payload["recovery"], "/api/mobile/session-minis/snapshot");
    }

    let snapshot_after = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(
        snapshot_after["thread_count"],
        snapshot_before_disabled_routes["thread_count"]
    );
    assert_eq!(
        snapshot_after["active_thread_count"],
        snapshot_before_disabled_routes["active_thread_count"]
    );
    assert_eq!(
        snapshot_after["revision"], snapshot_before_disabled_routes["revision"],
        "disabled HTTP routes must not change session state"
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
    assert_eq!(
        desktop_push_devices["devices"][0]["canTest"],
        serde_json::json!(false)
    );

    let desktop_test_response = request_json_body_with_options(
        &router,
        Method::POST,
        "/desktop/push/devices/install-1/test",
        serde_json::json!({}),
        &[],
        Some("127.0.0.1:49152".parse().expect("loopback socket")),
    )
    .await;
    assert_eq!(desktop_test_response["delivered"], serde_json::json!(false));
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

    assert_eq!(test_response["delivered"], serde_json::json!(false));
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

fn session_mini_snapshot_session<'a>(
    snapshot: &'a serde_json::Value,
    session_id: &str,
) -> &'a serde_json::Value {
    snapshot["sessions"]
        .as_array()
        .expect("session minis")
        .iter()
        .find(|session| session["id"] == session_id)
        .expect("session mini")
}

fn session_mini_snapshot_has_session(snapshot: &serde_json::Value, session_id: &str) -> bool {
    snapshot["sessions"]
        .as_array()
        .expect("session minis")
        .iter()
        .any(|session| session["id"] == session_id)
}

fn mobile_surface_has_session(
    snapshot: &serde_json::Value,
    surface: &str,
    session_id: &str,
) -> bool {
    snapshot["surfaceSessions"][surface]
        .as_array()
        .expect("surface sessions")
        .iter()
        .any(|session| session["id"] == session_id)
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

fn record_thread_stopped(control_plane: &ControlPlane, thread_id: &str) {
    control_plane
        .mobile_session_service()
        .record_hook_lifecycle(
            &MobileHookPayload {
                hook_event_name: "Stop".to_owned(),
                session_id: Some(thread_id.to_owned()),
                turn_id: None,
                cwd: None,
                last_assistant_message: None,
            },
            false,
        )
        .expect("record stopped mobile lifecycle");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn prime_state_mini_cache(control_plane: &ControlPlane) {
    control_plane
        .reconcile_mobile_session_mini_projection()
        .expect("reconcile state minis");
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

async fn submit_grpc_session_command(
    control_plane: ControlPlane,
    authorization: &str,
    command: command::Command,
) -> agent_control_plane::grpc::proto::CommandAck {
    let (_server, mut client) = spawn_grpc_client(control_plane).await;
    let command_frame = ClientFrame {
        frame: Some(client_frame::Frame::Command(Command {
            command: Some(command),
        })),
    };
    let mut request = tonic::Request::new(tokio_stream::iter(vec![command_frame]));
    request.metadata_mut().insert(
        "authorization",
        authorization.parse().expect("authorization metadata"),
    );
    let mut stream = client
        .session(request)
        .await
        .expect("Session command stream")
        .into_inner();
    for _ in 0..SESSION_COMMAND_ACK_POLL_LIMIT {
        let frame = stream
            .message()
            .await
            .expect("Session command frame result")
            .expect("Session command ACK frame");
        if let Some(server_frame::Frame::Ack(ack)) = frame.frame {
            return ack;
        }
    }

    panic!("expected Session ACK frame before stream ended")
}

async fn wait_for_mobile_event_detail(control_plane: &ControlPlane, thread_id: &str, detail: &str) {
    for _ in 0..80 {
        let found = control_plane
            .store()
            .mobile_events_since(0, 1_000)
            .expect("mobile events")
            .iter()
            .any(|event| {
                event.thread_id.as_deref() == Some(thread_id)
                    && event.detail.as_deref() == Some(detail)
            });
        if found {
            return;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {detail} mobile event for {thread_id}");
}

async fn wait_for_session_mini_absent(control_plane: &ControlPlane, session_id: &str) {
    for _ in 0..80 {
        let is_absent = control_plane
            .store()
            .mobile_session_minis()
            .expect("mobile session minis")
            .iter()
            .all(|record| record.session_id != session_id);
        if is_absent {
            return;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {session_id} session mini to be absent");
}

async fn wait_for_prompt_queued(control_plane: &ControlPlane, thread_id: &str) -> String {
    for _ in 0..80 {
        if let Some(event) = control_plane
            .store()
            .mobile_events_since(0, 1_000)
            .expect("mobile events")
            .iter()
            .find(|event| {
                event.thread_id.as_deref() == Some(thread_id)
                    && event.event_type == MobileEventKind::PromptQueued
            })
        {
            return event.prompt_id.clone().expect("queued prompt id");
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for prompt queued event for {thread_id}");
}

fn seed_replyable_session_mini_for_thread(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: &str,
    revision: &str,
    seq: i64,
) {
    control_plane
        .store()
        .replace_mobile_session_minis(
            vec![MobileSessionMiniProjectionInput {
                session_id: thread_id.to_owned(),
                assistant_surface: assistant_surface.to_owned(),
                body_json: serde_json::json!({
                    "sessionId": thread_id,
                    "assistantSurface": assistant_surface,
                    "effectiveMode": "await-reply",
                    "replyable": true,
                    "canSendPrompt": true,
                }),
            }],
            seq,
            revision,
        )
        .expect("seed replyable session mini");
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

    fn slow_codex_resume_stub(&self) -> std::path::PathBuf {
        let executable = self.temp_dir.path().join("codex-resume-slow-stub");
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
      sleep 3
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
        .expect("write slow codex resume stub");
        let mut permissions = fs::metadata(&executable)
            .expect("slow codex resume stub metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("chmod slow codex resume stub");
        executable
    }

    fn control_plane(&self) -> ControlPlane {
        self.control_plane_with_zed_processes(Vec::new())
    }

    fn control_plane_with_codex_executable(
        &self,
        codex_executable: std::path::PathBuf,
    ) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: self.codex_home.clone(),
            codex_executable: Some(codex_executable.display().to_string()),
            grok_home: self.grok_home(),
            store_path: self.temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
            home_path: self.temp_dir.path().to_path_buf(),
            zed_process_commands: Some(Vec::new()),
        })
    }

    fn control_plane_with_running_zed(&self) -> ControlPlane {
        self.control_plane_with_zed_processes(vec![
            "/Applications/Zed.app/Contents/MacOS/zed --foreground".to_owned(),
        ])
    }

    fn control_plane_with_zed_processes(&self, zed_process_commands: Vec<String>) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: self.codex_home.clone(),
            codex_executable: Some(self.codex_resume_stub().display().to_string()),
            grok_home: self.grok_home(),
            store_path: self.temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
            home_path: self.temp_dir.path().to_path_buf(),
            zed_process_commands: Some(zed_process_commands),
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
        self.write_zed_settings_value(serde_json::json!({
            "agent_servers": {
                "looper": {
                    "type": "custom",
                    "command": "looper",
                    "args": ["acp", "stdio", "zed"],
                    "env": {
                        "TOKEN": "zed-secret-token"
                    }
                }
            }
        }));
    }

    fn write_zed_settings_without_command(&self) {
        self.write_zed_settings_value(serde_json::json!({
            "agent_servers": {
                "looper": {
                    "type": "custom",
                    "args": ["acp", "stdio", "zed"],
                    "env": {
                        "TOKEN": "zed-secret-token"
                    }
                }
            }
        }));
    }

    fn write_zed_settings_value(&self, settings: serde_json::Value) {
        let settings_path = self.temp_dir.path().join(".zed/settings.json");
        fs::create_dir_all(settings_path.parent().expect("zed settings parent"))
            .expect("create zed settings parent");
        fs::write(settings_path, settings.to_string()).expect("write zed settings");
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
                        "sessionUpdate": "user_message_chunk",
                        "content": {
                            "type": "text",
                            "text": "Fix "
                        },
                        "_meta": {
                            "cognition.ai/streamingMessageId": "user-1"
                        }
                    }
                })
                .to_string(),
                serde_json::json!({
                    "providerId": "devin-cli",
                    "notification": {
                        "sessionUpdate": "user_message_chunk",
                        "content": {
                            "type": "text",
                            "text": "Devin support"
                        },
                        "_meta": {
                            "cognition.ai/streamingMessageId": "user-1"
                        }
                    }
                })
                .to_string(),
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
                            "eventCount": 5,
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
            session_dir.join("chat_history.jsonl"),
            [
                serde_json::json!({
                    "type": "user",
                    "content": "Build Grok hooks"
                })
                .to_string(),
                serde_json::json!({
                    "type": "assistant",
                    "content": "Grok hooks are visible."
                })
                .to_string(),
            ]
            .join("\n"),
        )
        .expect("write grok chat history");
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

// ── Delivery-action resolution micro-benchmark ────────────────────────────────
//
// Times the two paths that `resolve_delivery_action` can take:
//
//   MISS  – `mobile_desktop_snapshot` (SQLite + file scan) +
//            `mobile_session_service().state()` (SQLite) +
//            `prompt_delivery_action_for_target` (pure in-memory)
//
//   HIT   – a single `HashMap::get + clone`  (reproduces the exact work
//            inside `delivery_action_cache().lock().unwrap().get(...).cloned()`
//            which is private, so we replicate it locally)
//
// Run with:
//   cargo test --manifest-path crates/agent-control-plane/Cargo.toml \
//       bench_delivery_action_miss_vs_hit -- --nocapture --ignored
#[test]
#[ignore]
fn bench_delivery_action_miss_vs_hit() {
    use std::collections::HashMap;
    use std::time::Instant;

    use agent_control_plane::mobile::api::{
        PromptDeliveryAction, PromptResumeTarget, prompt_delivery_action_for_target,
    };

    const ITERATIONS: u32 = 1_000;
    const THREAD_ID: &str = "thread-main";

    // ── Setup ──────────────────────────────────────────────────────────────
    let fixture = IsolatedCodexFixture::new();
    fixture.write_state_db(); // seeds thread-main + thread-child in SQLite
    let control_plane = fixture.control_plane();

    // Warm up: one full miss so SQLite page cache is hot.
    {
        let snapshot = control_plane
            .desktop_mobile_snapshot()
            .expect("warm-up snapshot");
        let session_state = control_plane
            .mobile_session_service()
            .state()
            .expect("warm-up state");
        let _ = prompt_delivery_action_for_target(&snapshot, &session_state, THREAD_ID);
    }

    // ── MISS path benchmark ────────────────────────────────────────────────
    let mut miss_us: Vec<u64> = Vec::with_capacity(ITERATIONS as usize);
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let snapshot = control_plane.desktop_mobile_snapshot().expect("snapshot");
        let session_state = control_plane
            .mobile_session_service()
            .state()
            .expect("state");
        let _ = prompt_delivery_action_for_target(&snapshot, &session_state, THREAD_ID)
            .expect("action");
        miss_us.push(t0.elapsed().as_micros() as u64);
    }
    miss_us.sort_unstable();
    let miss_mean_us = miss_us.iter().sum::<u64>() / miss_us.len() as u64;
    let miss_median_us = miss_us[miss_us.len() / 2];
    let miss_p99_us = miss_us[(miss_us.len() * 99) / 100];

    // ── HIT path benchmark (HashMap::get + clone) ─────────────────────────
    let cached_action = PromptDeliveryAction::ResumeCodex(PromptResumeTarget {
        thread_id: THREAD_ID.to_owned(),
        cwd: None,
    });
    let mut cache: HashMap<String, PromptDeliveryAction> = HashMap::new();
    cache.insert(THREAD_ID.to_owned(), cached_action);

    let mut hit_us: Vec<u64> = Vec::with_capacity(ITERATIONS as usize);
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let _action: Option<PromptDeliveryAction> = cache.get(THREAD_ID).cloned();
        hit_us.push(t0.elapsed().as_micros() as u64);
    }
    hit_us.sort_unstable();
    let hit_mean_us = hit_us.iter().sum::<u64>() / hit_us.len() as u64;
    let hit_median_us = hit_us[hit_us.len() / 2];
    let hit_p99_us = hit_us[(hit_us.len() * 99) / 100];

    // ── Report ─────────────────────────────────────────────────────────────
    let delta_median_us = miss_median_us.saturating_sub(hit_median_us);
    println!();
    println!("=== delivery-action resolution: MISS vs HIT (N={ITERATIONS}) ===");
    println!("MISS  median={miss_median_us}µs  mean={miss_mean_us}µs  p99={miss_p99_us}µs");
    println!("HIT   median={hit_median_us}µs   mean={hit_mean_us}µs   p99={hit_p99_us}µs");
    println!("DELTA (miss-hit) median={delta_median_us}µs");
    println!("LAN RTT reference: ~300-2000µs");
    println!("=================================================================");
}

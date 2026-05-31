use std::collections::BTreeMap;
use std::fs;

use agent_control_plane::assistant::{
    AssistantKind, AssistantRuntimeKind, discover_assistant_adapters_from_processes,
    discover_assistant_adapters_from_sources,
};
use agent_control_plane::auth::{
    AuthManager, CloudAuthContract, LinkedIdentityMethod, MemorySecretStore,
};
use agent_control_plane::control_plane::{ControlPlane, ControlPlaneConfig};
use agent_control_plane::http::build_router;
use agent_control_plane::scheduler::AutomationRunner;
use axum::body::Body;
use axum::http::Method;
use http_body_util::BodyExt;
use rusqlite::Connection;
use tempfile::TempDir;
use tower::ServiceExt;

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

    let duplicate = runner.tick(1_000_000).expect("dedupe tick");
    assert!(duplicate.is_empty());

    let snapshot = request_json(&router, "/desktop/snapshot").await;
    assert_eq!(snapshot["control_plane"]["hooks"]["owner"], "looper-rust");
    assert_eq!(snapshot["thread_count"], 2);
    assert_eq!(snapshot["active_thread_count"], 2);
    assert!(snapshot["control_plane"]["codex_servers"].is_array());
    assert_eq!(snapshot["automations"][0]["id"], "daily-review");
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
async fn degraded_source_is_reported_without_touching_user_state() {
    let fixture = IsolatedCodexFixture::new();
    fixture.write_hooks_json("bun src/bun/managed-hook-script.ts");
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
async fn nested_codex_hooks_json_shape_is_supported() {
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
    assert!(config_toml.contains("codex_hooks = true"));

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
    assert_eq!(response["installed_handlers"], 3);
    assert_eq!(response["hooks_auto_registration"], true);
    assert_eq!(response["status"]["hooks"]["enabled"], true);
    assert_eq!(response["status"]["hooks"]["owner"], "looper-rust");

    let hooks_json = fs::read_to_string(fixture.codex_home.join("hooks.json")).expect("hooks");
    assert!(hooks_json.contains("agent-control-plane --hook --managed-by looper"));
    assert!(hooks_json.contains("SessionStart"));
    assert!(hooks_json.contains("Stop"));
    assert!(hooks_json.contains("UserPromptSubmit"));
    assert!(hooks_json.contains("/usr/local/bin/custom-user-hook"));

    let config_toml = fs::read_to_string(fixture.codex_home.join("config.toml")).expect("config");
    assert!(config_toml.contains("[features]"));
    assert!(config_toml.contains("codex_hooks = true"));
    assert!(config_toml.contains("[model]"));
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
    assert_eq!(register_response["installed_handlers"], 3);
    assert_eq!(register_response["status"]["hooks"]["owner"], "looper-rust");
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
        "/Applications/Superconductor.app/Contents/MacOS/Superconductor --host".to_owned(),
        "/Applications/Cursor.app/Contents/MacOS/Cursor --type=renderer".to_owned(),
        "/opt/homebrew/bin/opencode run --json".to_owned(),
    ]);

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
}

#[test]
fn assistant_adapters_separate_cli_installed_from_running() {
    let mut cli_paths = BTreeMap::new();
    cli_paths.insert(
        "cursor".to_owned(),
        "/Users/test/.local/bin/cursor".to_owned(),
    );
    cli_paths.insert(
        "opencode".to_owned(),
        "/Users/test/.opencode/bin/opencode".to_owned(),
    );

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
}

async fn request_json(router: &axum::Router, path: &str) -> serde_json::Value {
    request_json_with_method(router, Method::GET, path).await
}

async fn request_json_with_method(
    router: &axum::Router,
    method: Method,
    path: &str,
) -> serde_json::Value {
    let response = router
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&body).expect("json")
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
        Self {
            temp_dir,
            codex_home,
        }
    }

    fn control_plane(&self) -> ControlPlane {
        ControlPlane::new(ControlPlaneConfig {
            codex_home: self.codex_home.clone(),
            store_path: self.temp_dir.path().join("control-plane.sqlite"),
            hook_command: Some("agent-control-plane --hook --managed-by looper".to_owned()),
        })
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
                                "command": "bun src/bun/managed-hook-script.ts",
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
            format!("codex_hooks = {enabled}\n"),
        )
        .expect("write config");
    }

    fn write_config_toml_with_model_block(&self) {
        fs::write(
            self.codex_home.join("config.toml"),
            "[model]\ndefault = \"gpt-5.5\"\n",
        )
        .expect("write config");
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

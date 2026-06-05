mod args;
mod doctor;
mod output;
mod server;
mod transport;

use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use serde_json::Value;

use self::args::{
    WAIT_FOR_REPLY_FLAG, WAIT_FOR_UPDATES_FLAG, joined_args, nullable_id, parse_global_args,
    split_csv, take_arg,
};
use self::doctor::run_doctor_command;
use self::output::{OutputFormat, print_value};
use self::server::ensure_server_ready;
use self::transport::{delete_json, fetch_json, patch_json, post_json, print_get};

const SEND_ALL_TARGET: &str = "--all";
const SEND_ACTIVE_TARGET: &str = "active";
const WAIT_ARCHIVED_FLAG: &str = "--archived";
const WAIT_GONE_FLAG: &str = "--gone";
const WAIT_TIMEOUT_FLAG: &str = "--timeout";
const WAIT_DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(500);

pub async fn run() -> Result<()> {
    let (format, mut args) = parse_global_args(std::env::args().skip(1).collect());
    let Some(command) = take_arg(&mut args) else {
        return run_terminal_command().await;
    };

    if command_requires_server(&command) {
        ensure_server_ready().await?;
    }

    match command.as_str() {
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "serve" => crate::runtime::run_server().await,
        "status" => print_get("/status/control-plane", format).await,
        "snapshot" => print_get("/desktop/snapshot", format).await,
        "shutdown" => post_json("/desktop/shutdown", Value::Null, format).await,
        "attach" => run_attach_command(&args).await,
        "send" => run_send_command(&args, format).await,
        "wait" => run_wait_command(&args, format).await,
        "detach" => run_detach_command(format).await,
        "hooks" => run_hooks_command(&args, format).await,
        "connections" => run_connections_command(&args, format).await,
        "devin" => run_devin_command(&args, format).await,
        "pairing" => run_pairing_command(&args, format).await,
        "settings" => run_settings_command(&args, format).await,
        "sessions" => run_sessions_command(&args, format).await,
        "notifications" => run_notifications_command(&args, format).await,
        "checks" => run_checks_command(&args, format).await,
        "push" => run_push_command(&args, format).await,
        "doctor" => run_doctor_command(&args, format).await,
        "menubar" => run_menubar_command(&args),
        _ => {
            print_usage();
            bail!("unknown command: {command}");
        }
    }
}

fn command_requires_server(command: &str) -> bool {
    matches!(
        command,
        "status"
            | "snapshot"
            | "shutdown"
            | "attach"
            | "send"
            | "wait"
            | "detach"
            | "hooks"
            | "connections"
            | "devin"
            | "pairing"
            | "settings"
            | "sessions"
            | "notifications"
            | "checks"
            | "push"
            | "doctor"
    )
}

fn run_menubar_command(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("launch") => {
            std::process::Command::new("open")
                .args(["-a", "looper"])
                .status()?;
            Ok(())
        }
        _ => bail!("usage: looper menubar launch"),
    }
}

async fn run_devin_command(args: &[String], format: OutputFormat) -> Result<()> {
    ensure_server_ready().await?;
    match args.first().map(String::as_str) {
        Some("status") | Some("show") | None => print_get("/desktop/devin", format).await,
        Some("bridge") | Some("acp") | Some("agents") if args.get(1).is_none() => {
            print_get("/desktop/devin/acp-bridge", format).await
        }
        Some("probe") => run_devin_probe_command(args.get(1), format).await,
        Some("bridge") | Some("acp") | Some("agents")
            if args.get(1).map(String::as_str) == Some("probe") =>
        {
            run_devin_probe_command(args.get(2), format).await
        }
        _ => bail!("usage: looper devin [status|show|bridge|acp|agents|probe [agent-id]]"),
    }
}

async fn run_devin_probe_command(agent_id: Option<&String>, format: OutputFormat) -> Result<()> {
    post_json(
        "/desktop/devin/acp-bridge/probe",
        devin_probe_body(agent_id.map(String::as_str)),
        format,
    )
    .await
}

fn devin_probe_body(agent_id: Option<&str>) -> Value {
    match agent_id {
        Some(agent_id) => serde_json::json!({ "agentId": agent_id }),
        None => serde_json::json!({}),
    }
}

async fn run_hooks_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("register") => post_json("/hooks/register", Value::Null, format).await,
        Some("clear") => post_json("/hooks/clear", Value::Null, format).await,
        Some("unregister-live") => post_json("/hooks/unregister-live", Value::Null, format).await,
        _ => bail!("usage: looper hooks [register|clear|unregister-live]"),
    }
}

async fn run_connections_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => print_get("/desktop/connections", format).await,
        Some("rename") if args.len() >= 3 => {
            let body = serde_json::json!({ "label": joined_args(&args[2..]) });
            patch_json(
                &format!("/desktop/connections/mobile/{}", args[1]),
                body,
                format,
            )
            .await
        }
        Some("revoke") if args.len() >= 2 => {
            delete_json(&format!("/desktop/connections/mobile/{}", args[1]), format).await
        }
        _ => bail!("usage: looper connections [list|rename <id> <label>|revoke <id>]"),
    }
}

async fn run_pairing_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("code") | Some("show") => print_get("/desktop/pairing", format).await,
        Some("orb") => {
            let pairing = fetch_json("/desktop/pairing").await?;
            println!(
                "{}",
                pairing
                    .get("orbImageURL")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            );
            Ok(())
        }
        _ => bail!("usage: looper pairing [code|show|orb]"),
    }
}

async fn run_settings_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("get") => print_get("/desktop/mobile-state", format).await,
        Some("default-prompt") if args.len() >= 2 => {
            post_json(
                "/desktop/settings/default-prompt",
                serde_json::json!({ "defaultPrompt": joined_args(&args[1..]) }),
                format,
            )
            .await
        }
        Some("scope") if args.len() >= 2 => {
            post_json(
                "/desktop/settings/scope",
                serde_json::json!({ "scope": args[1] }),
                format,
            )
            .await
        }
        Some("global-preset") if args.len() >= 2 => {
            post_json(
                "/desktop/settings/global-preset",
                serde_json::json!({ "preset": nullable_id(&args[1]) }),
                format,
            )
            .await
        }
        Some("global-notification") if args.len() >= 2 => {
            post_json(
                "/desktop/settings/global-notification",
                serde_json::json!({ "notificationId": nullable_id(&args[1]) }),
                format,
            )
            .await
        }
        Some("global-completion-check") if args.len() >= 2 => post_json(
            "/desktop/settings/global-completion-check",
            serde_json::json!({
                "completionCheckId": nullable_id(&args[1]),
                "waitForReplyAfterCompletion": args.iter().any(|arg| arg == WAIT_FOR_REPLY_FLAG),
            }),
            format,
        )
        .await,
        _ => bail!(
            "usage: looper settings [get|default-prompt <text>|scope <global|per-task>|global-preset <preset|off>|global-notification <id|off>|global-completion-check <id|off>]"
        ),
    }
}

async fn run_sessions_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        None => print_get("/desktop/snapshot", format).await,
        Some("list") => print_get("/desktop/snapshot", format).await,
        Some("show") if args.len() >= 2 => {
            print_get(&format!("/desktop/sessions/{}", args[1]), format).await
        }
        Some("mode") if args.len() >= 3 => {
            post_json(
                &format!("/desktop/sessions/{}/mode", args[1]),
                serde_json::json!({ "preset": nullable_id(&args[2]) }),
                format,
            )
            .await
        }
        Some("archive") if args.len() >= 2 => session_archive(&args[1], true, format).await,
        Some("unarchive") if args.len() >= 2 => session_archive(&args[1], false, format).await,
        Some("delete") if args.len() >= 2 => {
            delete_json(&format!("/desktop/sessions/{}", args[1]), format).await
        }
        Some("mute") if args.len() >= 2 => {
            post_json(
                &format!("/desktop/sessions/{}/mute", args[1]),
                Value::Null,
                format,
            )
            .await
        }
        Some("prompt") if args.len() >= 3 => {
            post_json(
                &format!("/desktop/sessions/{}/prompt", args[1]),
                serde_json::json!({ "prompt": joined_args(&args[2..]) }),
                format,
            )
            .await
        }
        Some("prompt-mode") if args.len() >= 4 => {
            post_session_prompts(
                vec![args[1].clone()],
                &joined_args(&args[3..]),
                Some(args[2].as_str()),
                format,
            )
            .await
        }
        Some("prompt-active") | Some("prompt-all") if args.len() >= 2 => {
            let thread_ids = active_thread_ids().await?;
            post_session_prompts(thread_ids, &joined_args(&args[1..]), None, format).await
        }
        Some("prompt-active-mode") | Some("prompt-all-mode") if args.len() >= 3 => {
            let thread_ids = active_thread_ids().await?;
            post_session_prompts(
                thread_ids,
                &joined_args(&args[2..]),
                Some(args[1].as_str()),
                format,
            )
            .await
        }
        Some("notifications") if args.len() >= 3 => {
            post_json(
                &format!("/desktop/sessions/{}/notifications", args[1]),
                serde_json::json!({ "notificationIds": split_csv(&args[2]) }),
                format,
            )
            .await
        }
        Some("completion-check") if args.len() >= 3 => post_json(
            &format!("/desktop/sessions/{}/completion-check", args[1]),
            serde_json::json!({
                "completionCheckId": nullable_id(&args[2]),
                "waitForReplyAfterCompletion": args.iter().any(|arg| arg == WAIT_FOR_REPLY_FLAG),
            }),
            format,
        )
        .await,
        _ => bail!(
            "usage: looper sessions [list|show <id>|mode <id> <preset|off>|archive <id>|unarchive <id>|delete <id>|mute <id>|prompt <id> <text>|prompt-mode <id> <preset> <text>|prompt-active <text>|prompt-active-mode <preset> <text>|notifications <id> <ids>|completion-check <id> <check|off>]"
        ),
    }
}

async fn run_notifications_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => print_get("/desktop/mobile-state", format).await,
        Some("create-slack") if args.len() >= 3 => {
            post_json(
                "/desktop/notifications",
                serde_json::json!({
                    "label": args[1],
                    "channel": "slack",
                    "webhookUrl": args[2],
                }),
                format,
            )
            .await
        }
        Some("create-telegram") if args.len() >= 4 => {
            post_json(
                "/desktop/notifications",
                serde_json::json!({
                    "label": args[1],
                    "channel": "telegram",
                    "botToken": args[2],
                    "chatId": args[3],
                    "chatUsername": args.get(4),
                    "chatDisplayName": args.get(5),
                }),
                format,
            )
            .await
        }
        Some("delete") if args.len() >= 2 => {
            delete_json(&format!("/desktop/notifications/{}", args[1]), format).await
        }
        Some("telegram-chats") if args.len() >= 2 => {
            post_json(
                "/desktop/telegram/chats",
                serde_json::json!({
                    "botToken": args[1],
                    "waitForUpdates": args.iter().any(|arg| arg == WAIT_FOR_UPDATES_FLAG),
                }),
                format,
            )
            .await
        }
        _ => bail!(
            "usage: looper notifications [list|create-slack <label> <webhook-url>|create-telegram <label> <bot-token> <chat-id> [username] [display-name]|delete <id>|telegram-chats <bot-token> [--wait]]"
        ),
    }
}

async fn run_checks_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("list") => print_get("/desktop/mobile-state", format).await,
        Some("create") if args.len() >= 3 => {
            post_json(
                "/desktop/completion-checks",
                serde_json::json!({
                    "label": args[1],
                    "commands": [joined_args(&args[2..])],
                }),
                format,
            )
            .await
        }
        Some("delete") if args.len() >= 2 => {
            delete_json(&format!("/desktop/completion-checks/{}", args[1]), format).await
        }
        _ => bail!("usage: looper checks [list|create <label> <command>|delete <id>]"),
    }
}

async fn run_push_command(args: &[String], format: OutputFormat) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("devices") => print_get("/desktop/push/devices", format).await,
        Some("test") if args.len() >= 2 => {
            post_json(
                &format!("/desktop/push/devices/{}/test", args[1]),
                Value::Null,
                format,
            )
            .await
        }
        _ => bail!("usage: looper push [devices|test <installation-id>]"),
    }
}

async fn run_terminal_command() -> Result<()> {
    ensure_server_ready().await?;
    crate::tui::run().await
}

async fn run_attach_command(args: &[String]) -> Result<()> {
    ensure_server_ready().await?;
    let thread_id = args.first().cloned();
    crate::tui::run_with_options(crate::tui::LaunchOptions { thread_id }).await
}

async fn run_send_command(args: &[String], format: OutputFormat) -> Result<()> {
    ensure_server_ready().await?;
    let Some(target) = args.first() else {
        bail!("usage: looper send [active|--all|<thread-id>] <prompt>");
    };
    if args.len() < 2 {
        bail!("usage: looper send [active|--all|<thread-id>] <prompt>");
    }
    let prompt = joined_args(&args[1..]);
    let thread_ids = match target.as_str() {
        SEND_ACTIVE_TARGET | SEND_ALL_TARGET => active_thread_ids().await?,
        thread_id => vec![thread_id.to_owned()],
    };
    post_session_prompts(thread_ids, &prompt, None, format).await
}

async fn run_detach_command(format: OutputFormat) -> Result<()> {
    let server_status = fetch_json("/health")
        .await
        .map(|health| {
            if health.get("ok").and_then(Value::as_bool) == Some(true) {
                "running"
            } else {
                "degraded"
            }
        })
        .unwrap_or("not-running");
    print_value(
        &serde_json::json!({
            "detached": true,
            "server": server_status,
            "detail": "looper terminal surfaces are clients; close them with q and looper-server keeps running",
        }),
        format,
    )
}

async fn run_wait_command(args: &[String], format: OutputFormat) -> Result<()> {
    ensure_server_ready().await?;
    let wait_request = WaitRequest::parse(args)?;
    let baseline = wait_request.baseline().await?;
    let deadline = Instant::now() + wait_request.timeout;
    loop {
        let snapshot = fetch_json("/desktop/snapshot").await?;
        if let Some(result) = wait_request.match_snapshot(&snapshot, baseline) {
            print_value(&result, format)?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "wait timed out after {}s for {}",
                wait_request.timeout.as_secs(),
                wait_request.thread_id
            );
        }
        tokio::time::sleep(WAIT_POLL_INTERVAL).await;
    }
}

async fn session_archive(thread_id: &str, archived: bool, format: OutputFormat) -> Result<()> {
    post_json(
        &format!("/desktop/sessions/{thread_id}/archive"),
        serde_json::json!({ "archived": archived }),
        format,
    )
    .await
}

async fn post_session_prompts(
    thread_ids: Vec<String>,
    prompt: &str,
    preset: Option<&str>,
    format: OutputFormat,
) -> Result<()> {
    post_json(
        "/desktop/session-prompts",
        serde_json::json!({
            "threadIds": thread_ids,
            "prompt": prompt,
            "preset": preset,
        }),
        format,
    )
    .await
}

async fn active_thread_ids() -> Result<Vec<String>> {
    let snapshot = fetch_json("/desktop/snapshot").await?;
    let thread_ids = snapshot
        .get("threads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|thread| {
            !thread
                .get("archived")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .filter_map(|thread| {
            thread
                .get("thread_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    if thread_ids.is_empty() {
        bail!("no active sessions in current snapshot");
    }
    Ok(thread_ids)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitCondition {
    Updated,
    Archived,
    Gone,
}

#[derive(Debug, PartialEq, Eq)]
struct WaitRequest {
    thread_id: String,
    condition: WaitCondition,
    timeout: Duration,
}

impl WaitRequest {
    fn parse(args: &[String]) -> Result<Self> {
        let Some(thread_id) = args.first() else {
            bail!("usage: looper wait <thread-id> [--archived|--gone] [--timeout <seconds>]");
        };
        let mut condition = WaitCondition::Updated;
        let mut timeout = WAIT_DEFAULT_TIMEOUT;
        let mut index = 1;
        while index < args.len() {
            match args[index].as_str() {
                WAIT_ARCHIVED_FLAG => condition = WaitCondition::Archived,
                WAIT_GONE_FLAG => condition = WaitCondition::Gone,
                WAIT_TIMEOUT_FLAG => {
                    let Some(value) = args.get(index + 1) else {
                        bail!("{WAIT_TIMEOUT_FLAG} requires seconds");
                    };
                    timeout = Duration::from_secs(value.parse()?);
                    index += 1;
                }
                value => bail!("unknown wait flag: {value}"),
            }
            index += 1;
        }
        Ok(Self {
            thread_id: thread_id.to_owned(),
            condition,
            timeout,
        })
    }

    async fn baseline(&self) -> Result<Option<i64>> {
        if self.condition != WaitCondition::Updated {
            return Ok(None);
        }
        let snapshot = fetch_json("/desktop/snapshot").await?;
        let thread = snapshot_thread(&snapshot, &self.thread_id);
        Ok(thread.and_then(thread_updated_at_ms))
    }

    fn match_snapshot(&self, snapshot: &Value, baseline: Option<i64>) -> Option<Value> {
        let thread = snapshot_thread(snapshot, &self.thread_id);
        match self.condition {
            WaitCondition::Updated => {
                let thread = thread?;
                let updated_at_ms = thread_updated_at_ms(thread);
                (updated_at_ms != baseline).then(|| {
                    serde_json::json!({
                        "ok": true,
                        "condition": "updated",
                        "threadId": self.thread_id,
                        "updatedAtMs": updated_at_ms,
                        "thread": thread,
                    })
                })
            }
            WaitCondition::Archived => thread
                .filter(|thread| {
                    thread
                        .get("archived")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .map(|thread| {
                    serde_json::json!({
                        "ok": true,
                        "condition": "archived",
                        "threadId": self.thread_id,
                        "thread": thread,
                    })
                }),
            WaitCondition::Gone => thread.is_none().then(|| {
                serde_json::json!({
                    "ok": true,
                    "condition": "gone",
                    "threadId": self.thread_id,
                })
            }),
        }
    }
}

fn snapshot_thread<'a>(snapshot: &'a Value, thread_id: &str) -> Option<&'a Value> {
    snapshot
        .get("threads")
        .and_then(Value::as_array)?
        .iter()
        .find(|thread| thread.get("thread_id").and_then(Value::as_str) == Some(thread_id))
}

fn thread_updated_at_ms(thread: &Value) -> Option<i64> {
    thread.get("updated_at_ms").and_then(Value::as_i64)
}

fn print_usage() {
    let _ = writeln!(
        std::io::stderr(),
        "usage: looper [--json|--table|--format table] [serve|status|snapshot|shutdown|attach|send|wait|detach|hooks|connections|devin|pairing|settings|sessions|notifications|checks|push|doctor|menubar|version]\n       looper              # attach inline, starting looper-server if needed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_request_defaults_to_updated_condition() {
        let request = WaitRequest::parse(&["thread-1".to_owned()]).unwrap();

        assert_eq!(request.thread_id, "thread-1");
        assert_eq!(request.condition, WaitCondition::Updated);
        assert_eq!(request.timeout, WAIT_DEFAULT_TIMEOUT);
    }

    #[test]
    fn wait_request_parses_condition_and_timeout() {
        let request = WaitRequest::parse(&[
            "thread-1".to_owned(),
            WAIT_ARCHIVED_FLAG.to_owned(),
            WAIT_TIMEOUT_FLAG.to_owned(),
            "12".to_owned(),
        ])
        .unwrap();

        assert_eq!(request.condition, WaitCondition::Archived);
        assert_eq!(request.timeout, Duration::from_secs(12));
    }

    #[test]
    fn wait_request_matches_thread_update() {
        let request = WaitRequest {
            thread_id: "thread-1".to_owned(),
            condition: WaitCondition::Updated,
            timeout: WAIT_DEFAULT_TIMEOUT,
        };
        let snapshot = serde_json::json!({
            "threads": [
                { "thread_id": "thread-1", "updated_at_ms": 2, "archived": false }
            ]
        });

        let result = request.match_snapshot(&snapshot, Some(1));

        assert_eq!(result.unwrap()["condition"], "updated");
    }
}

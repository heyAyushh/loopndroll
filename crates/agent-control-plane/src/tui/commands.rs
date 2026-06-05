use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;

use super::state::TuiState;
use super::tabs::TuiTab;
use super::transport::{delete_json, get_json, patch_json, post_json};

const OFF_VALUE: &str = "off";
const WAIT_COMMAND_TOKEN: &str = "wait";

pub(crate) async fn execute_command(
    client: &Client,
    app: &mut TuiState,
    command: &str,
) -> Result<()> {
    let command = command.trim();
    if command.is_empty() {
        return Ok(());
    }
    let mut parts = command.split_whitespace();
    let Some(name) = parts.next() else {
        return Ok(());
    };
    match (app.tab(), name) {
        (TuiTab::Sessions, "mode") => {
            let thread_id = selected_thread_id(app)?;
            let preset = parts.next().unwrap_or(OFF_VALUE);
            post_json(
                client,
                &format!("/desktop/sessions/{thread_id}/mode"),
                serde_json::json!({ "preset": nullable_id(preset) }),
            )
            .await?;
        }
        (TuiTab::Sessions, "prompt") => {
            let thread_id = selected_thread_id(app)?;
            let prompt = command_body(command, name)?;
            post_json(
                client,
                &format!("/desktop/sessions/{thread_id}/prompt"),
                serde_json::json!({ "prompt": prompt }),
            )
            .await?;
        }
        (TuiTab::Sessions, "prompt-mode") => {
            let thread_id = selected_thread_id(app)?;
            let (preset, prompt) = command_arg_and_body(command, name)?;
            prompt_threads(client, vec![thread_id], prompt, Some(preset)).await?;
        }
        (TuiTab::Sessions, "prompt-active") | (TuiTab::Sessions, "prompt-all") => {
            let thread_ids = active_thread_ids(app)?;
            let prompt = command_body(command, name)?;
            prompt_threads(client, thread_ids, prompt, None).await?;
        }
        (TuiTab::Sessions, "prompt-active-mode") | (TuiTab::Sessions, "prompt-all-mode") => {
            let thread_ids = active_thread_ids(app)?;
            let (preset, prompt) = command_arg_and_body(command, name)?;
            prompt_threads(client, thread_ids, prompt, Some(preset)).await?;
        }
        (TuiTab::Sessions, "archive") => archive_session(client, app, true).await?,
        (TuiTab::Sessions, "unarchive") => archive_session(client, app, false).await?,
        (TuiTab::Sessions, "mute") => {
            let thread_id = selected_thread_id(app)?;
            post_json(
                client,
                &format!("/desktop/sessions/{thread_id}/mute"),
                Value::Null,
            )
            .await?;
        }
        (TuiTab::Sessions, "delete") => {
            let thread_id = selected_thread_id(app)?;
            delete_json(client, &format!("/desktop/sessions/{thread_id}")).await?;
        }
        (TuiTab::Sessions, "notifications") => {
            let thread_id = selected_thread_id(app)?;
            let raw_ids = parts.next().unwrap_or(OFF_VALUE);
            post_json(
                client,
                &format!("/desktop/sessions/{thread_id}/notifications"),
                serde_json::json!({ "notificationIds": nullable_id_list(raw_ids) }),
            )
            .await?;
        }
        (TuiTab::Sessions, "completion-check") => {
            let thread_id = selected_thread_id(app)?;
            let check_id = parts.next().unwrap_or(OFF_VALUE);
            post_json(
                client,
                &format!("/desktop/sessions/{thread_id}/completion-check"),
                serde_json::json!({
                    "completionCheckId": nullable_id(check_id),
                    "waitForReplyAfterCompletion": command_has_token(command, WAIT_COMMAND_TOKEN),
                }),
            )
            .await?;
        }
        (TuiTab::Connections, "rename") => {
            if !app.selected_connection_can_rename() {
                bail!("selected connection cannot be renamed");
            }
            let connection_id = selected_connection_id(app)?;
            let label = command_body(command, name)?;
            patch_json(
                client,
                &format!("/desktop/connections/mobile/{connection_id}"),
                serde_json::json!({ "label": label }),
            )
            .await?;
        }
        (TuiTab::Connections, "new-pairing") | (TuiTab::Connections, "pair") => {
            app.replace_pairing(get_json(client, "/desktop/pairing").await?);
        }
        (TuiTab::Connections, "revoke") => {
            if !app.selected_connection_can_revoke() {
                bail!("selected connection cannot be revoked");
            }
            let connection_id = selected_connection_id(app)?;
            delete_json(
                client,
                &format!("/desktop/connections/mobile/{connection_id}"),
            )
            .await?;
        }
        (TuiTab::Connections, "test-push") => {
            let installation_id = parts.next().context("missing installation id")?;
            post_json(
                client,
                &format!("/desktop/push/devices/{installation_id}/test"),
                Value::Null,
            )
            .await?;
        }
        (TuiTab::Settings, "default-prompt") => {
            post_json(
                client,
                "/desktop/settings/default-prompt",
                serde_json::json!({ "defaultPrompt": command_body(command, name)? }),
            )
            .await?;
        }
        (TuiTab::Settings, "scope") => {
            post_json(
                client,
                "/desktop/settings/scope",
                serde_json::json!({ "scope": parts.next().unwrap_or("global") }),
            )
            .await?;
        }
        (TuiTab::Settings, "global-preset") => {
            let preset = parts.next().unwrap_or(OFF_VALUE);
            post_json(
                client,
                "/desktop/settings/global-preset",
                serde_json::json!({ "preset": nullable_id(preset) }),
            )
            .await?;
        }
        (TuiTab::Settings, "global-notification") => {
            let notification_id = parts.next().unwrap_or(OFF_VALUE);
            post_json(
                client,
                "/desktop/settings/global-notification",
                serde_json::json!({ "notificationId": nullable_id(notification_id) }),
            )
            .await?;
        }
        (TuiTab::Settings, "global-check") => {
            let check_id = parts.next().unwrap_or(OFF_VALUE);
            post_json(
                client,
                "/desktop/settings/global-completion-check",
                serde_json::json!({
                    "completionCheckId": nullable_id(check_id),
                    "waitForReplyAfterCompletion": command_has_token(command, WAIT_COMMAND_TOKEN),
                }),
            )
            .await?;
        }
        (TuiTab::Settings, "notify-slack") => {
            let args = command_args(command, name)?;
            if args.len() < 2 {
                bail!("usage: notify-slack <label> <webhook>");
            }
            post_json(
                client,
                "/desktop/notifications",
                serde_json::json!({
                    "label": args[0],
                    "channel": "slack",
                    "webhookUrl": args[1],
                }),
            )
            .await?;
        }
        (TuiTab::Settings, "notify-telegram") => {
            let args = command_args(command, name)?;
            if args.len() < 3 {
                bail!("usage: notify-telegram <label> <bot-token> <chat-id>");
            }
            post_json(
                client,
                "/desktop/notifications",
                serde_json::json!({
                    "label": args[0],
                    "channel": "telegram",
                    "botToken": args[1],
                    "chatId": args[2],
                }),
            )
            .await?;
        }
        (TuiTab::Settings, "delete-notification") => {
            let notification_id = parts.next().context("missing notification id")?;
            delete_json(client, &format!("/desktop/notifications/{notification_id}")).await?;
        }
        (TuiTab::Settings, "check") => {
            let args = command_args(command, name)?;
            if args.len() < 2 {
                bail!("usage: check <label> <command>");
            }
            post_json(
                client,
                "/desktop/completion-checks",
                serde_json::json!({
                    "label": args[0],
                    "commands": [args[1..].join(" ")],
                }),
            )
            .await?;
        }
        (TuiTab::Settings, "delete-check") => {
            let check_id = parts.next().context("missing check id")?;
            delete_json(client, &format!("/desktop/completion-checks/{check_id}")).await?;
        }
        (TuiTab::Settings, "test-push") => {
            let installation_id = parts.next().context("missing installation id")?;
            post_json(
                client,
                &format!("/desktop/push/devices/{installation_id}/test"),
                Value::Null,
            )
            .await?;
        }
        (TuiTab::Dashboard | TuiTab::Logs, _)
        | (TuiTab::Sessions | TuiTab::Connections | TuiTab::Settings, _) => {
            bail!("unknown command for current tab: {name}")
        }
    }
    Ok(())
}

async fn archive_session(client: &Client, app: &TuiState, archived: bool) -> Result<()> {
    let thread_id = selected_thread_id(app)?;
    post_json(
        client,
        &format!("/desktop/sessions/{thread_id}/archive"),
        serde_json::json!({ "archived": archived }),
    )
    .await
}

async fn prompt_threads(
    client: &Client,
    thread_ids: Vec<String>,
    prompt: &str,
    preset: Option<&str>,
) -> Result<()> {
    post_json(
        client,
        "/desktop/session-prompts",
        serde_json::json!({
            "threadIds": thread_ids,
            "prompt": prompt,
            "preset": preset,
        }),
    )
    .await
}

fn selected_thread_id(app: &TuiState) -> Result<String> {
    app.selected_thread_id()
        .context("no selected session in current snapshot")
}

fn active_thread_ids(app: &TuiState) -> Result<Vec<String>> {
    let thread_ids = app.active_thread_ids();
    if thread_ids.is_empty() {
        bail!("no active sessions in current snapshot");
    }
    Ok(thread_ids)
}

fn selected_connection_id(app: &TuiState) -> Result<String> {
    app.selected_connection_id()
        .context("no selected connection in current snapshot")
}

fn nullable_id(value: &str) -> Option<&str> {
    if value == OFF_VALUE {
        None
    } else {
        Some(value)
    }
}

fn nullable_id_list(value: &str) -> Vec<String> {
    if value == OFF_VALUE {
        return Vec::new();
    }
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn command_body<'a>(command: &'a str, name: &str) -> Result<&'a str> {
    command
        .strip_prefix(name)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("missing command body")
}

fn command_arg_and_body<'a>(command: &'a str, name: &str) -> Result<(&'a str, &'a str)> {
    let body = command_body(command, name)?;
    let mut parts = body.splitn(2, char::is_whitespace);
    let arg = parts.next().context("missing command argument")?;
    let body = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context("missing command body")?;
    Ok((arg, body))
}

fn command_args(command: &str, name: &str) -> Result<Vec<String>> {
    Ok(command_body(command, name)?
        .split_whitespace()
        .map(str::to_owned)
        .collect())
}

fn command_has_token(command: &str, token: &str) -> bool {
    command.split_whitespace().any(|part| part == token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_body_returns_text_after_command_name() {
        assert_eq!(
            command_body("prompt continue now", "prompt").expect("body"),
            "continue now"
        );
    }

    #[test]
    fn command_token_matching_is_exact() {
        assert!(command_has_token("global-check unit wait", "wait"));
        assert!(!command_has_token("global-check unit waiter", "wait"));
    }

    #[test]
    fn command_arg_and_body_splits_mode_and_prompt() {
        assert_eq!(
            command_arg_and_body("prompt-mode max-turns-1 continue now", "prompt-mode")
                .expect("mode and prompt"),
            ("max-turns-1", "continue now")
        );
    }
}

use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;

use crate::grpc::proto;

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
            submit_session_mode(&thread_id, nullable_id(preset)).await?;
        }
        (TuiTab::Sessions, "prompt") => {
            let thread_id = selected_thread_id(app)?;
            let prompt = command_body(command, name)?;
            submit_session_prompt(&thread_id, prompt).await?;
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
        (TuiTab::Sessions, "archive") => archive_session(app, true).await?,
        (TuiTab::Sessions, "unarchive") => archive_session(app, false).await?,
        (TuiTab::Sessions, "mute") => {
            let thread_id = selected_thread_id(app)?;
            submit_mute_session(&thread_id).await?;
        }
        (TuiTab::Sessions, "delete") => {
            let thread_id = selected_thread_id(app)?;
            submit_delete_session(&thread_id).await?;
        }
        (TuiTab::Sessions, "notifications") => {
            let thread_id = selected_thread_id(app)?;
            let raw_ids = parts.next().unwrap_or(OFF_VALUE);
            let _ = client;
            submit_session_notifications(&thread_id, nullable_id_list(raw_ids)).await?;
        }
        (TuiTab::Sessions, "completion-check") => {
            let thread_id = selected_thread_id(app)?;
            let check_id = parts.next().unwrap_or(OFF_VALUE);
            let _ = client;
            submit_session_completion_check(
                &thread_id,
                nullable_id(check_id),
                command_has_token(command, WAIT_COMMAND_TOKEN),
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
            submit_default_prompt(command_body(command, name)?).await?;
        }
        (TuiTab::Settings, "scope") => {
            let _ = client;
            submit_scope(parts.next().unwrap_or("global")).await?;
        }
        (TuiTab::Settings, "global-preset") => {
            let preset = parts.next().unwrap_or(OFF_VALUE);
            let _ = client;
            submit_global_preset(nullable_id(preset)).await?;
        }
        (TuiTab::Settings, "global-notification") => {
            let notification_id = parts.next().unwrap_or(OFF_VALUE);
            let _ = client;
            submit_global_notification(nullable_id(notification_id)).await?;
        }
        (TuiTab::Settings, "global-check") => {
            let check_id = parts.next().unwrap_or(OFF_VALUE);
            let _ = client;
            submit_global_completion_check(
                nullable_id(check_id),
                command_has_token(command, WAIT_COMMAND_TOKEN),
            )
            .await?;
        }
        (TuiTab::Settings, "notify-slack") => {
            let args = command_args(command, name)?;
            if args.len() < 2 {
                bail!("usage: notify-slack <label> <webhook>");
            }
            let _ = client;
            submit_upsert_notification_route(
                &new_record_id("notification"),
                &args[0],
                "slack",
                Some(&args[1]),
                None,
                None,
            )
            .await?;
        }
        (TuiTab::Settings, "notify-telegram") => {
            let args = command_args(command, name)?;
            if args.len() < 3 {
                bail!("usage: notify-telegram <label> <bot-token> <chat-id>");
            }
            let _ = client;
            submit_upsert_notification_route(
                &new_record_id("notification"),
                &args[0],
                "telegram",
                None,
                Some(&args[2]),
                Some(&args[1]),
            )
            .await?;
        }
        (TuiTab::Settings, "delete-notification") => {
            let notification_id = parts.next().context("missing notification id")?;
            let _ = client;
            submit_delete_notification_route(notification_id).await?;
        }
        (TuiTab::Settings, "check") => {
            let args = command_args(command, name)?;
            if args.len() < 2 {
                bail!("usage: check <label> <command>");
            }
            let _ = client;
            submit_upsert_completion_check(
                &new_record_id("check"),
                &args[0],
                vec![args[1..].join(" ")],
            )
            .await?;
        }
        (TuiTab::Settings, "delete-check") => {
            let check_id = parts.next().context("missing check id")?;
            let _ = client;
            submit_delete_completion_check(check_id).await?;
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
        (TuiTab::Dashboard, _) | (TuiTab::Sessions | TuiTab::Connections | TuiTab::Settings, _) => {
            bail!("unknown command for current tab: {name}")
        }
    }
    Ok(())
}

async fn archive_session(app: &TuiState, archived: bool) -> Result<()> {
    let thread_id = selected_thread_id(app)?;
    submit_archive_session(&thread_id, archived).await
}

async fn prompt_threads(
    client: &Client,
    thread_ids: Vec<String>,
    prompt: &str,
    preset: Option<&str>,
) -> Result<()> {
    let _ = client;
    for thread_id in thread_ids {
        if let Some(preset) = preset {
            submit_session_mode(&thread_id, nullable_id(preset)).await?;
        }
        submit_session_prompt(&thread_id, prompt).await?;
    }
    Ok(())
}

async fn submit_session_mode(thread_id: &str, preset: Option<&str>) -> Result<()> {
    let client_mutation_id = format!("tui-mode-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetSessionMode(
            proto::SetSessionModeRequest {
                thread_id: thread_id.to_owned(),
                preset: preset.unwrap_or_default().to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_session_prompt(thread_id: &str, prompt: &str) -> Result<()> {
    let client_mutation_id = format!("tui-prompt-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SendSessionPrompt(
            proto::SendSessionPromptRequest {
                thread_id: thread_id.to_owned(),
                prompt: prompt.to_owned(),
                assistant_surface: String::new(),
                client_mutation_id: client_mutation_id.clone(),
                prompt_intent: "queue".to_owned(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_archive_session(thread_id: &str, archived: bool) -> Result<()> {
    let client_mutation_id = format!("tui-archive-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetSessionArchived(
            proto::SetSessionArchivedRequest {
                thread_id: thread_id.to_owned(),
                archived,
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_delete_session(thread_id: &str) -> Result<()> {
    let client_mutation_id = format!("tui-delete-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::DeleteSession(
            proto::DeleteSessionRequest {
                thread_id: thread_id.to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_mute_session(thread_id: &str) -> Result<()> {
    let client_mutation_id = format!("tui-mute-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::MuteSession(
            proto::MuteSessionRequest {
                thread_id: thread_id.to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_default_prompt(prompt: &str) -> Result<()> {
    let client_mutation_id = format!("tui-default-prompt-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SaveDefaultPrompt(
            proto::SaveDefaultPromptRequest {
                prompt: prompt.to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_scope(scope: &str) -> Result<()> {
    let client_mutation_id = format!("tui-scope-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetScope(proto::SetScopeRequest {
            scope: scope.to_owned(),
            client_mutation_id: client_mutation_id.clone(),
        })),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_global_preset(preset: Option<&str>) -> Result<()> {
    let client_mutation_id = format!("tui-global-preset-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetGlobalPreset(
            proto::SetGlobalPresetRequest {
                preset: preset.unwrap_or_default().to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_global_notification(notification_id: Option<&str>) -> Result<()> {
    let client_mutation_id = format!("tui-global-notification-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetGlobalNotification(
            proto::SetGlobalNotificationRequest {
                notification_id: notification_id.unwrap_or_default().to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_global_completion_check(
    completion_check_id: Option<&str>,
    wait_for_reply_after_completion: bool,
) -> Result<()> {
    let client_mutation_id = format!("tui-global-completion-check-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetGlobalCompletionCheck(
            proto::SetGlobalCompletionCheckRequest {
                completion_check_id: completion_check_id.unwrap_or_default().to_owned(),
                wait_for_reply_after_completion,
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_session_notifications(
    thread_id: &str,
    notification_ids: Vec<String>,
) -> Result<()> {
    let client_mutation_id = format!("tui-session-notifications-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetSessionNotifications(
            proto::SetSessionNotificationsRequest {
                thread_id: thread_id.to_owned(),
                notification_ids,
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_session_completion_check(
    thread_id: &str,
    completion_check_id: Option<&str>,
    wait_for_reply_after_completion: bool,
) -> Result<()> {
    let client_mutation_id = format!("tui-session-completion-check-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::SetSessionCompletionCheck(
            proto::SetSessionCompletionCheckRequest {
                thread_id: thread_id.to_owned(),
                completion_check_id: completion_check_id.unwrap_or_default().to_owned(),
                wait_for_reply_after_completion,
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_upsert_notification_route(
    notification_id: &str,
    label: &str,
    channel: &str,
    webhook_url: Option<&str>,
    chat_id: Option<&str>,
    bot_token: Option<&str>,
) -> Result<()> {
    let client_mutation_id = format!("tui-upsert-notification-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::UpsertNotificationRoute(
            proto::UpsertNotificationRouteRequest {
                notification_id: notification_id.to_owned(),
                label: label.to_owned(),
                channel: channel.to_owned(),
                webhook_url: webhook_url.unwrap_or_default().to_owned(),
                chat_id: chat_id.unwrap_or_default().to_owned(),
                bot_token: bot_token.unwrap_or_default().to_owned(),
                chat_username: String::new(),
                chat_display_name: String::new(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_delete_notification_route(notification_id: &str) -> Result<()> {
    let client_mutation_id = format!("tui-delete-notification-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::DeleteNotificationRoute(
            proto::DeleteNotificationRouteRequest {
                notification_id: notification_id.to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_upsert_completion_check(
    completion_check_id: &str,
    label: &str,
    commands: Vec<String>,
) -> Result<()> {
    let client_mutation_id = format!("tui-upsert-completion-check-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::UpsertCompletionCheck(
            proto::UpsertCompletionCheckRequest {
                completion_check_id: completion_check_id.to_owned(),
                label: label.to_owned(),
                commands,
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_delete_completion_check(completion_check_id: &str) -> Result<()> {
    let client_mutation_id = format!("tui-delete-completion-check-{}", uuid::Uuid::new_v4());
    let command = proto::Command {
        command: Some(proto::command::Command::DeleteCompletionCheck(
            proto::DeleteCompletionCheckRequest {
                completion_check_id: completion_check_id.to_owned(),
                client_mutation_id: client_mutation_id.clone(),
            },
        )),
    };
    submit_local_session_command(command, &client_mutation_id).await
}

async fn submit_local_session_command(
    command: proto::Command,
    client_mutation_id: &str,
) -> Result<()> {
    crate::grpc::submit_local_session_command(
        &crate::runtime::default_server_base_url(),
        command,
        client_mutation_id,
    )
    .await?;
    Ok(())
}

fn new_record_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
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

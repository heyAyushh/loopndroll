use anyhow::Result;
use serde_json::Value;

use crate::control_plane::{ControlPlane, DesktopSnapshot, DesktopThread};
use crate::mobile::session::{MobileNotificationRoute, MobileSessionState};
use crate::telegram::{TelegramInboundMessage, TelegramUpdate};

const COMMAND_PREFIX: char = '/';
const HELP_COMMAND: &str = "help";
const LIST_COMMAND: &str = "list";
const STATUS_COMMAND: &str = "status";
const REPLY_COMMAND: &str = "reply";
const MODE_COMMAND: &str = "mode";
const GLOBAL_TARGET: &str = "global";
const OFF_MODE: &str = "off";
const AWAIT_MODE: &str = "await";
const CHECKS_MODE: &str = "checks";
const AWAIT_REPLY_PRESET: &str = "await-reply";
const COMPLETION_CHECKS_PRESET: &str = "completion-checks";
const MAX_LISTED_SESSIONS: usize = 20;

pub async fn poll_once(control_plane: &ControlPlane) -> Result<()> {
    let session_service = control_plane.mobile_session_service();
    let bot_tokens = session_service.telegram_bot_tokens()?;
    for bot_token in bot_tokens {
        let updates = match control_plane
            .telegram_service()
            .poll_updates(&bot_token)
            .await
        {
            Ok(updates) => updates,
            Err(error) => {
                eprintln!("telegram bridge poll failed: {error}");
                continue;
            }
        };
        for update in updates {
            if let Err(error) = handle_update(control_plane, &bot_token, &update).await {
                eprintln!("telegram bridge update failed: {error}");
            }
        }
    }
    Ok(())
}

async fn handle_update(
    control_plane: &ControlPlane,
    bot_token: &str,
    update: &TelegramUpdate,
) -> Result<()> {
    let Some(message) = update.message.as_ref().or(update.channel_post.as_ref()) else {
        return Ok(());
    };
    let Some(chat_id) = message_chat_id(message) else {
        return Ok(());
    };
    if !control_plane
        .mobile_session_service()
        .has_telegram_route(bot_token, &chat_id)?
    {
        return Ok(());
    }
    let Some(text) = message.text.as_deref().and_then(normalized_optional) else {
        return Ok(());
    };

    if let Some(command) = command_name(&text) {
        return handle_command(control_plane, bot_token, &chat_id, &text, &command).await;
    }
    if let Some(reply_message_id) = message
        .reply_to_message
        .as_ref()
        .and_then(|reply| reply.message_id)
    {
        return queue_reply_prompt(control_plane, bot_token, &chat_id, reply_message_id, &text)
            .await;
    }
    Ok(())
}

async fn handle_command(
    control_plane: &ControlPlane,
    bot_token: &str,
    chat_id: &str,
    text: &str,
    command: &str,
) -> Result<()> {
    match command {
        HELP_COMMAND => send_bridge_message(control_plane, bot_token, chat_id, help_text()).await,
        LIST_COMMAND => {
            let snapshot = control_plane.desktop_snapshot()?;
            let state = control_plane.mobile_session_service().state()?;
            send_bridge_message(
                control_plane,
                bot_token,
                chat_id,
                &session_list_text(&snapshot, &state, bot_token, chat_id),
            )
            .await
        }
        STATUS_COMMAND => {
            let snapshot = control_plane.desktop_snapshot()?;
            let state = control_plane.mobile_session_service().state()?;
            send_bridge_message(
                control_plane,
                bot_token,
                chat_id,
                &status_text(&snapshot, &state, bot_token, chat_id),
            )
            .await
        }
        REPLY_COMMAND => handle_reply_command(control_plane, bot_token, chat_id, text).await,
        MODE_COMMAND => handle_mode_command(control_plane, bot_token, chat_id, text).await,
        _ => send_bridge_message(control_plane, bot_token, chat_id, help_text()).await,
    }
}

async fn handle_reply_command(
    control_plane: &ControlPlane,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<()> {
    let args = command_args(text);
    let Some(session_ref) = args.first() else {
        return send_bridge_message(
            control_plane,
            bot_token,
            chat_id,
            "Usage: /reply T1 message",
        )
        .await;
    };
    let prompt = args.get(1..).unwrap_or_default().join(" ");
    let Some(prompt) = normalized_optional(&prompt) else {
        return send_bridge_message(
            control_plane,
            bot_token,
            chat_id,
            "Usage: /reply T1 message",
        )
        .await;
    };
    let snapshot = control_plane.desktop_snapshot()?;
    let Some(thread) = thread_for_ref(&snapshot, session_ref) else {
        return send_bridge_message(
            control_plane,
            bot_token,
            chat_id,
            "Unknown Looper chat ref.",
        )
        .await;
    };
    control_plane
        .mobile_session_service()
        .queue_prompt(&thread.thread_id, &prompt)?;
    send_bridge_message(control_plane, bot_token, chat_id, "Prompt queued.").await
}

async fn handle_mode_command(
    control_plane: &ControlPlane,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<()> {
    let args = command_args(text);
    let Some(target) = args.first() else {
        return send_bridge_message(
            control_plane,
            bot_token,
            chat_id,
            "Usage: /mode global await | /mode T1 off",
        )
        .await;
    };
    let preset = args.get(1).and_then(|value| preset_for_mode(value));
    if target.eq_ignore_ascii_case(GLOBAL_TARGET) {
        control_plane
            .mobile_session_service()
            .set_global_preset(preset)?;
        return send_bridge_message(control_plane, bot_token, chat_id, "Global mode updated.")
            .await;
    }
    let snapshot = control_plane.desktop_snapshot()?;
    let Some(thread) = thread_for_ref(&snapshot, target) else {
        return send_bridge_message(
            control_plane,
            bot_token,
            chat_id,
            "Unknown Looper chat ref.",
        )
        .await;
    };
    control_plane
        .mobile_session_service()
        .set_session_preset(&thread.thread_id, preset)?;
    send_bridge_message(control_plane, bot_token, chat_id, "Chat mode updated.").await
}

async fn queue_reply_prompt(
    control_plane: &ControlPlane,
    bot_token: &str,
    chat_id: &str,
    reply_message_id: i64,
    prompt: &str,
) -> Result<()> {
    let Some(thread_id) = control_plane
        .mobile_session_service()
        .telegram_receipt_thread_id(bot_token, chat_id, reply_message_id)?
    else {
        return Ok(());
    };
    control_plane
        .mobile_session_service()
        .queue_prompt(&thread_id, prompt)?;
    send_bridge_message(control_plane, bot_token, chat_id, "Prompt queued.").await
}

async fn send_bridge_message(
    control_plane: &ControlPlane,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<()> {
    control_plane
        .telegram_service()
        .send_message(bot_token, chat_id, text)
        .await
        .map(|_| ())
        .map_err(anyhow::Error::from)
}

fn help_text() -> &'static str {
    "Available commands:\n/list\n/status\n/reply T1 message\n/mode global await\n/mode T1 off\n\nReply to a Looper notification to target that chat."
}

fn session_list_text(
    snapshot: &DesktopSnapshot,
    state: &MobileSessionState,
    bot_token: &str,
    chat_id: &str,
) -> String {
    let sessions = sessions_for_chat(snapshot, state, bot_token, chat_id);
    if sessions.is_empty() {
        return "No Looper chats are registered to this Telegram destination yet.".to_owned();
    }
    let lines = sessions
        .into_iter()
        .take(MAX_LISTED_SESSIONS)
        .map(|(session_ref, thread)| {
            format!(
                "[{session_ref}] - {}",
                thread
                    .title
                    .clone()
                    .unwrap_or_else(|| "Untitled chat".to_owned())
            )
        })
        .collect::<Vec<_>>();
    format!("Registered chats:\n{}", lines.join("\n"))
}

fn status_text(
    snapshot: &DesktopSnapshot,
    state: &MobileSessionState,
    bot_token: &str,
    chat_id: &str,
) -> String {
    let mut lines = vec![
        "Current status:".to_owned(),
        format!(
            "Global preset: {}",
            state.global_preset.as_deref().unwrap_or(OFF_MODE)
        ),
        String::new(),
        "Per-chat presets:".to_owned(),
    ];
    for (session_ref, thread) in sessions_for_chat(snapshot, state, bot_token, chat_id)
        .into_iter()
        .take(MAX_LISTED_SESSIONS)
    {
        let preset = state
            .sessions
            .get(&thread.thread_id)
            .and_then(|session| session.preset.as_deref())
            .unwrap_or("inherit global");
        let title = thread
            .title
            .clone()
            .unwrap_or_else(|| "Untitled chat".to_owned());
        lines.push(format!("[{session_ref}] - {title}: {preset}"));
    }
    lines.join("\n")
}

fn sessions_for_chat<'a>(
    snapshot: &'a DesktopSnapshot,
    state: &MobileSessionState,
    bot_token: &str,
    chat_id: &str,
) -> Vec<(String, &'a DesktopThread)> {
    snapshot
        .threads
        .iter()
        .enumerate()
        .filter(|(_, thread)| {
            notification_routes_for_thread(&thread.thread_id, state)
                .iter()
                .any(|route| route_matches_chat(route, bot_token, chat_id))
        })
        .map(|(index, thread)| (format!("T{}", index + 1), thread))
        .collect()
}

fn notification_routes_for_thread<'a>(
    thread_id: &str,
    state: &'a MobileSessionState,
) -> Vec<&'a MobileNotificationRoute> {
    let ids = state
        .sessions
        .get(thread_id)
        .map(|session| session.notification_ids.clone())
        .filter(|ids| !ids.is_empty())
        .or_else(|| state.global_notification_id.clone().map(|id| vec![id]))
        .unwrap_or_default();
    state
        .notifications
        .iter()
        .filter(|route| ids.contains(&route.id))
        .collect()
}

fn route_matches_chat(route: &MobileNotificationRoute, bot_token: &str, chat_id: &str) -> bool {
    route.bot_token.as_deref() == Some(bot_token) && route.chat_id.as_deref() == Some(chat_id)
}

fn thread_for_ref<'a>(
    snapshot: &'a DesktopSnapshot,
    session_ref: &str,
) -> Option<&'a DesktopThread> {
    let normalized = session_ref.trim().trim_start_matches('T').trim();
    let index = normalized.parse::<usize>().ok()?.checked_sub(1)?;
    snapshot.threads.get(index)
}

fn message_chat_id(message: &TelegramInboundMessage) -> Option<String> {
    match message.chat.as_ref()?.id.clone() {
        Value::Number(number) => Some(number.to_string()),
        Value::String(value) => normalized_optional(&value),
        _ => None,
    }
}

fn command_name(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    let command = token
        .strip_prefix(COMMAND_PREFIX)?
        .split('@')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    (!command.is_empty()).then_some(command)
}

fn command_args(text: &str) -> Vec<String> {
    text.split_whitespace().skip(1).map(str::to_owned).collect()
}

fn preset_for_mode(value: &str) -> Option<&str> {
    match value {
        OFF_MODE => None,
        AWAIT_MODE => Some(AWAIT_REPLY_PRESET),
        CHECKS_MODE => Some(COMPLETION_CHECKS_PRESET),
        value => Some(value),
    }
}

fn normalized_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_name_allows_bot_suffix() {
        assert_eq!(
            command_name("/reply@looper_bot T1 hi").as_deref(),
            Some("reply")
        );
    }
}

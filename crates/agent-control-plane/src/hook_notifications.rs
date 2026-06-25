use anyhow::{Context, Result, anyhow};
use reqwest::Client;

use crate::control_plane::ControlPlane;
use crate::mobile::session::{
    MobileHookPayload, MobileNotificationRoute, NOTIFICATION_TARGET_IPHONE,
};

const STOP_HOOK_EVENT: &str = "Stop";
const SLACK_CHANNEL: &str = "slack";
const TELEGRAM_CHANNEL: &str = "telegram";
const TELEGRAM_REPLY_HINT: &str =
    "Reply to this Telegram message to send the next prompt to this Looper chat.";

pub async fn send_stop_notifications(
    control_plane: &ControlPlane,
    payload: &MobileHookPayload,
) -> Result<()> {
    if payload.hook_event_name != STOP_HOOK_EVENT {
        return Ok(());
    }
    let Some(thread_id) = payload.session_id.as_deref().and_then(normalized_optional) else {
        return Ok(());
    };
    let Some(message) = payload
        .last_assistant_message
        .as_deref()
        .and_then(normalized_optional)
    else {
        return Ok(());
    };
    let routes = control_plane
        .mobile_session_service()
        .notification_routes_for_thread(&thread_id)?;
    let target_ids = control_plane
        .mobile_session_service()
        .notification_target_ids_for_thread(&thread_id)?;
    if target_ids
        .iter()
        .any(|target_id| target_id == NOTIFICATION_TARGET_IPHONE)
    {
        if let Err(error) = control_plane
            .mobile_push_service()
            .send_session_stop_pushes(&thread_id, &message)
            .await
        {
            eprintln!("iPhone notification target failed: {error}");
        }
    }
    let client = Client::new();
    for route in routes {
        if let Err(error) = send_route(&client, control_plane, &route, &thread_id, &message).await {
            eprintln!("notification route {} failed: {error}", route.id);
        }
    }
    Ok(())
}

async fn send_route(
    client: &Client,
    control_plane: &ControlPlane,
    route: &MobileNotificationRoute,
    thread_id: &str,
    message: &str,
) -> Result<()> {
    match route.channel.as_str() {
        SLACK_CHANNEL => send_slack(client, route, message).await,
        TELEGRAM_CHANNEL => send_telegram(control_plane, route, thread_id, message).await,
        _ => Ok(()),
    }
}

async fn send_slack(client: &Client, route: &MobileNotificationRoute, message: &str) -> Result<()> {
    let webhook_url = route
        .webhook_url
        .as_deref()
        .and_then(normalized_optional)
        .ok_or_else(|| anyhow!("Slack route is missing webhook URL"))?;
    let response = client
        .post(webhook_url)
        .json(&serde_json::json!({ "text": message }))
        .send()
        .await
        .context("send Slack notification")?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "Slack notification failed with status {}",
            response.status()
        ));
    }
    Ok(())
}

async fn send_telegram(
    control_plane: &ControlPlane,
    route: &MobileNotificationRoute,
    thread_id: &str,
    message: &str,
) -> Result<()> {
    let bot_token = route
        .bot_token
        .as_deref()
        .and_then(normalized_optional)
        .ok_or_else(|| anyhow!("Telegram route is missing bot token"))?;
    let chat_id = route
        .chat_id
        .as_deref()
        .and_then(normalized_optional)
        .ok_or_else(|| anyhow!("Telegram route is missing chat id"))?;
    let message_id = control_plane
        .telegram_service()
        .send_message(&bot_token, &chat_id, &telegram_message_text(message))
        .await?;
    if let Some(message_id) = message_id {
        control_plane
            .mobile_session_service()
            .record_telegram_delivery_receipt(
                &route.id, thread_id, &bot_token, &chat_id, message_id,
            )?;
    }
    Ok(())
}

fn telegram_message_text(message: &str) -> String {
    format!("{message}\n\n{TELEGRAM_REPLY_HINT}")
}

fn normalized_optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

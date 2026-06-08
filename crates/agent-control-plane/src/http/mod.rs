use std::collections::BTreeSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use async_stream::stream;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response, Sse};
use axum::{
    Json, Router,
    routing::{delete, get, post},
};
use futures_util::{SinkExt, StreamExt};

use crate::claude_code::inspect_claude_hooks;
use crate::codex_resume::{CodexResumeRequest, spawn_thread_resume};
use crate::control_plane::{ControlPlane, DesktopSnapshot, DesktopThread};
use crate::grok_build::inspect_grok_hooks;
use crate::hook_integration::{HookBridgeContract, hook_bridge_contract_toml};
use crate::mobile_api::{
    PromptDeliveryAction, mobile_session_detail, mobile_snapshot,
    prompt_delivery_action_for_target, prompt_delivery_action_for_visible_target,
};
use crate::mobile_auth::{
    CONNECTION_ORB_TTL_SECONDS, CompleteMobilePasskeyAuthenticationInput,
    CompleteMobilePasskeyRegistrationInput, MobileConnectionCode,
};
use crate::mobile_events::{
    MobileEvent, MobileEventInput, MobileEventKind, MobileEventRecord, mobile_event_now,
    mobile_event_sse_name,
};
use crate::mobile_push::MobilePushRegistrationRequest;
use crate::mobile_session::{
    ASSISTANT_SURFACES, MobileSessionError, MobileSessionState, UpsertMobileNotificationRoute,
};

mod mobile_access;
mod requests;
mod responses;

use self::mobile_access::{
    authorize_mobile_api_request, authorize_mobile_request, current_mobile_time,
    desktop_loopback_rejection, request_advertised_mobile_base_urls,
};
use self::requests::{
    DesktopCompletionCheckConfigRequest, DesktopCompletionCheckRequest,
    DesktopConnectionRenameRequest, DesktopDefaultPromptRequest, DesktopDevinAcpBridgeProbeRequest,
    DesktopGlobalNotificationRequest, DesktopNotificationRequest, DesktopScopeRequest,
    DesktopSessionBatchPromptRequest, DesktopSessionNotificationsRequest, DesktopSnapshotQuery,
    DesktopTelegramChatsRequest, MobileAssistantSurfaceRequest, MobileDefaultPromptRequest,
    MobilePasskeyAuthenticationChallengeRequest, MobilePushTestRequest,
    MobileSessionArchiveRequest, MobileSessionDetailQuery, MobileSessionModeRequest,
    MobileSessionPromptQuery, MobileSessionPromptRequest,
};
use self::responses::{
    internal_mobile_error_response, mobile_auth_error_response,
    mobile_authorization_error_response, mobile_push_error_response, mobile_session_error_response,
    mobile_session_not_found_response, telegram_error_response,
};

pub fn build_router(control_plane: ControlPlane) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status/control-plane", get(control_plane_status))
        .route("/automations", get(automations))
        .route("/goal", get(goals))
        .route("/goals", get(goals))
        .route("/assistant-adapters", get(assistant_adapters))
        .route("/codex/servers", get(codex_servers))
        .route("/codex/compactions", get(compactions))
        .route("/desktop/snapshot", get(desktop_snapshot))
        .route("/desktop/events", get(desktop_events))
        .route("/handoff/sessions/:thread_id", get(handoff_session_page))
        .route("/acp/devin", get(devin_acp_websocket))
        .route("/desktop/devin", get(desktop_devin))
        .route("/desktop/devin/acp-bridge", get(desktop_devin_acp_bridge))
        .route(
            "/desktop/devin/acp-bridge/probe",
            post(desktop_devin_acp_bridge_probe),
        )
        .route(
            "/desktop/devin/acp-bridge/install",
            post(desktop_devin_acp_bridge_install),
        )
        .route("/desktop/connections", get(desktop_connections))
        .route("/desktop/pairing", get(desktop_pairing))
        .route(
            "/desktop/pairing-orbs/:orb_id",
            get(desktop_pairing_orb_png),
        )
        .route(
            "/desktop/connections/mobile/:connection_id",
            delete(desktop_mobile_connection_revoke).patch(desktop_mobile_connection_rename),
        )
        .route("/desktop/mobile-state", get(desktop_mobile_state))
        .route("/desktop/push/devices", get(desktop_push_devices))
        .route(
            "/desktop/push/devices/:installation_id/test",
            post(desktop_push_test),
        )
        .route(
            "/desktop/settings/default-prompt",
            post(desktop_default_prompt),
        )
        .route("/desktop/settings/scope", post(desktop_scope))
        .route(
            "/desktop/settings/assistant-surface",
            post(desktop_assistant_surface),
        )
        .route(
            "/desktop/settings/global-preset",
            post(desktop_global_preset),
        )
        .route(
            "/desktop/settings/global-notification",
            post(desktop_global_notification),
        )
        .route(
            "/desktop/settings/global-completion-check",
            post(desktop_global_completion_check),
        )
        .route("/desktop/notifications", post(desktop_notification_upsert))
        .route(
            "/desktop/notifications/:notification_id",
            delete(desktop_notification_delete),
        )
        .route("/desktop/telegram/chats", post(desktop_telegram_chats))
        .route(
            "/desktop/completion-checks",
            post(desktop_completion_check_upsert),
        )
        .route(
            "/desktop/completion-checks/:completion_check_id",
            delete(desktop_completion_check_delete),
        )
        .route(
            "/desktop/sessions/:thread_id/notifications",
            post(desktop_session_notifications),
        )
        .route(
            "/desktop/sessions/:thread_id/completion-check",
            post(desktop_session_completion_check),
        )
        .route("/desktop/session-prompts", post(desktop_sessions_prompt))
        .route(
            "/desktop/sessions/:thread_id/mode",
            post(desktop_session_mode),
        )
        .route(
            "/desktop/sessions/:thread_id/archive",
            post(desktop_session_archive),
        )
        .route(
            "/desktop/sessions/:thread_id/prompt",
            post(desktop_session_prompt),
        )
        .route(
            "/desktop/sessions/:thread_id/mute",
            post(desktop_session_mute),
        )
        .route(
            "/desktop/sessions/:thread_id",
            get(desktop_session_detail).delete(desktop_session_delete),
        )
        .route("/desktop/shutdown", post(desktop_shutdown))
        .route("/sync/manifest", get(sync_manifest))
        .route("/hooks/clear", post(unregister_hooks))
        .route("/hooks/register", post(register_hooks))
        .route("/hooks/unregister", post(unregister_hooks))
        .route("/hooks/unregister-live", post(unregister_live_hooks))
        .route("/integrations/hook/contract", get(hook_contract))
        .route("/integrations/hook/contract.toml", get(hook_contract_toml))
        .route("/api/mobile/health", get(mobile_health))
        .route("/api/mobile/connection-code", get(mobile_connection_code))
        .route(
            "/api/mobile/connection-orb.png",
            get(mobile_connection_orb_png),
        )
        .route(
            "/api/mobile/connection-orbs/:orb_id",
            get(mobile_connection_orb),
        )
        .route("/api/mobile/snapshot", get(mobile_snapshot_handler))
        .route("/api/mobile/events", get(mobile_events_handler))
        .route(
            "/api/mobile/sessions/:thread_id",
            get(mobile_session_detail_handler).delete(mobile_session_delete),
        )
        .route(
            "/api/mobile/sessions/:thread_id/mode",
            post(mobile_session_mode),
        )
        .route(
            "/api/mobile/sessions/:thread_id/archive",
            post(mobile_session_archive),
        )
        .route(
            "/api/mobile/sessions/:thread_id/prompt",
            post(mobile_session_prompt),
        )
        .route(
            "/api/mobile/sessions/:thread_id/mute",
            post(mobile_session_mute),
        )
        .route(
            "/api/mobile/settings/default-prompt",
            post(mobile_default_prompt),
        )
        .route(
            "/api/mobile/settings/assistant-surface",
            post(mobile_assistant_surface),
        )
        .route(
            "/api/mobile/passkeys/registration-challenge",
            post(mobile_passkey_registration_challenge),
        )
        .route(
            "/api/mobile/passkeys/register",
            post(mobile_passkey_registration),
        )
        .route(
            "/api/mobile/passkeys/authentication-challenge",
            post(mobile_passkey_authentication_challenge),
        )
        .route(
            "/api/mobile/passkeys/authenticate",
            post(mobile_passkey_authentication),
        )
        .route(
            "/api/mobile/passkeys/:credential_id",
            delete(mobile_passkey_revocation),
        )
        .route("/api/mobile/push/register", post(mobile_push_registration))
        .route("/api/mobile/push/test", post(mobile_push_test))
        .route("/threads", get(threads))
        .route("/threads/:thread_id", get(thread_detail))
        .route("/threads/:thread_id/capabilities", get(thread_capabilities))
        .route("/events/tail", get(events_tail))
        .with_state(control_plane)
}

async fn health(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    let status = control_plane.status();
    let grok_hooks = inspect_grok_hooks(control_plane.grok_home());
    let claude_hooks = inspect_claude_hooks(&control_plane.claude_home());
    Json(serde_json::json!({
        "service": "looper",
        "ok": status.source.health == "healthy",
        "source": status.source,
        "hooks": status.hooks,
        "grok_hooks": grok_hooks,
        "claude_hooks": claude_hooks,
    }))
}

async fn control_plane_status(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    Json(control_plane.status())
}

async fn automations(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.automations_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn goals(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.goals_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn assistant_adapters(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    Json(control_plane.assistant_adapters_response())
}

async fn desktop_devin(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    Json(control_plane.devin_desktop_response())
}

async fn desktop_devin_acp_bridge(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    Json(control_plane.devin_acp_bridge_response())
}

async fn desktop_devin_acp_bridge_probe(
    State(control_plane): State<ControlPlane>,
    Json(input): Json<DesktopDevinAcpBridgeProbeRequest>,
) -> impl IntoResponse {
    Json(control_plane.devin_acp_bridge_probe_response(input.agent_id.as_deref()))
}

async fn desktop_devin_acp_bridge_install(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane.install_devin_acp_bridge_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn devin_acp_websocket(
    State(control_plane): State<ControlPlane>,
    websocket: WebSocketUpgrade,
) -> impl IntoResponse {
    websocket.on_upgrade(move |socket| run_devin_acp_socket(control_plane, socket))
}

async fn run_devin_acp_socket(control_plane: ControlPlane, socket: WebSocket) {
    let (mut socket_sender, mut socket_receiver) = socket.split();
    let (outbound_sender, mut outbound_receiver) = tokio::sync::mpsc::unbounded_channel::<String>();
    let runtime = control_plane.devin_acp_runtime().clone();
    let connection_id = runtime.register_connection(outbound_sender.clone());
    let writer = tokio::spawn(async move {
        while let Some(message) = outbound_receiver.recv().await {
            if socket_sender.send(Message::Text(message)).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = socket_receiver.next().await {
        match message {
            Ok(Message::Text(text)) => {
                for response in runtime.handle_text_message(&connection_id, &text) {
                    if outbound_sender.send(response).is_err() {
                        break;
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(Message::Binary(_)) | Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
            Err(_) => break,
        }
    }

    runtime.unregister_connection(&connection_id);
    writer.abort();
}

async fn codex_servers(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    Json(control_plane.codex_servers_response())
}

async fn compactions(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.compactions_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn desktop_snapshot(
    State(control_plane): State<ControlPlane>,
    Query(query): Query<DesktopSnapshotQuery>,
) -> impl IntoResponse {
    let snapshot = if query.profile.as_deref() == Some("menu") {
        control_plane.desktop_menu_snapshot()
    } else {
        control_plane.desktop_snapshot()
    };
    match snapshot {
        Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn desktop_events(
    State(control_plane): State<ControlPlane>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    local_desktop_events_stream(control_plane)
}

async fn handoff_session_page(
    State(control_plane): State<ControlPlane>,
    Path(thread_id): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let snapshot = match control_plane.desktop_menu_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("text/plain; charset=utf-8"),
                )],
                error.to_string(),
            )
                .into_response();
        }
    };

    let Some(thread) = snapshot
        .threads
        .iter()
        .find(|thread| thread.thread_id == thread_id && !thread.archived)
        .or_else(|| {
            snapshot
                .threads
                .iter()
                .find(|thread| thread.thread_id == thread_id)
        })
    else {
        return (
            StatusCode::NOT_FOUND,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )],
            "Session not found".to_owned(),
        )
            .into_response();
    };

    (
        StatusCode::OK,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        handoff_session_html(
            thread,
            request_advertised_mobile_base_urls(&headers)
                .first()
                .map(String::as_str),
        ),
    )
        .into_response()
}

async fn desktop_connections(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    match control_plane.managed_connections_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn desktop_pairing(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    let base_urls = request_advertised_mobile_base_urls(&HeaderMap::new());
    match control_plane
        .mobile_auth_service()
        .issue_connection_code(base_urls)
    {
        Ok(connection_code) => (
            StatusCode::OK,
            Json(desktop_pairing_response(&connection_code)),
        )
            .into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn desktop_pairing_orb_png(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(orb_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    match control_plane
        .mobile_auth_service()
        .connection_orb_png_data(&orb_id)
    {
        Ok(png_data) => png_response(png_data),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn desktop_mobile_connection_rename(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(connection_id): Path<String>,
    Json(input): Json<DesktopConnectionRenameRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    if let Err(error) = control_plane
        .mobile_auth_service()
        .rename_mobile_connection(&connection_id, &input.label)
    {
        return mobile_auth_error_response(error);
    }

    match control_plane.managed_connections_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn desktop_mobile_connection_revoke(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(connection_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    if let Err(error) = control_plane
        .mobile_auth_service()
        .revoke_mobile_connection(&connection_id)
    {
        return mobile_auth_error_response(error);
    }

    match control_plane.managed_connections_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn desktop_mobile_state(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    desktop_mobile_state_response(&control_plane)
}

async fn desktop_push_devices(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    match control_plane.mobile_push_service().registered_devices() {
        Ok(devices) => (
            StatusCode::OK,
            Json(serde_json::json!({ "devices": devices })),
        )
            .into_response(),
        Err(error) => mobile_push_error_response(error),
    }
}

async fn desktop_push_test(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(installation_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    match control_plane
        .mobile_push_service()
        .send_test_push(&installation_id)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => mobile_push_error_response(error),
    }
}

async fn desktop_default_prompt(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopDefaultPromptRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .save_default_prompt(&input.default_prompt)
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_scope(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopScopeRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_scope(&input.scope)
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_assistant_surface(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<MobileAssistantSurfaceRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_assistant_surface(&input.assistant_surface)
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_global_preset(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<MobileSessionModeRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_global_preset(input.preset.as_deref())
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_global_notification(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopGlobalNotificationRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_global_notification(input.notification_id.as_deref())
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_global_completion_check(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopCompletionCheckConfigRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_global_completion_check(
            input.completion_check_id.as_deref(),
            input.wait_for_reply_after_completion,
        ) {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_notification_upsert(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopNotificationRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .upsert_notification_route(UpsertMobileNotificationRoute {
            id: input.id.or_else(|| Some(new_record_id("notification"))),
            label: input.label,
            channel: input.channel,
            webhook_url: input.webhook_url,
            chat_id: input.chat_id,
            bot_token: input.bot_token,
            chat_username: input.chat_username,
            chat_display_name: input.chat_display_name,
        }) {
        Ok(_) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_notification_delete(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(notification_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .delete_notification_route(&notification_id)
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_telegram_chats(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopTelegramChatsRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .telegram_service()
        .chats(&input.bot_token, input.wait_for_updates.unwrap_or(false))
        .await
    {
        Ok(chats) => (StatusCode::OK, Json(serde_json::json!({ "chats": chats }))).into_response(),
        Err(error) => telegram_error_response(error),
    }
}

async fn desktop_completion_check_upsert(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopCompletionCheckRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .upsert_completion_check(
            input.id.as_deref().unwrap_or(&new_record_id("check")),
            &input.label,
            &input.commands,
        ) {
        Ok(_) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_completion_check_delete(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(completion_check_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .delete_completion_check(&completion_check_id)
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_session_notifications(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
    Json(input): Json<DesktopSessionNotificationsRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_session_notifications(&thread_id, &input.notification_ids)
    {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_session_detail(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    let snapshot = match control_plane.desktop_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    let session_state = match control_plane.mobile_session_service().state() {
        Ok(session_state) => session_state,
        Err(error) => return mobile_session_error_response(error),
    };
    match mobile_session_detail(&snapshot, &session_state, &thread_id, None) {
        Some(detail) => (StatusCode::OK, Json(detail)).into_response(),
        None => mobile_session_not_found_response(),
    }
}

async fn desktop_session_completion_check(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
    Json(input): Json<DesktopCompletionCheckConfigRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_session_completion_check(
            &thread_id,
            input.completion_check_id.as_deref(),
            input.wait_for_reply_after_completion,
        ) {
        Ok(()) => desktop_mobile_state_response(&control_plane),
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_session_mode(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
    Json(input): Json<MobileSessionModeRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_session_preset(&thread_id, input.preset.as_deref())
    {
        Ok(()) => {
            emit_mobile_lifecycle_changed(
                &control_plane,
                &thread_id,
                input.preset.as_deref().or(Some("mode-cleared")),
            );
            emit_mobile_session_changed(&control_plane, Some(&thread_id), Some("mode-updated"));
            desktop_mobile_state_response(&control_plane)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_session_archive(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
    Json(input): Json<MobileSessionArchiveRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .set_session_archived(&thread_id, input.archived)
    {
        Ok(()) => {
            emit_mobile_session_changed(
                &control_plane,
                Some(&thread_id),
                Some(if input.archived {
                    "archived"
                } else {
                    "unarchived"
                }),
            );
            desktop_mobile_state_response(&control_plane)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_session_prompt(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
    Json(input): Json<MobileSessionPromptRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match dispatch_session_prompt(&control_plane, &thread_id, None, &input.prompt) {
        Ok(dispatch) => {
            emit_prompt_dispatch(&control_plane, &thread_id, &dispatch);
            desktop_mobile_state_response(&control_plane)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_sessions_prompt(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Json(input): Json<DesktopSessionBatchPromptRequest>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }

    let snapshot = match control_plane.desktop_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    match queue_desktop_batch_prompt(&control_plane, &snapshot, input) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => mobile_session_error_response(error),
    }
}

fn queue_desktop_batch_prompt(
    control_plane: &ControlPlane,
    snapshot: &DesktopSnapshot,
    input: DesktopSessionBatchPromptRequest,
) -> Result<serde_json::Value, MobileSessionError> {
    let prompt = required_prompt(&input.prompt)?;
    let thread_ids = unique_thread_ids(input.thread_ids);
    if thread_ids.is_empty() {
        return Err(MobileSessionError::SessionNotFound);
    }
    let session_state = control_plane.mobile_session_service().state()?;
    let actions = thread_ids
        .iter()
        .map(|thread_id| prompt_delivery_action_for_target(snapshot, &session_state, thread_id))
        .collect::<Result<Vec<_>, _>>()?;

    let session_service = control_plane.mobile_session_service();
    let mut prompt_ids = Vec::with_capacity(thread_ids.len());
    let mut resumed_thread_ids = Vec::new();
    for (thread_id, action) in thread_ids.iter().zip(actions) {
        if let Some(preset) = input.preset.as_deref() {
            session_service.set_session_preset(thread_id, Some(preset))?;
        }
        let dispatch =
            dispatch_session_prompt_with_action(control_plane, thread_id, &prompt, action)?;
        emit_prompt_dispatch(control_plane, thread_id, &dispatch);
        match dispatch {
            PromptDispatch::Delivered { prompt_id } => prompt_ids.push(prompt_id),
            PromptDispatch::Queued { prompt_id } => prompt_ids.push(prompt_id),
            PromptDispatch::Resumed => resumed_thread_ids.push(thread_id.clone()),
        }
    }

    Ok(serde_json::json!({
        "prompted": thread_ids.len(),
        "threadIds": thread_ids,
        "promptIds": prompt_ids,
        "resumedThreadIds": resumed_thread_ids,
    }))
}

fn unique_thread_ids(thread_ids: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    thread_ids
        .into_iter()
        .filter_map(|thread_id| {
            let thread_id = thread_id.trim().to_owned();
            (!thread_id.is_empty() && seen.insert(thread_id.clone())).then_some(thread_id)
        })
        .collect()
}

enum PromptDispatch {
    Delivered { prompt_id: String },
    Queued { prompt_id: String },
    Resumed,
}

fn dispatch_session_prompt(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
    prompt: &str,
) -> Result<PromptDispatch, MobileSessionError> {
    let prompt = required_prompt(prompt)?;
    let snapshot = mobile_desktop_snapshot(control_plane)
        .map_err(|error| MobileSessionError::PromptSnapshotUnavailable(error.to_string()))?;
    let session_state = control_plane.mobile_session_service().state()?;
    let action = match assistant_surface {
        Some(surface) => prompt_delivery_action_for_visible_target(
            &snapshot,
            &session_state,
            thread_id,
            Some(surface),
        )?,
        None => prompt_delivery_action_for_target(&snapshot, &session_state, thread_id)?,
    };
    dispatch_session_prompt_with_action(control_plane, thread_id, &prompt, action)
}

fn dispatch_session_prompt_with_action(
    control_plane: &ControlPlane,
    thread_id: &str,
    prompt: &str,
    action: PromptDeliveryAction,
) -> Result<PromptDispatch, MobileSessionError> {
    match action {
        PromptDeliveryAction::QueueForHook => {
            let prompt = control_plane
                .mobile_session_service()
                .queue_prompt(thread_id, prompt)?;
            Ok(PromptDispatch::Queued {
                prompt_id: prompt.id,
            })
        }
        PromptDeliveryAction::SendDevinAcp { session_id } => {
            let delivered = control_plane
                .devin_acp_runtime()
                .deliver_mobile_prompt(&session_id, prompt)
                .map_err(|error| MobileSessionError::PromptResumeUnavailable(error.to_string()))?;
            Ok(PromptDispatch::Delivered {
                prompt_id: delivered.prompt_id,
            })
        }
        PromptDeliveryAction::ResumeCodex(target) => {
            spawn_thread_resume(&CodexResumeRequest {
                thread_id: target.thread_id,
                prompt: prompt.to_owned(),
                cwd: target.cwd,
                codex_executable: control_plane.codex_executable().map(str::to_owned),
            })
            .map_err(|error| MobileSessionError::PromptResumeUnavailable(error.to_string()))?;
            Ok(PromptDispatch::Resumed)
        }
    }
}

fn required_prompt(prompt: &str) -> Result<String, MobileSessionError> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err(MobileSessionError::PromptRequired);
    }
    Ok(prompt.to_owned())
}

async fn desktop_session_mute(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .mute_session(&thread_id)
    {
        Ok(()) => {
            emit_mobile_session_changed(&control_plane, Some(&thread_id), Some("muted"));
            desktop_mobile_state_response(&control_plane)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_session_delete(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    match control_plane
        .mobile_session_service()
        .delete_session(&thread_id)
    {
        Ok(()) => {
            emit_mobile_session_changed(&control_plane, Some(&thread_id), Some("deleted"));
            desktop_mobile_state_response(&control_plane)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn desktop_shutdown(ConnectInfo(socket_addr): ConnectInfo<SocketAddr>) -> impl IntoResponse {
    if let Some(response) = desktop_loopback_rejection(socket_addr) {
        return response;
    }
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::process::exit(0);
    });
    (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response()
}

async fn sync_manifest(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.sync_manifest_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn unregister_hooks(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.unregister_hooks() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn register_hooks(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.register_hooks() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn unregister_live_hooks(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.unregister_live_hooks() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn hook_contract() -> impl IntoResponse {
    Json(HookBridgeContract::default_local_relay())
}

async fn hook_contract_toml() -> impl IntoResponse {
    match hook_bridge_contract_toml(&HookBridgeContract::default_local_relay()) {
        Ok(contract) => (StatusCode::OK, contract).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn mobile_health(headers: HeaderMap) -> impl IntoResponse {
    let base_urls = request_advertised_mobile_base_urls(&headers);
    Json(serde_json::json!({
        "ok": true,
        "baseURL": base_urls.first().cloned().unwrap_or_default(),
        "baseURLs": base_urls,
        "requiresAuthentication": true,
        "serverTime": current_mobile_time(),
    }))
}

async fn mobile_connection_code(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if !socket_addr.ip().is_loopback() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "message": "Connection codes are only available from this Mac."
            })),
        )
            .into_response();
    }

    let base_urls = request_advertised_mobile_base_urls(&HeaderMap::new());
    match control_plane
        .mobile_auth_service()
        .issue_connection_code(base_urls)
    {
        Ok(connection_code) => (StatusCode::OK, Json(connection_code)).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_connection_orb_png(
    State(control_plane): State<ControlPlane>,
    ConnectInfo(socket_addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    if !socket_addr.ip().is_loopback() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "message": "Connection orbs are only available from this Mac."
            })),
        )
            .into_response();
    }

    let base_urls = request_advertised_mobile_base_urls(&HeaderMap::new());
    match control_plane
        .mobile_auth_service()
        .issue_connection_orb_image(base_urls)
    {
        Ok(orb_image) => png_response(orb_image.png_data),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_connection_orb(
    State(control_plane): State<ControlPlane>,
    Path(orb_id): Path<String>,
) -> impl IntoResponse {
    match control_plane
        .mobile_auth_service()
        .resolve_connection_orb(&orb_id)
    {
        Ok(connection_code) => (StatusCode::OK, Json(connection_code)).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_snapshot_handler(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    mobile_snapshot_response(&control_plane, &headers)
}

async fn mobile_events_handler(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }

    local_desktop_events_stream(control_plane).into_response()
}

fn local_desktop_events_stream(
    control_plane: ControlPlane,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = control_plane.mobile_event_hub().subscribe();
    let mut last_revision = control_plane.mobile_snapshot_revision().unwrap_or_default();
    let mut last_event_ms = control_plane
        .store()
        .latest_mobile_event_created_at_ms()
        .unwrap_or_default();
    let connected_payload = serde_json::to_string(&serde_json::json!({
        "event_type": "connected",
        "server_time": mobile_event_now(),
        "revision": last_revision,
    }))
    .unwrap_or_else(|_| "{}".to_owned());

    let stream = stream! {
        yield Ok::<Event, Infallible>(Event::default().event("connected").data(connected_payload));
        let mut poll_interval = tokio::time::interval(Duration::from_secs(2));
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                received = receiver.recv() => {
                    match received {
                        Ok(event) => {
                            last_event_ms = last_event_ms.max(
                                control_plane.store().latest_mobile_event_created_at_ms().unwrap_or(last_event_ms)
                            );
                            yield Ok::<Event, Infallible>(mobile_sse_event(&event));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = poll_interval.tick() => {
                    if let Ok(records) = control_plane.store().mobile_events_since(last_event_ms, 32) {
                        for record in records {
                            last_event_ms = last_event_ms.max(record.created_at_ms);
                            yield Ok::<Event, Infallible>(mobile_sse_event_from_record(&record));
                        }
                    }

                    if let Ok(revision) = control_plane.mobile_snapshot_revision() {
                        if revision != last_revision {
                            last_revision = revision;
                            yield Ok::<Event, Infallible>(mobile_sse_event(&MobileEvent {
                                event_type: MobileEventKind::SessionChanged,
                                thread_id: None,
                                prompt_id: None,
                                detail: Some("snapshot-revision-changed".to_owned()),
                                server_time: mobile_event_now(),
                            }));
                        }
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn mobile_session_detail_handler(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Query(query): Query<MobileSessionDetailQuery>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    if let Some(assistant_surface) = query.assistant_surface.as_deref() {
        if !ASSISTANT_SURFACES.contains(&assistant_surface) {
            return mobile_session_error_response(MobileSessionError::InvalidAssistantSurface);
        }
    }

    let snapshot = match mobile_desktop_snapshot(&control_plane) {
        Ok(snapshot) => snapshot,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    let session_state = match control_plane.mobile_session_service().state() {
        Ok(session_state) => session_state,
        Err(error) => return mobile_session_error_response(error),
    };
    match mobile_session_detail(
        &snapshot,
        &session_state,
        &thread_id,
        query.assistant_surface.as_deref(),
    ) {
        Some(detail) => (StatusCode::OK, Json(detail)).into_response(),
        None => mobile_session_not_found_response(),
    }
}

async fn mobile_session_mode(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(input): Json<MobileSessionModeRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    if let Some(response) = missing_mobile_session_rejection(&control_plane, &thread_id, None) {
        return response;
    }

    match control_plane
        .mobile_session_service()
        .set_session_preset(&thread_id, input.preset.as_deref())
    {
        Ok(()) => {
            emit_mobile_lifecycle_changed(
                &control_plane,
                &thread_id,
                input.preset.as_deref().or(Some("mode-cleared")),
            );
            emit_mobile_session_changed(&control_plane, Some(&thread_id), Some("mode-updated"));
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_session_archive(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Json(input): Json<MobileSessionArchiveRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    if let Some(response) = missing_mobile_session_rejection(&control_plane, &thread_id, None) {
        return response;
    }

    match control_plane
        .mobile_session_service()
        .set_session_archived(&thread_id, input.archived)
    {
        Ok(()) => {
            emit_mobile_session_changed(
                &control_plane,
                Some(&thread_id),
                Some(if input.archived {
                    "archived"
                } else {
                    "unarchived"
                }),
            );
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_session_delete(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    if let Some(response) = missing_mobile_session_rejection(&control_plane, &thread_id, None) {
        return response;
    }

    match control_plane
        .mobile_session_service()
        .delete_session(&thread_id)
    {
        Ok(()) => {
            emit_mobile_session_changed(&control_plane, Some(&thread_id), Some("deleted"));
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_session_prompt(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
    Query(query): Query<MobileSessionPromptQuery>,
    Json(input): Json<MobileSessionPromptRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    if let Some(assistant_surface) = query.assistant_surface.as_deref() {
        if !ASSISTANT_SURFACES.contains(&assistant_surface) {
            return mobile_session_error_response(MobileSessionError::InvalidAssistantSurface);
        }
    }
    if let Some(response) = missing_mobile_session_rejection(
        &control_plane,
        &thread_id,
        query.assistant_surface.as_deref(),
    ) {
        return response;
    }
    match dispatch_session_prompt(
        &control_plane,
        &thread_id,
        query.assistant_surface.as_deref(),
        &input.prompt,
    ) {
        Ok(dispatch) => {
            emit_prompt_dispatch(&control_plane, &thread_id, &dispatch);
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_session_mute(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }
    if let Some(response) = missing_mobile_session_rejection(&control_plane, &thread_id, None) {
        return response;
    }

    match control_plane
        .mobile_session_service()
        .mute_session(&thread_id)
    {
        Ok(()) => {
            emit_mobile_session_changed(&control_plane, Some(&thread_id), Some("muted"));
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_default_prompt(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<MobileDefaultPromptRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }

    match control_plane
        .mobile_session_service()
        .save_default_prompt(&input.default_prompt)
    {
        Ok(()) => {
            emit_mobile_session_changed(&control_plane, None, Some("default-prompt-updated"));
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_assistant_surface(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<MobileAssistantSurfaceRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }

    match control_plane
        .mobile_session_service()
        .set_assistant_surface(&input.assistant_surface)
    {
        Ok(()) => {
            emit_mobile_lifecycle_changed(
                &control_plane,
                "global",
                Some(input.assistant_surface.as_str()),
            );
            emit_mobile_session_changed(&control_plane, None, Some("assistant-surface-updated"));
            mobile_snapshot_response(&control_plane, &headers)
        }
        Err(error) => mobile_session_error_response(error),
    }
}

async fn mobile_passkey_registration_challenge(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let (service, credential) = match authorize_mobile_request(&control_plane, &headers) {
        Ok(value) => value,
        Err(error) => return mobile_authorization_error_response(error),
    };
    match service.issue_registration_challenge(&credential.id) {
        Ok(challenge) => (StatusCode::OK, Json(challenge)).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_passkey_registration(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<CompleteMobilePasskeyRegistrationInput>,
) -> impl IntoResponse {
    let (service, credential) = match authorize_mobile_request(&control_plane, &headers) {
        Ok(value) => value,
        Err(error) => return mobile_authorization_error_response(error),
    };
    match service.complete_registration(input, &credential.id) {
        Ok(registration) => (StatusCode::OK, Json(registration)).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_passkey_authentication_challenge(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<MobilePasskeyAuthenticationChallengeRequest>,
) -> impl IntoResponse {
    let (service, credential) = match authorize_mobile_request(&control_plane, &headers) {
        Ok(value) => value,
        Err(error) => return mobile_authorization_error_response(error),
    };
    match service.issue_authentication_challenge(&input.credential_id, &credential.id) {
        Ok(challenge) => (StatusCode::OK, Json(challenge)).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_passkey_authentication(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<CompleteMobilePasskeyAuthenticationInput>,
) -> impl IntoResponse {
    let (service, credential) = match authorize_mobile_request(&control_plane, &headers) {
        Ok(value) => value,
        Err(error) => return mobile_authorization_error_response(error),
    };
    match service.complete_authentication(input, &credential.id) {
        Ok(authentication) => (StatusCode::OK, Json(authentication)).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_passkey_revocation(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Path(credential_id): Path<String>,
) -> impl IntoResponse {
    let (service, credential) = match authorize_mobile_request(&control_plane, &headers) {
        Ok(value) => value,
        Err(error) => return mobile_authorization_error_response(error),
    };
    match service.revoke_credential(&credential_id, &credential.id) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "ok": true }))).into_response(),
        Err(error) => mobile_auth_error_response(error),
    }
}

async fn mobile_push_registration(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<MobilePushRegistrationRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }

    match control_plane.mobile_push_service().register_device(input) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => mobile_push_error_response(error),
    }
}

async fn mobile_push_test(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
    Json(input): Json<MobilePushTestRequest>,
) -> impl IntoResponse {
    if let Err(error) = authorize_mobile_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }

    match control_plane
        .mobile_push_service()
        .send_test_push(&input.installation_id)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => mobile_push_error_response(error),
    }
}

async fn threads(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.threads_response() {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn thread_detail(
    State(control_plane): State<ControlPlane>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    match control_plane.thread_detail(&thread_id) {
        Ok(Some(detail)) => (StatusCode::OK, Json(detail)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown thread" })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn thread_capabilities(
    State(control_plane): State<ControlPlane>,
    Path(thread_id): Path<String>,
) -> impl IntoResponse {
    match control_plane.capabilities(&thread_id) {
        Ok(capabilities) => (StatusCode::OK, Json(capabilities)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
}

async fn events_tail(
    State(control_plane): State<ControlPlane>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let stream = stream! {
        loop {
            let runs = control_plane
                .store()
                .automation_runs()
                .unwrap_or_default();
            let payload = serde_json::to_string(&serde_json::json!({
                "event_type": "automation.snapshot",
                "runs": runs,
            }))
            .unwrap_or_else(|_| "{}".to_owned());
            yield Ok(Event::default().event("automation.snapshot").data(payload));
            for compaction in control_plane.compactions().unwrap_or_default().into_iter().take(50) {
                let payload = serde_json::to_string(&compaction).unwrap_or_else(|_| "{}".to_owned());
                yield Ok(Event::default().event("codex.context_compacted").data(payload));
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn desktop_mobile_state_response(control_plane: &ControlPlane) -> Response {
    match control_plane.mobile_session_service().state() {
        Ok(state) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => mobile_session_error_response(error),
    }
}

fn new_record_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

fn desktop_pairing_response(connection_code: &MobileConnectionCode) -> serde_json::Value {
    let orb_image_path = format!("/desktop/pairing-orbs/{}", connection_code.orb_id);
    let orb_image_url = format!(
        "{}{}",
        crate::runtime::default_server_base_url(),
        orb_image_path
    );
    serde_json::json!({
        "baseURL": &connection_code.base_url,
        "baseURLs": &connection_code.base_urls,
        "pairingTokenId": &connection_code.pairing_token_id,
        "code": &connection_code.code,
        "orbId": &connection_code.orb_id,
        "generatedAt": &connection_code.generated_at,
        "expiresInSeconds": CONNECTION_ORB_TTL_SECONDS,
        "orbImagePath": orb_image_path,
        "orbImageURL": orb_image_url,
        "orbResolvePath": format!("/api/mobile/connection-orbs/{}", connection_code.orb_id),
    })
}

fn png_response(png_data: Vec<u8>) -> Response {
    let mut response = (StatusCode::OK, png_data).into_response();
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/png"));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn mobile_snapshot_response(control_plane: &ControlPlane, headers: &HeaderMap) -> Response {
    let snapshot = match mobile_desktop_snapshot(control_plane) {
        Ok(snapshot) => snapshot,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    let session_state = match control_plane.mobile_session_service().state() {
        Ok(session_state) => session_state,
        Err(error) => return mobile_session_error_response(error),
    };
    let base_urls = request_advertised_mobile_base_urls(headers);

    (
        StatusCode::OK,
        Json(mobile_snapshot(
            &snapshot,
            &session_state,
            base_urls.first().map(String::as_str).unwrap_or_default(),
            &current_mobile_time(),
        )),
    )
        .into_response()
}

fn missing_mobile_session_rejection(
    control_plane: &ControlPlane,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> Option<Response> {
    let snapshot = match mobile_desktop_snapshot(control_plane) {
        Ok(snapshot) => snapshot,
        Err(error) => return Some(internal_mobile_error_response(error.to_string())),
    };
    let session_state = match control_plane.mobile_session_service().state() {
        Ok(session_state) => session_state,
        Err(error) => return Some(mobile_session_error_response(error)),
    };
    if mobile_session_is_visible(&snapshot, &session_state, thread_id, assistant_surface) {
        return None;
    }

    Some(mobile_session_not_found_response())
}

fn mobile_desktop_snapshot(control_plane: &ControlPlane) -> anyhow::Result<DesktopSnapshot> {
    control_plane.desktop_menu_snapshot()
}

fn mobile_session_is_visible(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> bool {
    mobile_session_detail(snapshot, session_state, thread_id, assistant_surface).is_some()
}

fn handoff_session_html(thread: &DesktopThread, handoff_base_url: Option<&str>) -> String {
    let title = handoff_session_title(thread);
    let subtitle = thread
        .cwd
        .as_deref()
        .map(handoff_project_name)
        .unwrap_or_else(|| "Looper session".to_owned());
    let preview = thread
        .assistant_preview
        .as_deref()
        .map(str::trim)
        .filter(|preview| !preview.is_empty())
        .unwrap_or("Open this session in Looper on your iPhone.");
    let deep_link = handoff_deep_link(&thread.thread_id, handoff_base_url);

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif; }}
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }}
main {{ width: min(34rem, calc(100vw - 2rem)); }}
h1 {{ font-size: 1.35rem; line-height: 1.2; margin: 0 0 .5rem; }}
p {{ color: color-mix(in srgb, CanvasText 72%, transparent); line-height: 1.45; }}
a {{ display: inline-block; margin-top: 1rem; padding: .7rem .95rem; border-radius: .75rem; background: LinkText; color: Canvas; text-decoration: none; font-weight: 650; }}
</style>
</head>
<body>
<main>
<h1>{title}</h1>
<p>{subtitle}</p>
<p>{preview}</p>
<a href="{deep_link}">Open in looper</a>
</main>
</body>
</html>"#,
        title = html_escaped_text(&title),
        subtitle = html_escaped_text(&subtitle),
        preview = html_escaped_text(preview),
        deep_link = html_escaped_attribute(&deep_link)
    )
}

fn handoff_deep_link(thread_id: &str, handoff_base_url: Option<&str>) -> String {
    let encoded_thread_id = percent_encoded_path_segment(thread_id);
    let Some(handoff_base_url) = handoff_base_url else {
        return format!("looper://session/{encoded_thread_id}");
    };

    format!(
        "looper://session/{encoded_thread_id}?baseURL={}",
        percent_encoded_url_component(handoff_base_url)
    )
}

fn handoff_session_title(thread: &DesktopThread) -> String {
    thread
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or(&thread.thread_id)
        .to_owned()
}

fn handoff_project_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}

fn html_escaped_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_escaped_attribute(value: &str) -> String {
    html_escaped_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn percent_encoded_path_segment(value: &str) -> String {
    percent_encoded_url_component(value)
}

fn percent_encoded_url_component(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect()
}

fn emit_mobile_session_changed(
    control_plane: &ControlPlane,
    thread_id: Option<&str>,
    detail: Option<&str>,
) {
    control_plane.emit_mobile_event(MobileEventInput {
        kind: MobileEventKind::SessionChanged,
        thread_id: thread_id.map(str::to_owned),
        prompt_id: None,
        detail: detail.map(str::to_owned),
    });
}

fn emit_mobile_lifecycle_changed(
    control_plane: &ControlPlane,
    thread_id: &str,
    detail: Option<&str>,
) {
    control_plane.emit_mobile_event(MobileEventInput {
        kind: MobileEventKind::LifecycleChanged,
        thread_id: Some(thread_id.to_owned()),
        prompt_id: None,
        detail: detail.map(str::to_owned),
    });
}

fn emit_mobile_prompt_queued(control_plane: &ControlPlane, thread_id: &str, prompt_id: &str) {
    control_plane.emit_mobile_event(MobileEventInput {
        kind: MobileEventKind::PromptQueued,
        thread_id: Some(thread_id.to_owned()),
        prompt_id: Some(prompt_id.to_owned()),
        detail: None,
    });
    emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-queued"));
}

fn emit_prompt_dispatch(control_plane: &ControlPlane, thread_id: &str, dispatch: &PromptDispatch) {
    match dispatch {
        PromptDispatch::Delivered { prompt_id } => {
            control_plane.emit_mobile_event(MobileEventInput {
                kind: MobileEventKind::PromptDelivered,
                thread_id: Some(thread_id.to_owned()),
                prompt_id: Some(prompt_id.to_owned()),
                detail: Some("devin-acp".to_owned()),
            });
            emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-delivered"));
        }
        PromptDispatch::Queued { prompt_id } => {
            emit_mobile_prompt_queued(control_plane, thread_id, prompt_id);
        }
        PromptDispatch::Resumed => {
            emit_mobile_session_changed(control_plane, Some(thread_id), Some("prompt-resumed"));
        }
    }
}

fn mobile_sse_event(event: &MobileEvent) -> Event {
    let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_owned());
    Event::default()
        .event(mobile_event_sse_name(event.event_type))
        .data(payload)
}

fn mobile_sse_event_from_record(record: &MobileEventRecord) -> Event {
    mobile_sse_event(&MobileEvent {
        event_type: record.event_type,
        thread_id: record.thread_id.clone(),
        prompt_id: record.prompt_id.clone(),
        detail: record.detail.clone(),
        server_time: mobile_event_now(),
    })
}

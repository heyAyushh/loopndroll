use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Sse};
use axum::{
    Json, Router,
    routing::{get, post},
};

use crate::control_plane::ControlPlane;
use crate::hook_integration::{HookBridgeContract, hook_bridge_contract_toml};

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
        .route("/sync/manifest", get(sync_manifest))
        .route("/hooks/clear", post(unregister_hooks))
        .route("/hooks/register", post(register_hooks))
        .route("/hooks/unregister", post(unregister_hooks))
        .route("/hooks/unregister-live", post(unregister_live_hooks))
        .route("/integrations/hook/contract", get(hook_contract))
        .route("/integrations/hook/contract.toml", get(hook_contract_toml))
        .route("/threads", get(threads))
        .route("/threads/:thread_id", get(thread_detail))
        .route("/threads/:thread_id/capabilities", get(thread_capabilities))
        .route("/events/tail", get(events_tail))
        .with_state(control_plane)
}

async fn health(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    let status = control_plane.status();
    Json(serde_json::json!({
        "ok": status.source.health == "healthy",
        "source": status.source,
        "hooks": status.hooks,
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

async fn desktop_snapshot(State(control_plane): State<ControlPlane>) -> impl IntoResponse {
    match control_plane.desktop_snapshot() {
        Ok(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": error.to_string() })),
        )
            .into_response(),
    }
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

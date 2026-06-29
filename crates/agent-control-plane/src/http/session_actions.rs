use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
const HTTP_SESSION_STATE_MUTATION_DISABLED_ERROR: &str = "http_session_state_mutation_disabled";
const HTTP_SESSION_STATE_MUTATION_DISABLED_MESSAGE: &str = "HTTP session/state mutations are disabled; use the Session stream for hot commands and state changes.";
const SESSION_STATE_RECOVERY_SNAPSHOT_PATH: &str = "/api/mobile/session-minis/snapshot";

pub(super) fn disabled_http_session_state_mutation_response() -> Response {
    (
        StatusCode::GONE,
        Json(serde_json::json!({
            "error": HTTP_SESSION_STATE_MUTATION_DISABLED_ERROR,
            "message": HTTP_SESSION_STATE_MUTATION_DISABLED_MESSAGE,
            "recovery": SESSION_STATE_RECOVERY_SNAPSHOT_PATH,
        })),
    )
        .into_response()
}

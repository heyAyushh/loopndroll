use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use tonic::Status;

use crate::control_plane::ControlPlane;
use crate::mobile::realtime_commands::set_default_notification_targets_command;

const HTTP_SESSION_STATE_MUTATION_DISABLED_ERROR: &str = "http_session_state_mutation_disabled";
const HTTP_SESSION_STATE_MUTATION_DISABLED_MESSAGE: &str = "HTTP session/state mutations are disabled; use the Session stream for hot commands and state changes.";
const SESSION_STATE_RECOVERY_SNAPSHOT_PATH: &str = "/api/mobile/session-minis/snapshot";
const HTTP_DEFAULT_NOTIFICATION_TARGETS_MUTATION_PREFIX: &str = "http-default-notification-targets";

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

pub(super) fn set_default_notification_targets(
    control_plane: &ControlPlane,
    notification_target_ids: Vec<String>,
) -> Result<(), Status> {
    set_default_notification_targets_command(
        control_plane,
        notification_target_ids,
        &http_session_mutation_id(HTTP_DEFAULT_NOTIFICATION_TARGETS_MUTATION_PREFIX),
    )
    .map_err(|error| error.into_status())
    .map(|_| ())
}

fn http_session_mutation_id(prefix: &str) -> String {
    format!("{prefix}-{}", uuid::Uuid::new_v4())
}

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::mobile::auth::MobileAuthError;
use crate::mobile::push::MobilePushError;
use crate::mobile::session::MobileSessionError;
use crate::telegram::TelegramError;

pub(super) fn internal_mobile_error_response(message: String) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "message": message })),
    )
        .into_response()
}

pub(super) fn mobile_session_not_found_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "message": "Session not found." })),
    )
        .into_response()
}

pub(super) fn mobile_authorization_error_response(error: MobileAuthError) -> Response {
    match error {
        MobileAuthError::PairingTokenRequired => {
            return unauthorized_mobile_response(
                "pairing_token_required",
                "Pair this iPhone with the Mac before using the mobile API.",
            );
        }
        MobileAuthError::PasskeySessionRequired => {
            return unauthorized_mobile_response(
                "passkey_session_required",
                "Unlock looper with Face ID before using the mobile API.",
            );
        }
        _ => {}
    }
    mobile_auth_error_response(error)
}

pub(super) fn mobile_auth_error_response(error: MobileAuthError) -> Response {
    let status = match error {
        MobileAuthError::PairingTokenRequired | MobileAuthError::PasskeySessionRequired => {
            StatusCode::UNAUTHORIZED
        }
        MobileAuthError::CredentialNotRegistered | MobileAuthError::ConnectionOrbNotFound => {
            StatusCode::NOT_FOUND
        }
        MobileAuthError::ConnectionOrbExpired => StatusCode::GONE,
        MobileAuthError::Store(_)
        | MobileAuthError::Filesystem(_)
        | MobileAuthError::TimeFormat(_)
        | MobileAuthError::Json(_)
        | MobileAuthError::Orb(_) => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    error_response(status, error.to_string())
}

pub(super) fn mobile_session_error_response(error: MobileSessionError) -> Response {
    let status = match error {
        MobileSessionError::Store(_)
        | MobileSessionError::Filesystem(_)
        | MobileSessionError::TimeFormat(_)
        | MobileSessionError::PromptSnapshotUnavailable(_) => StatusCode::INTERNAL_SERVER_ERROR,
        MobileSessionError::PromptResumeUnavailable(_) => StatusCode::BAD_GATEWAY,
        MobileSessionError::SessionNotFound => StatusCode::NOT_FOUND,
        MobileSessionError::InvalidPreset
        | MobileSessionError::InvalidScope
        | MobileSessionError::InvalidAssistantSurface
        | MobileSessionError::PromptRequired
        | MobileSessionError::ModeRequired
        | MobileSessionError::SessionArchived
        | MobileSessionError::InvalidNotificationChannel
        | MobileSessionError::MissingNotificationConfig
        | MobileSessionError::InvalidCompletionCheck => StatusCode::BAD_REQUEST,
        MobileSessionError::PromptDeliveryUnavailable
        | MobileSessionError::PromptDeliveryUnavailableReason(_) => StatusCode::CONFLICT,
        MobileSessionError::NotificationNotFound | MobileSessionError::CompletionCheckNotFound => {
            StatusCode::NOT_FOUND
        }
    };
    error_response(status, error.to_string())
}

pub(super) fn telegram_error_response(error: TelegramError) -> Response {
    let status = match error {
        TelegramError::Store(_) | TelegramError::Filesystem(_) | TelegramError::TimeFormat(_) => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
        TelegramError::BotTokenRequired => StatusCode::BAD_REQUEST,
        TelegramError::Request(_) | TelegramError::Api(_) => StatusCode::BAD_GATEWAY,
    };
    error_response(status, error.to_string())
}

pub(super) fn mobile_push_error_response(error: MobilePushError) -> Response {
    let status = match error {
        MobilePushError::Store(_)
        | MobilePushError::Filesystem(_)
        | MobilePushError::TimeFormat(_)
        | MobilePushError::Http(_)
        | MobilePushError::Config(_)
        | MobilePushError::Jwt(_) => StatusCode::INTERNAL_SERVER_ERROR,
        MobilePushError::MissingRequiredValues => StatusCode::BAD_REQUEST,
    };
    error_response(status, error.to_string())
}

fn unauthorized_mobile_response(code: &str, message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "code": code,
            "message": message
        })),
    )
        .into_response()
}

fn error_response(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(serde_json::json!({
            "message": message.into()
        })),
    )
        .into_response()
}

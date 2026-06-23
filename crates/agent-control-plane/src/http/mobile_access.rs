use std::net::SocketAddr;

use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use time::format_description::well_known::Rfc3339;

use crate::control_plane::ControlPlane;
use crate::mobile::auth::{
    MobileAuthError, MobileAuthorizationCredential, parse_mobile_authorization_header,
};
use crate::mobile::network::{advertised_mobile_base_urls, advertised_mobile_pairing_base_urls};

const AUTHORIZATION_HEADER: &str = "authorization";
const MOBILE_SESSION_HEADER: &str = "x-looper-mobile-session";
const HOST_HEADER: &str = "host";
const FORWARDED_HOST_HEADER: &str = "x-forwarded-host";
const FORWARDED_PROTO_HEADER: &str = "x-forwarded-proto";
const DEFAULT_REQUEST_SCHEME: &str = "http";

pub(super) fn desktop_loopback_rejection(socket_addr: SocketAddr) -> Option<Response> {
    if socket_addr.ip().is_loopback() {
        return None;
    }

    Some(
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "message": "Desktop connection management is only available from this Mac."
            })),
        )
            .into_response(),
    )
}

pub(super) fn authorize_mobile_request(
    control_plane: &ControlPlane,
    headers: &HeaderMap,
) -> Result<
    (
        crate::mobile::auth::MobileAuthService,
        MobileAuthorizationCredential,
    ),
    MobileAuthError,
> {
    let service = control_plane.mobile_auth_service();
    let credential = parse_mobile_authorization_header(header_value(headers, AUTHORIZATION_HEADER))
        .ok_or(MobileAuthError::PairingTokenRequired)?;
    if service.validate_authorization_credential(Some(&credential))? {
        return Ok((service, credential));
    }
    Err(MobileAuthError::PairingTokenRequired)
}

pub(super) fn authorize_mobile_api_request(
    control_plane: &ControlPlane,
    headers: &HeaderMap,
) -> Result<
    (
        crate::mobile::auth::MobileAuthService,
        MobileAuthorizationCredential,
    ),
    MobileAuthError,
> {
    let (service, credential) = authorize_mobile_request(control_plane, headers)?;
    service.validate_api_session_header(
        header_value(headers, MOBILE_SESSION_HEADER),
        &credential.id,
    )?;
    Ok((service, credential))
}

pub(super) fn request_advertised_mobile_base_urls(headers: &HeaderMap) -> Vec<String> {
    advertised_mobile_base_urls(request_base_url(headers).as_deref())
}

pub(super) async fn request_advertised_mobile_pairing_base_urls(
    headers: &HeaderMap,
) -> Vec<String> {
    advertised_mobile_pairing_base_urls(request_base_url(headers).as_deref()).await
}

pub(super) fn current_mobile_time() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn request_base_url(headers: &HeaderMap) -> Option<String> {
    let host = header_value(headers, FORWARDED_HOST_HEADER)
        .or_else(|| header_value(headers, HOST_HEADER))?;
    let scheme = header_value(headers, FORWARDED_PROTO_HEADER).unwrap_or(DEFAULT_REQUEST_SCHEME);
    Some(format!("{scheme}://{host}"))
}

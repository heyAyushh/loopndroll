use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::control_plane::{ControlPlane, DesktopSnapshot};
use crate::mobile::api::{mobile_session_detail, mobile_snapshot};
use crate::mobile::events::{MobileEventInput, MobileEventKind};
use crate::mobile::network::advertised_mobile_grpc_base_urls;
use crate::mobile::prompt_delivery::mobile_desktop_snapshot;
use crate::mobile::session::MobileSessionState;

use super::mobile_access::{current_mobile_time, request_advertised_mobile_base_urls};
use super::responses::{
    internal_mobile_error_response, mobile_session_error_response,
    mobile_session_not_found_response,
};

pub(super) fn desktop_mobile_state_response(control_plane: &ControlPlane) -> Response {
    match control_plane.mobile_session_service().state() {
        Ok(state) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => mobile_session_error_response(error),
    }
}

pub(super) fn mobile_snapshot_response(
    control_plane: &ControlPlane,
    headers: &HeaderMap,
) -> Response {
    let snapshot = match mobile_desktop_snapshot(control_plane) {
        Ok(snapshot) => snapshot,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    let session_state = match control_plane.mobile_session_service().state() {
        Ok(session_state) => session_state,
        Err(error) => return mobile_session_error_response(error),
    };
    let base_urls = request_advertised_mobile_base_urls(headers);
    let grpc_base_urls = advertised_mobile_grpc_base_urls(&base_urls);

    (
        StatusCode::OK,
        Json(mobile_snapshot(
            &snapshot,
            &session_state,
            base_urls.first().map(String::as_str).unwrap_or_default(),
            &grpc_base_urls,
            &current_mobile_time(),
        )),
    )
        .into_response()
}

pub(super) fn missing_mobile_session_rejection(
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

pub(super) fn emit_mobile_session_changed(
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

pub(super) fn emit_mobile_lifecycle_changed(
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

fn mobile_session_is_visible(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> bool {
    mobile_session_detail(snapshot, session_state, thread_id, assistant_surface).is_some()
}

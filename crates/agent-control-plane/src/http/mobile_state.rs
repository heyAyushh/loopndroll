use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};

use crate::control_plane::ControlPlane;
use crate::events::MobileStateEventGap;
use crate::mobile::api::{
    mobile_session_mini_delta, mobile_session_mini_snapshot, mobile_snapshot,
    session_mini_projection_inputs,
};
use crate::mobile::network::advertised_mobile_grpc_base_urls;
use crate::mobile::prompt_delivery::{mobile_desktop_snapshot, prime_delivery_action_cache};

use super::mobile_access::{current_mobile_time, request_advertised_mobile_base_urls};
use super::responses::{internal_mobile_error_response, mobile_session_error_response};

pub(super) const DEFAULT_SESSION_MINI_REPLAY_LIMIT: usize = 100;
pub(super) const MAX_SESSION_MINI_REPLAY_LIMIT: usize = 500;

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
    prime_delivery_action_cache(control_plane, &snapshot, &session_state);
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

pub(super) fn mobile_session_minis_snapshot_response(control_plane: &ControlPlane) -> Response {
    match cached_mobile_session_mini_projection(control_plane) {
        Ok(Some((latest_seq, records))) => {
            return (
                StatusCode::OK,
                Json(mobile_session_mini_snapshot(latest_seq, &records)),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(error) => return internal_mobile_error_response(error),
    }

    match rebuild_mobile_session_mini_projection(control_plane) {
        Ok((latest_seq, records)) => (
            StatusCode::OK,
            Json(mobile_session_mini_snapshot(latest_seq, &records)),
        )
            .into_response(),
        Err(error) => internal_mobile_error_response(error),
    }
}

pub(super) fn mobile_session_minis_delta_response(
    control_plane: &ControlPlane,
    after_seq: i64,
    limit: Option<usize>,
) -> Response {
    let limit = limit
        .unwrap_or(DEFAULT_SESSION_MINI_REPLAY_LIMIT)
        .clamp(1, MAX_SESSION_MINI_REPLAY_LIMIT);
    let records = match control_plane
        .store()
        .mobile_session_minis_after_seq(after_seq, limit)
    {
        Ok(records) => records,
        Err(error) => {
            if let Some(gap) = error.downcast_ref::<MobileStateEventGap>() {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({
                        "error": "seq_gap",
                        "latest_seq": gap.latest_seq,
                        "latestSeq": gap.latest_seq,
                        "requested_after_seq": gap.requested_after_seq,
                        "recovery": "/api/mobile/session-minis/snapshot",
                    })),
                )
                    .into_response();
            }
            return internal_mobile_error_response(error.to_string());
        }
    };
    let all_records = match control_plane.store().mobile_session_minis() {
        Ok(records) => records,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    match control_plane.store().latest_mobile_state_event_seq() {
        Ok(latest_seq) => {
            let has_changes = latest_seq > after_seq;
            let delta_contains_complete_projection = has_changes
                && records.len() == all_records.len()
                && records.iter().all(|record| record.seq > after_seq);
            let (payload_records, replace) = if has_changes && !delta_contains_complete_projection {
                (all_records.as_slice(), true)
            } else {
                (records.as_slice(), delta_contains_complete_projection)
            };
            (
                StatusCode::OK,
                Json(mobile_session_mini_delta(
                    latest_seq,
                    payload_records,
                    replace,
                )),
            )
                .into_response()
        }
        Err(error) => internal_mobile_error_response(error.to_string()),
    }
}

fn rebuild_mobile_session_mini_projection(
    control_plane: &ControlPlane,
) -> Result<(i64, Vec<crate::events::MobileSessionMiniRecord>), String> {
    let snapshot = mobile_desktop_snapshot(control_plane).map_err(|error| error.to_string())?;
    let session_state = control_plane
        .mobile_session_service()
        .state()
        .map_err(|error| error.to_string())?;
    prime_delivery_action_cache(control_plane, &snapshot, &session_state);
    let queued_prompt_counts = control_plane
        .mobile_session_service()
        .queued_prompt_counts()
        .map_err(|error| error.to_string())?;
    let latest_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .map_err(|error| error.to_string())?;
    let minis = session_mini_projection_inputs(
        &snapshot,
        &session_state,
        &queued_prompt_counts,
        latest_seq,
        &snapshot.revision,
    );
    let records = control_plane
        .store()
        .replace_mobile_session_minis(minis, latest_seq, &snapshot.revision)
        .map_err(|error| error.to_string())?;
    Ok((latest_seq, records))
}

fn cached_mobile_session_mini_projection(
    control_plane: &ControlPlane,
) -> Result<Option<(i64, Vec<crate::events::MobileSessionMiniRecord>)>, String> {
    let records = control_plane
        .store()
        .mobile_session_minis()
        .map_err(|error| error.to_string())?;
    if records.is_empty() {
        return Ok(None);
    }

    let latest_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .map_err(|error| error.to_string())?;
    let latest_projection_seq = records
        .iter()
        .map(|record| record.seq)
        .max()
        .unwrap_or_default();
    if latest_projection_seq < latest_seq {
        return Ok(None);
    }
    let has_complete_replacement_at_latest_seq = control_plane
        .store()
        .mobile_session_minis_replaced_at_seq(latest_seq)
        .map_err(|error| error.to_string())?;
    if !has_complete_replacement_at_latest_seq {
        return Ok(None);
    }

    Ok(Some((latest_seq, records)))
}

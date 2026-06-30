use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::control_plane::ControlPlane;
use crate::events::{MobileSessionMiniRecord, MobileStateEventGap};
use crate::mobile::api::{
    latest_session_mini_revision, mobile_session_mini_delta, mobile_session_mini_snapshot,
    mobile_snapshot,
};
use crate::mobile::network::advertised_mobile_grpc_base_urls;

use super::mobile_access::{current_mobile_time, request_advertised_mobile_base_urls};
use super::responses::{internal_mobile_error_response, mobile_session_error_response};

pub(super) const DEFAULT_SESSION_MINI_REPLAY_LIMIT: usize = 100;
pub(super) const MAX_SESSION_MINI_REPLAY_LIMIT: usize = 500;
const SESSION_MINI_RECOVERY_PATH: &str = "/api/mobile/session-minis/snapshot";
const SESSION_MINI_FRESHNESS_SOURCE: &str = "mobile-session-mini-projection";
const MOBILE_STATE_REVISION_PREFIX: &str = "mobile-state:seq-";

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
    let snapshot = match control_plane.desktop_mobile_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    let session_state = match control_plane.mobile_session_service().state() {
        Ok(session_state) => session_state,
        Err(error) => return mobile_session_error_response(error),
    };
    let base_urls = request_advertised_mobile_base_urls(headers);
    let grpc_base_urls = advertised_mobile_grpc_base_urls(&base_urls);
    let latest_seq = match control_plane.store().latest_mobile_state_event_seq() {
        Ok(latest_seq) => latest_seq,
        Err(error) => return internal_mobile_error_response(error.to_string()),
    };
    let server_time = current_mobile_time();

    (
        StatusCode::OK,
        Json(mobile_snapshot(
            &snapshot,
            &session_state,
            base_urls.first().map(String::as_str).unwrap_or_default(),
            &grpc_base_urls,
            latest_seq,
            &server_time,
        )),
    )
        .into_response()
}

pub(super) fn mobile_session_minis_snapshot_response(control_plane: &ControlPlane) -> Response {
    match cached_mobile_session_mini_projection(control_plane) {
        Ok(Some((latest_seq, records))) => {
            let revision = session_mini_recovery_revision(control_plane, latest_seq, &records);
            let server_time = current_mobile_time();
            return (
                StatusCode::OK,
                Json(mobile_session_mini_snapshot(
                    latest_seq,
                    &revision,
                    &server_time,
                    &records,
                )),
            )
                .into_response();
        }
        Ok(None) => {}
        Err(error) => return internal_mobile_error_response(error),
    }

    mobile_session_minis_recovery_required_response(control_plane)
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
                let revision = latest_mobile_state_revision(control_plane, gap.latest_seq);
                let server_time = current_mobile_time();
                return (
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "seq_gap",
                        "latest_seq": gap.latest_seq,
                        "latestSeq": gap.latest_seq,
                        "requested_after_seq": gap.requested_after_seq,
                        "requestedAfterSeq": gap.requested_after_seq,
                        "revision": revision,
                        "server_time": server_time,
                        "serverTime": server_time,
                        "freshness": session_mini_freshness(
                            gap.latest_seq,
                            &revision,
                            &server_time,
                        ),
                        "recovery": SESSION_MINI_RECOVERY_PATH,
                    })),
                )
                    .into_response();
            }
            return internal_mobile_error_response(error.to_string());
        }
    };
    let (latest_projection_seq, all_records) =
        match cached_mobile_session_mini_projection(control_plane) {
            Ok(Some(projection)) => projection,
            Ok(None) => return mobile_session_minis_recovery_required_response(control_plane),
            Err(error) => return internal_mobile_error_response(error),
        };
    match control_plane.store().latest_mobile_state_event_seq() {
        Ok(latest_event_seq) => {
            let latest_seq = latest_projection_seq.min(latest_event_seq);
            let revision = session_mini_recovery_revision(control_plane, latest_seq, &all_records);
            let server_time = current_mobile_time();
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
                    &revision,
                    &server_time,
                    payload_records,
                    replace,
                )),
            )
                .into_response()
        }
        Err(error) => internal_mobile_error_response(error.to_string()),
    }
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

    let latest_event_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .map_err(|error| error.to_string())?;
    let latest_projection_seq = latest_session_mini_projection_seq(&records);
    let has_produced_replacement_baseline = control_plane
        .store()
        .latest_mobile_session_mini_replacement_event_seq_after(-1)
        .map_err(|error| error.to_string())?;
    if has_produced_replacement_baseline.is_none() {
        return Ok(None);
    }

    Ok(Some((latest_projection_seq.min(latest_event_seq), records)))
}

fn mobile_session_minis_recovery_required_response(control_plane: &ControlPlane) -> Response {
    let latest_seq = control_plane
        .store()
        .latest_mobile_state_event_seq()
        .unwrap_or_default();
    let revision = latest_mobile_state_revision(control_plane, latest_seq);
    let server_time = current_mobile_time();
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "recovery_required",
            "message": "Session mini projection is not ready; wait for the producer projection event and retry.",
            "latest_seq": latest_seq,
            "latestSeq": latest_seq,
            "revision": revision,
            "server_time": server_time,
            "serverTime": server_time,
            "freshness": session_mini_freshness(latest_seq, &revision, &server_time),
            "recovery": SESSION_MINI_RECOVERY_PATH,
        })),
    )
        .into_response()
}

fn session_mini_recovery_revision(
    control_plane: &ControlPlane,
    latest_seq: i64,
    records: &[MobileSessionMiniRecord],
) -> String {
    latest_session_mini_revision(records).unwrap_or_else(|| {
        control_plane
            .store()
            .latest_mobile_session_mini_revision()
            .ok()
            .flatten()
            .unwrap_or_else(|| fallback_mobile_state_revision(latest_seq))
    })
}

fn latest_session_mini_projection_seq(records: &[MobileSessionMiniRecord]) -> i64 {
    records
        .iter()
        .map(|record| record.seq)
        .max()
        .unwrap_or_default()
}

fn latest_mobile_state_revision(control_plane: &ControlPlane, latest_seq: i64) -> String {
    control_plane
        .store()
        .latest_mobile_session_mini_revision()
        .ok()
        .flatten()
        .unwrap_or_else(|| fallback_mobile_state_revision(latest_seq))
}

fn fallback_mobile_state_revision(latest_seq: i64) -> String {
    format!("{MOBILE_STATE_REVISION_PREFIX}{latest_seq}")
}

fn session_mini_freshness(latest_seq: i64, revision: &str, server_time: &str) -> Value {
    json!({
        "source": SESSION_MINI_FRESHNESS_SOURCE,
        "latest_seq": latest_seq,
        "latestSeq": latest_seq,
        "revision": revision,
        "server_time": server_time,
        "serverTime": server_time,
    })
}

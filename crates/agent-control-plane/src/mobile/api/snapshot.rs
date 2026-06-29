use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::control_plane::DesktopSnapshot;
use crate::mobile::session::{ASSISTANT_SURFACES, MobileSessionState};

use super::assistant_identity::thread_matches_assistant_surface;
use super::overrides::is_deleted;
use super::settings::{completion_check_summary, mobile_global_settings, notification_summary};
use super::status::{mobile_devin_desktop_status, mobile_grok_build_status};
use super::summary::session_summary;
use super::work_status::mobile_work_status;

const HOST_ID: &str = "rust-control-plane";
const HOST_NAME: &str = "Looper";

pub fn mobile_snapshot(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    base_url: &str,
    grpc_base_urls: &[String],
    latest_seq: i64,
    synced_at: &str,
) -> Value {
    let surface_sessions = mobile_surface_sessions(snapshot, session_state);
    let sessions = surface_sessions
        .get(&session_state.assistant_surface)
        .cloned()
        .unwrap_or_default();
    let revision = snapshot.revision.as_str();

    json!({
        "revision": revision,
        "latest_seq": latest_seq,
        "latestSeq": latest_seq,
        "server_time": synced_at,
        "serverTime": synced_at,
        "snapshot_kind": "bootstrapRecovery",
        "snapshotKind": "bootstrapRecovery",
        "freshness": {
            "source": "desktop-mobile-snapshot",
            "latest_seq": latest_seq,
            "latestSeq": latest_seq,
            "revision": revision,
            "server_time": synced_at,
            "serverTime": synced_at,
        },
        "host": host_summary(base_url, grpc_base_urls, synced_at),
        "globalSettings": mobile_global_settings(session_state),
        "sessions": sessions,
        "surfaceSessions": surface_sessions,
        "notifications": session_state
            .notifications
            .iter()
            .map(notification_summary)
            .collect::<Vec<_>>(),
        "completionChecks": session_state
            .completion_checks
            .iter()
            .map(completion_check_summary)
            .collect::<Vec<_>>(),
        "workStatus": mobile_work_status(&snapshot.goals, &snapshot.automations),
        "devinDesktop": mobile_devin_desktop_status(
            &snapshot.devin_desktop,
            snapshot.devin_session_count,
            snapshot.devin_active_session_count,
            &snapshot.devin_session_errors,
        ),
        "grokBuild": mobile_grok_build_status(&snapshot.grok_build),
    })
}

fn mobile_surface_sessions(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
) -> BTreeMap<String, Vec<Value>> {
    ASSISTANT_SURFACES
        .iter()
        .map(|surface| {
            (
                (*surface).to_owned(),
                mobile_sessions_for_surface(snapshot, session_state, surface),
            )
        })
        .collect()
}

fn mobile_sessions_for_surface(
    snapshot: &DesktopSnapshot,
    session_state: &MobileSessionState,
    surface: &str,
) -> Vec<Value> {
    snapshot
        .threads
        .iter()
        .enumerate()
        .filter(|(_, thread)| !is_deleted(thread, session_state))
        .filter(|(_, thread)| thread_matches_assistant_surface(thread, surface))
        .map(|(index, thread)| session_summary(thread, index, session_state))
        .collect()
}

fn host_summary(base_url: &str, grpc_base_urls: &[String], synced_at: &str) -> Value {
    json!({
        "id": HOST_ID,
        "name": HOST_NAME,
        "address": base_url,
        "grpcAddress": grpc_base_urls.first().cloned().unwrap_or_default(),
        "grpcAddresses": grpc_base_urls,
        "isReachable": true,
        "lastSyncedAt": synced_at,
    })
}

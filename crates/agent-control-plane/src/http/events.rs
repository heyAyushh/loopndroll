use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive};
use axum::response::{IntoResponse, Response, Sse};

use crate::control_plane::ControlPlane;
use crate::mobile::events::{
    MOBILE_EVENT_CONNECTED_NAME, MobileEvent, MobileEventBroadcast, MobileEventRecord,
    mobile_event_now, mobile_event_sse_name, snapshot_revision_changed_event,
};

use super::mobile_access::authorize_mobile_api_request;
use super::responses::mobile_authorization_error_response;

const MOBILE_EVENT_BACKFILL_LIMIT: usize = 32;
const COMPACTION_TAIL_LIMIT: usize = 50;
const MOBILE_EVENT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const AUTOMATION_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5);

pub(super) async fn desktop_events(
    State(control_plane): State<ControlPlane>,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    local_desktop_events_stream(control_plane)
}

pub(super) async fn mobile_events_handler(
    State(control_plane): State<ControlPlane>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authorize_mobile_api_request(&control_plane, &headers) {
        return mobile_authorization_error_response(error);
    }

    local_desktop_events_stream(control_plane).into_response()
}

pub(super) async fn events_tail(
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
            for compaction in control_plane.compactions().unwrap_or_default().into_iter().take(COMPACTION_TAIL_LIMIT) {
                let payload = serde_json::to_string(&compaction).unwrap_or_else(|_| "{}".to_owned());
                yield Ok(Event::default().event("codex.context_compacted").data(payload));
            }
            tokio::time::sleep(AUTOMATION_SNAPSHOT_INTERVAL).await;
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn local_desktop_events_stream(
    control_plane: ControlPlane,
) -> Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = control_plane.mobile_event_hub().subscribe();
    let mut last_revision = control_plane.mobile_snapshot_revision().unwrap_or_default();
    let mut last_event_cursor = control_plane
        .store()
        .latest_mobile_event_cursor()
        .unwrap_or_default();
    let connected_payload = serde_json::to_string(&serde_json::json!({
        "event_type": MOBILE_EVENT_CONNECTED_NAME,
        "server_time": mobile_event_now(),
        "revision": last_revision,
    }))
    .unwrap_or_else(|_| "{}".to_owned());

    let stream = stream! {
        yield Ok::<Event, Infallible>(Event::default().event("connected").data(connected_payload));
        let mut poll_interval = tokio::time::interval(MOBILE_EVENT_POLL_INTERVAL);
        poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                received = receiver.recv() => {
                    match received {
                        Ok(event) => {
                            match event {
                                MobileEventBroadcast::Persisted(record) => {
                                    last_event_cursor = (&record).into();
                                    let revision = current_snapshot_revision(&control_plane, &mut last_revision);
                                    yield Ok::<Event, Infallible>(mobile_sse_event_from_record(&record, revision));
                                }
                                MobileEventBroadcast::Ephemeral(event) => {
                                    yield Ok::<Event, Infallible>(mobile_sse_event(&event));
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            if let Ok(records) = control_plane.store().mobile_events_after(&last_event_cursor, MOBILE_EVENT_BACKFILL_LIMIT) {
                                for record in records {
                                    last_event_cursor = (&record).into();
                                    let revision = current_snapshot_revision(&control_plane, &mut last_revision);
                                    yield Ok::<Event, Infallible>(mobile_sse_event_from_record(&record, revision));
                                }
                            }
                            if let Ok(revision) = control_plane.mobile_snapshot_revision()
                                && revision != last_revision
                            {
                                last_revision = revision;
                                yield Ok::<Event, Infallible>(mobile_sse_event(&snapshot_revision_changed_event(last_revision.clone())));
                            }
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = poll_interval.tick() => {
                    if let Ok(records) = control_plane.store().mobile_events_after(&last_event_cursor, MOBILE_EVENT_BACKFILL_LIMIT) {
                        for record in records {
                            last_event_cursor = (&record).into();
                            let revision = current_snapshot_revision(&control_plane, &mut last_revision);
                            yield Ok::<Event, Infallible>(mobile_sse_event_from_record(&record, revision));
                        }
                    }

                    if let Ok(revision) = control_plane.mobile_snapshot_revision()
                        && revision != last_revision
                    {
                        last_revision = revision;
                        yield Ok::<Event, Infallible>(mobile_sse_event(&snapshot_revision_changed_event(last_revision.clone())));
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn mobile_sse_event(event: &MobileEvent) -> Event {
    let payload = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_owned());
    Event::default()
        .event(mobile_event_sse_name(event.event_type))
        .data(payload)
}

fn mobile_sse_event_from_record(record: &MobileEventRecord, revision: Option<String>) -> Event {
    mobile_sse_event(&MobileEvent {
        event_type: record.event_type,
        thread_id: record.thread_id.clone(),
        prompt_id: record.prompt_id.clone(),
        detail: record.detail.clone(),
        server_time: mobile_event_now(),
        revision,
    })
}

fn current_snapshot_revision(
    control_plane: &ControlPlane,
    last_revision: &mut String,
) -> Option<String> {
    let revision = control_plane.mobile_snapshot_revision().ok()?;
    *last_revision = revision.clone();
    Some(revision)
}

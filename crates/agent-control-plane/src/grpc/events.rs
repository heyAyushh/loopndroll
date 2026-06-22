use std::pin::Pin;
use std::time::Duration;

use async_stream::stream;
use futures_core::Stream;
use tonic::Status;

use crate::control_plane::ControlPlane;
use crate::grpc::proto;
use crate::mobile::events::{
    MOBILE_EVENT_CONNECTED_NAME, MobileEvent, MobileEventBroadcast, MobileEventKind,
    MobileEventRecord, mobile_event_now, snapshot_revision_changed_event,
};

const EVENT_REPLAY_BATCH_SIZE: usize = 32;
const EVENT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROTO_EVENT_KIND_UNSPECIFIED: i32 = 0;
const PROTO_EVENT_KIND_SESSION_CHANGED: i32 = 1;
const PROTO_EVENT_KIND_PROMPT_QUEUED: i32 = 2;
const PROTO_EVENT_KIND_PROMPT_DELIVERED: i32 = 3;
const PROTO_EVENT_KIND_LIFECYCLE_CHANGED: i32 = 4;

pub type MobileEventStream =
    Pin<Box<dyn Stream<Item = Result<proto::MobileEvent, Status>> + Send + 'static>>;

pub fn mobile_events(control_plane: ControlPlane) -> MobileEventStream {
    let stream = stream! {
        let mut receiver = control_plane.mobile_event_hub().subscribe();
        let mut last_revision = control_plane.mobile_snapshot_revision().unwrap_or_default();
        let mut last_event_cursor = control_plane
            .store()
            .latest_mobile_event_cursor()
            .unwrap_or_default();

        yield Ok(connected_event(&last_revision));

        let mut poll_interval = tokio::time::interval(EVENT_POLL_INTERVAL);
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
                                    yield Ok(proto_event_from_record(&record, revision));
                                }
                                MobileEventBroadcast::Ephemeral(event) => {
                                    yield Ok(proto_event_from_mobile_event(&event));
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                            if let Ok(records) = control_plane.store().mobile_events_after(&last_event_cursor, EVENT_REPLAY_BATCH_SIZE) {
                                for record in records {
                                    last_event_cursor = (&record).into();
                                    let revision = current_snapshot_revision(&control_plane, &mut last_revision);
                                    yield Ok(proto_event_from_record(&record, revision));
                                }
                            }
                            if let Ok(revision) = control_plane.mobile_snapshot_revision()
                                && revision != last_revision
                            {
                                last_revision = revision;
                                yield Ok(proto_event_from_mobile_event(&snapshot_revision_changed_event(last_revision.clone())));
                            }
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                _ = poll_interval.tick() => {
                    if let Ok(records) = control_plane.store().mobile_events_after(&last_event_cursor, EVENT_REPLAY_BATCH_SIZE) {
                        for record in records {
                            last_event_cursor = (&record).into();
                            let revision = current_snapshot_revision(&control_plane, &mut last_revision);
                            yield Ok(proto_event_from_record(&record, revision));
                        }
                    }

                    if let Ok(revision) = control_plane.mobile_snapshot_revision()
                        && revision != last_revision
                    {
                        last_revision = revision;
                        yield Ok(proto_event_from_mobile_event(&snapshot_revision_changed_event(last_revision.clone())));
                    }
                }
            }
        }
    };

    Box::pin(stream)
}

pub fn connected_event(revision: &str) -> proto::MobileEvent {
    proto::MobileEvent {
        kind: PROTO_EVENT_KIND_UNSPECIFIED,
        event_name: MOBILE_EVENT_CONNECTED_NAME.to_owned(),
        thread_id: String::new(),
        prompt_id: String::new(),
        detail: String::new(),
        server_time: mobile_event_now(),
        revision: revision.to_owned(),
    }
}

fn proto_event_from_mobile_event(event: &MobileEvent) -> proto::MobileEvent {
    proto::MobileEvent {
        kind: proto_event_kind(event.event_type),
        event_name: event_name(event.event_type).to_owned(),
        thread_id: event.thread_id.clone().unwrap_or_default(),
        prompt_id: event.prompt_id.clone().unwrap_or_default(),
        detail: event.detail.clone().unwrap_or_default(),
        server_time: event.server_time.clone(),
        revision: event.revision.clone().unwrap_or_default(),
    }
}

fn proto_event_from_record(
    record: &MobileEventRecord,
    revision: Option<String>,
) -> proto::MobileEvent {
    proto::MobileEvent {
        kind: proto_event_kind(record.event_type),
        event_name: event_name(record.event_type).to_owned(),
        thread_id: record.thread_id.clone().unwrap_or_default(),
        prompt_id: record.prompt_id.clone().unwrap_or_default(),
        detail: record.detail.clone().unwrap_or_default(),
        server_time: mobile_event_now(),
        revision: revision.unwrap_or_default(),
    }
}

fn current_snapshot_revision(
    control_plane: &ControlPlane,
    last_revision: &mut String,
) -> Option<String> {
    let revision = control_plane.mobile_snapshot_revision().ok()?;
    *last_revision = revision.clone();
    Some(revision)
}

fn proto_event_kind(kind: MobileEventKind) -> i32 {
    match kind {
        MobileEventKind::SessionChanged => PROTO_EVENT_KIND_SESSION_CHANGED,
        MobileEventKind::PromptQueued => PROTO_EVENT_KIND_PROMPT_QUEUED,
        MobileEventKind::PromptDelivered => PROTO_EVENT_KIND_PROMPT_DELIVERED,
        MobileEventKind::LifecycleChanged => PROTO_EVENT_KIND_LIFECYCLE_CHANGED,
    }
}

fn event_name(kind: MobileEventKind) -> &'static str {
    match kind {
        MobileEventKind::SessionChanged => "session.changed",
        MobileEventKind::PromptQueued => "prompt.queued",
        MobileEventKind::PromptDelivered => "prompt.delivered",
        MobileEventKind::LifecycleChanged => "lifecycle.changed",
    }
}

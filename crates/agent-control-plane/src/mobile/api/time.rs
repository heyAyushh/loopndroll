use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::control_plane::DesktopThread;

const NANOS_PER_MILLISECOND: i128 = 1_000_000;

pub(super) fn thread_activity_timestamp(thread: &DesktopThread) -> String {
    latest_thread_activity_millis(thread)
        .and_then(timestamp_millis_to_iso)
        .unwrap_or_else(current_iso_time)
}

pub(super) fn thread_message_timestamp(thread: &DesktopThread) -> Option<String> {
    thread
        .latest_message_at_ms
        .and_then(timestamp_millis_to_iso)
}

pub(super) fn latest_thread_activity_millis(thread: &DesktopThread) -> Option<i64> {
    [
        thread.updated_at_ms,
        thread.latest_message_at_ms,
        thread.created_at_ms,
    ]
    .into_iter()
    .flatten()
    .max()
}

fn timestamp_millis_to_iso(timestamp_millis: i64) -> Option<String> {
    let timestamp_nanos = i128::from(timestamp_millis).checked_mul(NANOS_PER_MILLISECOND)?;
    OffsetDateTime::from_unix_timestamp_nanos(timestamp_nanos)
        .ok()?
        .format(&Rfc3339)
        .ok()
}

fn current_iso_time() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::extract::State;
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};

use crate::control_plane::ControlPlane;

const COMPACTION_TAIL_LIMIT: usize = 50;
const AUTOMATION_SNAPSHOT_INTERVAL: Duration = Duration::from_secs(5);

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

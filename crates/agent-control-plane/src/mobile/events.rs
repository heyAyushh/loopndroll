use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use tokio::sync::broadcast;

const MOBILE_EVENT_CHANNEL_CAPACITY: usize = 256;
pub const MOBILE_EVENT_CONNECTED_NAME: &str = "connected";
pub const SNAPSHOT_REVISION_CHANGED_DETAIL: &str = "snapshot-revision-changed";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MobileEventKind {
    SessionChanged,
    PromptQueued,
    PromptDelivered,
    LifecycleChanged,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileEvent {
    pub event_type: MobileEventKind,
    pub thread_id: Option<String>,
    pub prompt_id: Option<String>,
    pub detail: Option<String>,
    pub server_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MobileEventInput {
    pub kind: MobileEventKind,
    pub thread_id: Option<String>,
    pub prompt_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MobileEventRecord {
    pub event_id: String,
    pub event_type: MobileEventKind,
    pub thread_id: Option<String>,
    pub prompt_id: Option<String>,
    pub detail: Option<String>,
    pub created_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MobileEventBroadcast {
    Persisted(MobileEventRecord),
    Ephemeral(MobileEvent),
}

#[derive(Clone)]
pub struct MobileEventHub {
    sender: broadcast::Sender<MobileEventBroadcast>,
}

impl MobileEventHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(MOBILE_EVENT_CHANNEL_CAPACITY);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<MobileEventBroadcast> {
        self.sender.subscribe()
    }

    pub fn publish_persisted(&self, record: MobileEventRecord) {
        let _ = self.sender.send(MobileEventBroadcast::Persisted(record));
    }

    pub fn publish_ephemeral(&self, event: MobileEvent) {
        let _ = self.sender.send(MobileEventBroadcast::Ephemeral(event));
    }
}

impl Default for MobileEventHub {
    fn default() -> Self {
        Self::new()
    }
}

pub fn mobile_event_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

pub fn build_mobile_event(input: MobileEventInput) -> MobileEvent {
    MobileEvent {
        event_type: input.kind,
        thread_id: input.thread_id,
        prompt_id: input.prompt_id,
        detail: input.detail,
        server_time: mobile_event_now(),
        revision: None,
    }
}

pub fn snapshot_revision_changed_event(revision: String) -> MobileEvent {
    MobileEvent {
        event_type: MobileEventKind::SessionChanged,
        thread_id: None,
        prompt_id: None,
        detail: Some(SNAPSHOT_REVISION_CHANGED_DETAIL.to_owned()),
        server_time: mobile_event_now(),
        revision: Some(revision),
    }
}

pub fn mobile_event_wire_name(kind: MobileEventKind) -> &'static str {
    match kind {
        MobileEventKind::SessionChanged => "session.changed",
        MobileEventKind::PromptQueued => "prompt.queued",
        MobileEventKind::PromptDelivered => "prompt.delivered",
        MobileEventKind::LifecycleChanged => "lifecycle.changed",
    }
}

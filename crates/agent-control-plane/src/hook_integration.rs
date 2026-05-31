use serde::{Deserialize, Serialize};

use crate::compaction::LOCAL_COMPACTION_EVENT_TYPE;

const HOOK_BRIDGE_VERSION: &str = "2026-04-25";
const DEFAULT_INGRESS_PATH: &str = "/webhook/agent-control-plane";
const DEFAULT_EVENT_TOPIC: &str = "agent_control_plane.events";
const DEFAULT_DLQ_TOPIC: &str = "agent_control_plane.dlq";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookBridgeContract {
    pub version: String,
    pub profile: String,
    pub ingress: HookIngressContract,
    pub topics: HookTopicContract,
    pub events: Vec<HookEventContract>,
    pub payload_policy: HookPayloadPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookIngressContract {
    pub path: String,
    pub auth: String,
    pub signature_header: String,
    pub timestamp_header: String,
    pub delivery_header: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookTopicContract {
    pub events: String,
    pub dead_letter: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookEventContract {
    pub event_type: String,
    pub source: String,
    pub privacy: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HookPayloadPolicy {
    pub raw_transcripts: bool,
    pub raw_prompts: bool,
    pub raw_credentials: bool,
    pub redaction_required: bool,
    pub opaque_session_handles: bool,
}

impl HookBridgeContract {
    pub fn default_local_relay() -> Self {
        Self {
            version: HOOK_BRIDGE_VERSION.to_owned(),
            profile: "agent-control-plane-local-relay".to_owned(),
            ingress: HookIngressContract {
                path: DEFAULT_INGRESS_PATH.to_owned(),
                auth: "hmac-sha256".to_owned(),
                signature_header: "X-Agent-Control-Plane-Signature-256".to_owned(),
                timestamp_header: "X-Agent-Control-Plane-Timestamp".to_owned(),
                delivery_header: "X-Agent-Control-Plane-Delivery".to_owned(),
            },
            topics: HookTopicContract {
                events: DEFAULT_EVENT_TOPIC.to_owned(),
                dead_letter: DEFAULT_DLQ_TOPIC.to_owned(),
            },
            events: vec![
                event("control_plane.health_changed", "host-agent", "summary-only"),
                event("automation.mirrored", "host-agent", "summary-only"),
                event("automation.fired", "host-agent", "summary-only"),
                event(
                    "automation.delivery_succeeded",
                    "host-agent",
                    "summary-only",
                ),
                event("automation.delivery_failed", "host-agent", "summary-only"),
                event("automation.reconnected", "host-agent", "summary-only"),
                event(
                    LOCAL_COMPACTION_EVENT_TYPE,
                    "codex-rollout",
                    "metadata-only",
                ),
                event("hook.failure_detected", "host-agent", "redacted-diagnostic"),
                event(
                    "relay.command_received",
                    "cloud-relay",
                    "encrypted-envelope",
                ),
                event("relay.command_acknowledged", "host-agent", "receipt-only"),
            ],
            payload_policy: HookPayloadPolicy {
                raw_transcripts: false,
                raw_prompts: false,
                raw_credentials: false,
                redaction_required: true,
                opaque_session_handles: true,
            },
        }
    }
}

pub fn hook_bridge_contract_toml(
    contract: &HookBridgeContract,
) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(contract)
}

fn event(event_type: &str, source: &str, privacy: &str) -> HookEventContract {
    HookEventContract {
        event_type: event_type.to_owned(),
        source: source.to_owned(),
        privacy: privacy.to_owned(),
    }
}

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::events::{MobileSessionMiniRecord, MobileStateEventRecord};
use crate::mobile::events::MobileEventKind;

use super::session_fsm::{SessionCommand, SessionMode, SessionState, accepted_prompt_state, next};

const COMMAND_KIND_SET_SESSION_MODE: &str = "SetSessionMode";
const COMMAND_KIND_SEND_SESSION_PROMPT: &str = "SendSessionPrompt";
const COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY: &str = "SubmitNotificationReply";
const DETAIL_MODE_CLEARED: &str = "mode-cleared";
const DETAIL_PROMPT_RESUMED: &str = "prompt-resumed";
const DETAIL_SESSION_START: &str = "SessionStart";
const DETAIL_USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
const DETAIL_STOP: &str = "Stop";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReducedSessionState {
    pub latest_seq: i64,
    pub sessions: BTreeMap<String, SessionState>,
}

impl ReducedSessionState {
    pub fn state_for_thread(&self, thread_id: &str) -> Option<&SessionState> {
        self.sessions.get(thread_id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SessionEvent {
    ModeSet {
        thread_id: String,
        mode: Option<SessionMode>,
    },
    PromptAccepted {
        thread_id: String,
        client_mutation_id: String,
    },
    AgentStarted {
        thread_id: String,
    },
    AgentStopped {
        thread_id: String,
    },
    PromptDelivered {
        thread_id: String,
    },
}

pub fn fold_mobile_state_events<'a>(
    events: impl IntoIterator<Item = &'a MobileStateEventRecord>,
) -> ReducedSessionState {
    let mut state = ReducedSessionState::default();
    for record in events {
        state.latest_seq = state.latest_seq.max(record.seq);
        let Some(event) = session_event_from_record(record) else {
            continue;
        };
        fold_session_event(&mut state.sessions, event);
    }
    state
}

pub fn session_state_for_thread(
    events: &[MobileStateEventRecord],
    minis: &[MobileSessionMiniRecord],
    thread_id: &str,
    assistant_surface: Option<&str>,
) -> SessionState {
    let reduced = fold_mobile_state_events(events);
    if let Some(projected_state) =
        projected_session_state_from_minis(minis, thread_id, assistant_surface)
    {
        return projected_state;
    }
    if let Some(state) = reduced.state_for_thread(thread_id) {
        return state.clone();
    }
    SessionState::default()
}

pub fn projected_session_state_from_minis(
    records: &[MobileSessionMiniRecord],
    session_id: &str,
    assistant_surface: Option<&str>,
) -> Option<SessionState> {
    let record = records.iter().find(|record| {
        record.session_id == session_id
            && assistant_surface
                .map(|surface| record.assistant_surface == surface)
                .unwrap_or(true)
    })?;
    let body = serde_json::from_str::<Value>(&record.body_json).ok()?;
    let effective_mode = body.get("effectiveMode").and_then(Value::as_str);
    let lifecycle = body
        .get("lifecycle")
        .and_then(Value::as_str)
        .or_else(|| body.get("status").and_then(Value::as_str));
    Some(SessionState::from_projection(
        effective_mode,
        lifecycle,
        None,
    ))
}

fn fold_session_event(sessions: &mut BTreeMap<String, SessionState>, event: SessionEvent) {
    let thread_id = event.thread_id().to_owned();
    let previous = sessions.remove(&thread_id).unwrap_or_default();
    let next_state = match event {
        SessionEvent::ModeSet { mode, .. } => {
            next(previous.clone(), SessionCommand::SetMode { mode }).unwrap_or(previous)
        }
        SessionEvent::PromptAccepted {
            client_mutation_id, ..
        } => {
            let pending = next(
                previous.clone(),
                SessionCommand::SendPrompt {
                    client_mutation_id: client_mutation_id.clone(),
                },
            )
            .unwrap_or(previous);
            accepted_prompt_state(pending, client_mutation_id)
        }
        SessionEvent::AgentStarted { .. } => next(previous.clone(), SessionCommand::AgentStarted)
            .unwrap_or_else(|_| inferred_active_state(previous)),
        SessionEvent::AgentStopped { .. } => {
            next(previous.clone(), SessionCommand::AgentStopped).unwrap_or(previous)
        }
        SessionEvent::PromptDelivered { .. } => {
            next(previous.clone(), SessionCommand::PromptDelivered).unwrap_or(previous)
        }
    };
    sessions.insert(thread_id, next_state);
}

fn inferred_active_state(previous: SessionState) -> SessionState {
    previous
        .mode()
        .cloned()
        .map(|mode| SessionState::AgentRunning { mode })
        .unwrap_or(SessionState::AgentRunning {
            mode: SessionMode::Infinite,
        })
}

impl SessionEvent {
    fn thread_id(&self) -> &str {
        match self {
            Self::ModeSet { thread_id, .. }
            | Self::PromptAccepted { thread_id, .. }
            | Self::AgentStarted { thread_id }
            | Self::AgentStopped { thread_id }
            | Self::PromptDelivered { thread_id } => thread_id,
        }
    }
}

fn session_event_from_record(record: &MobileStateEventRecord) -> Option<SessionEvent> {
    if let Some(command_kind) = record.command_kind.as_deref() {
        return session_event_from_command_record(record, command_kind);
    }

    let payload = serde_json::from_str::<Value>(&record.payload_json).ok()?;
    let thread_id = payload
        .get("threadId")
        .and_then(Value::as_str)
        .filter(|thread_id| !thread_id.is_empty())
        .map(str::to_owned)
        .or_else(|| Some(record.entity_id.clone()))?;
    match record.kind {
        MobileEventKind::LifecycleChanged => {
            lifecycle_event_from_detail(thread_id, payload.get("detail").and_then(Value::as_str))
        }
        MobileEventKind::PromptQueued | MobileEventKind::PromptDelivered => {
            Some(SessionEvent::PromptDelivered { thread_id })
        }
        MobileEventKind::SessionChanged => session_event_from_session_detail(
            thread_id,
            payload.get("detail").and_then(Value::as_str),
        ),
    }
}

fn session_event_from_command_record(
    record: &MobileStateEventRecord,
    command_kind: &str,
) -> Option<SessionEvent> {
    let response = record
        .command_response_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<Value>(json).ok())?;
    let thread_id = response
        .get("threadId")
        .or_else(|| response.get("entityId"))
        .and_then(Value::as_str)
        .filter(|thread_id| !thread_id.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| record.entity_id.clone());
    match command_kind {
        COMMAND_KIND_SET_SESSION_MODE => Some(SessionEvent::ModeSet {
            thread_id,
            mode: response
                .get("preset")
                .and_then(Value::as_str)
                .filter(|preset| !preset.is_empty())
                .map(SessionMode::parse)
                .transpose()
                .ok()
                .flatten(),
        }),
        COMMAND_KIND_SEND_SESSION_PROMPT | COMMAND_KIND_SUBMIT_NOTIFICATION_REPLY => {
            Some(SessionEvent::PromptAccepted {
                thread_id,
                client_mutation_id: record.client_mutation_id.clone().unwrap_or_default(),
            })
        }
        _ => None,
    }
}

fn lifecycle_event_from_detail(thread_id: String, detail: Option<&str>) -> Option<SessionEvent> {
    match detail {
        Some(DETAIL_MODE_CLEARED) | None => Some(SessionEvent::ModeSet {
            thread_id,
            mode: None,
        }),
        Some(detail) => SessionMode::parse(detail)
            .ok()
            .map(|mode| SessionEvent::ModeSet {
                thread_id,
                mode: Some(mode),
            }),
    }
}

fn session_event_from_session_detail(
    thread_id: String,
    detail: Option<&str>,
) -> Option<SessionEvent> {
    match detail {
        Some(DETAIL_SESSION_START | DETAIL_USER_PROMPT_SUBMIT) => {
            Some(SessionEvent::AgentStarted { thread_id })
        }
        Some(DETAIL_STOP) => Some(SessionEvent::AgentStopped { thread_id }),
        Some(DETAIL_PROMPT_RESUMED) => Some(SessionEvent::PromptDelivered { thread_id }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::events::MobileStateEventRecord;
    use crate::mobile::events::MobileEventKind;

    use super::*;

    #[test]
    fn reducer_folds_command_log_into_session_state() {
        let events = vec![
            state_event(
                1,
                "thread-1",
                MobileEventKind::SessionChanged,
                Some(COMMAND_KIND_SET_SESSION_MODE),
                Some("cmid-mode"),
                serde_json::json!({
                    "threadId": "thread-1",
                    "preset": "await-reply",
                    "entityId": "thread-1",
                    "revision": "rev-1",
                    "serverTime": "now",
                }),
            ),
            state_event(
                2,
                "thread-1",
                MobileEventKind::SessionChanged,
                Some(COMMAND_KIND_SEND_SESSION_PROMPT),
                Some("cmid-prompt"),
                serde_json::json!({
                    "dispatchKind": "accepted",
                    "entityId": "thread-1",
                    "revision": "rev-2",
                    "serverTime": "now",
                }),
            ),
        ];

        let reduced = fold_mobile_state_events(&events);

        assert_eq!(reduced.latest_seq, 2);
        assert_eq!(
            reduced.sessions["thread-1"],
            SessionState::Dispatched {
                mode: SessionMode::AwaitReply,
                client_mutation_id: Some("cmid-prompt".to_owned())
            }
        );
    }

    #[test]
    fn projected_minis_seed_initial_mode_state() {
        let minis = vec![MobileSessionMiniRecord {
            session_id: "thread-1".to_owned(),
            assistant_surface: "codex".to_owned(),
            seq: 7,
            revision: "rev-7".to_owned(),
            body_json: serde_json::json!({
                "sessionId": "thread-1",
                "assistantSurface": "codex",
                "effectiveMode": "await-reply",
                "lifecycle": "waiting",
            })
            .to_string(),
            updated_at_ms: 1,
        }];

        assert_eq!(
            projected_session_state_from_minis(&minis, "thread-1", Some("codex")),
            Some(SessionState::WaitReply {
                mode: SessionMode::AwaitReply
            })
        );
    }

    #[test]
    fn active_projected_minis_seed_infinite_running_state_without_mode() {
        let minis = vec![MobileSessionMiniRecord {
            session_id: "thread-1".to_owned(),
            assistant_surface: "codex".to_owned(),
            seq: 7,
            revision: "rev-7".to_owned(),
            body_json: serde_json::json!({
                "sessionId": "thread-1",
                "assistantSurface": "codex",
                "status": "active",
            })
            .to_string(),
            updated_at_ms: 1,
        }];

        assert_eq!(
            projected_session_state_from_minis(&minis, "thread-1", Some("codex")),
            Some(SessionState::AgentRunning {
                mode: SessionMode::Infinite
            })
        );
    }

    #[test]
    fn projected_minis_are_current_truth_over_folded_events() {
        let events = vec![state_event(
            1,
            "thread-1",
            MobileEventKind::SessionChanged,
            Some(COMMAND_KIND_SET_SESSION_MODE),
            Some("cmid-mode"),
            serde_json::json!({
                "threadId": "thread-1",
                "preset": "await-reply",
                "entityId": "thread-1",
                "revision": "rev-1",
                "serverTime": "now",
            }),
        )];
        let minis = vec![MobileSessionMiniRecord {
            session_id: "thread-1".to_owned(),
            assistant_surface: "codex".to_owned(),
            seq: 2,
            revision: "rev-2".to_owned(),
            body_json: serde_json::json!({
                "sessionId": "thread-1",
                "assistantSurface": "codex",
                "effectiveMode": "max-turns-1",
                "lifecycle": "idle",
            })
            .to_string(),
            updated_at_ms: 2,
        }];

        assert_eq!(
            session_state_for_thread(&events, &minis, "thread-1", Some("codex")),
            SessionState::ModeArmed {
                mode: SessionMode::MaxTurns1
            }
        );
    }

    fn state_event(
        seq: i64,
        entity_id: &str,
        kind: MobileEventKind,
        command_kind: Option<&str>,
        client_mutation_id: Option<&str>,
        response_json: serde_json::Value,
    ) -> MobileStateEventRecord {
        MobileStateEventRecord {
            seq,
            entity_id: entity_id.to_owned(),
            kind,
            revision: format!("rev-{seq}"),
            server_time: "now".to_owned(),
            payload_json: serde_json::json!({
                "threadId": entity_id,
                "detail": "command-ack",
            })
            .to_string(),
            client_mutation_id: client_mutation_id.map(str::to_owned),
            command_kind: command_kind.map(str::to_owned),
            command_request_hash: None,
            command_response_json: command_kind.map(|_| response_json.to_string()),
            created_at_ms: seq,
        }
    }
}

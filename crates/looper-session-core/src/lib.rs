use std::fmt;

use serde::{Deserialize, Serialize};

const MODE_INFINITE: &str = "infinite";
const MODE_AWAIT_REPLY: &str = "await-reply";
const MODE_COMPLETION_CHECKS: &str = "completion-checks";
const MODE_MAX_TURNS_1: &str = "max-turns-1";
const MODE_MAX_TURNS_2: &str = "max-turns-2";
const MODE_MAX_TURNS_3: &str = "max-turns-3";
const ARCHIVED_STATUS: &str = "archived";
const WAITING_STATUS: &str = "waiting";
const AGENT_START_HOOK: &str = "SessionStart";
const USER_PROMPT_SUBMIT_HOOK: &str = "UserPromptSubmit";
const STOP_HOOK: &str = "Stop";

pub const ACTIVE_STATUS: &str = "active";
pub const STOPPED_STATUS: &str = "stopped";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionMode {
    Infinite,
    AwaitReply,
    CompletionChecks,
    MaxTurns1,
    MaxTurns2,
    MaxTurns3,
}

impl SessionMode {
    pub fn parse(preset: &str) -> Result<Self, SessionReject> {
        match preset.trim() {
            MODE_INFINITE => Ok(Self::Infinite),
            MODE_AWAIT_REPLY => Ok(Self::AwaitReply),
            MODE_COMPLETION_CHECKS => Ok(Self::CompletionChecks),
            MODE_MAX_TURNS_1 => Ok(Self::MaxTurns1),
            MODE_MAX_TURNS_2 => Ok(Self::MaxTurns2),
            MODE_MAX_TURNS_3 => Ok(Self::MaxTurns3),
            _ => Err(SessionReject::invalid_mode()),
        }
    }

    pub fn parse_optional(preset: Option<&str>) -> Result<Option<Self>, SessionReject> {
        preset
            .map(Self::parse)
            .transpose()
            .map_err(|reject| reject.with_current_state(SessionState::Idle))
    }

    pub fn as_preset(&self) -> &'static str {
        match self {
            Self::Infinite => MODE_INFINITE,
            Self::AwaitReply => MODE_AWAIT_REPLY,
            Self::CompletionChecks => MODE_COMPLETION_CHECKS,
            Self::MaxTurns1 => MODE_MAX_TURNS_1,
            Self::MaxTurns2 => MODE_MAX_TURNS_2,
            Self::MaxTurns3 => MODE_MAX_TURNS_3,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum SessionState {
    Idle,
    ModeArmed {
        mode: SessionMode,
    },
    PromptPending {
        mode: SessionMode,
        client_mutation_id: String,
    },
    Dispatched {
        mode: SessionMode,
        client_mutation_id: Option<String>,
    },
    AgentRunning {
        mode: SessionMode,
    },
    StopRequested {
        mode: Option<SessionMode>,
    },
    ContinuationPending {
        mode: SessionMode,
    },
    ChecksRunning {
        mode: SessionMode,
    },
    WaitReply {
        mode: SessionMode,
    },
    Done,
}

impl Default for SessionState {
    fn default() -> Self {
        Self::Idle
    }
}

impl SessionState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::ModeArmed { .. } => "mode_armed",
            Self::PromptPending { .. } => "prompt_pending",
            Self::Dispatched { .. } => "dispatched",
            Self::AgentRunning { .. } => "agent_running",
            Self::StopRequested { .. } => "stop_requested",
            Self::ContinuationPending { .. } => "continuation_pending",
            Self::ChecksRunning { .. } => "checks_running",
            Self::WaitReply { .. } => "wait_reply",
            Self::Done => "done",
        }
    }

    pub fn mode(&self) -> Option<&SessionMode> {
        match self {
            Self::ModeArmed { mode }
            | Self::PromptPending { mode, .. }
            | Self::Dispatched { mode, .. }
            | Self::AgentRunning { mode }
            | Self::ContinuationPending { mode }
            | Self::ChecksRunning { mode }
            | Self::WaitReply { mode } => Some(mode),
            Self::StopRequested { mode } => mode.as_ref(),
            Self::Idle | Self::Done => None,
        }
    }

    pub fn display_status(&self) -> &'static str {
        match self {
            Self::AgentRunning { .. } | Self::Dispatched { .. } | Self::PromptPending { .. } => {
                ACTIVE_STATUS
            }
            Self::WaitReply { .. } => WAITING_STATUS,
            Self::Idle
            | Self::ModeArmed { .. }
            | Self::StopRequested { .. }
            | Self::ContinuationPending { .. }
            | Self::ChecksRunning { .. }
            | Self::Done => STOPPED_STATUS,
        }
    }

    pub fn from_projection(
        effective_mode: Option<&str>,
        lifecycle_status: Option<&str>,
        runtime_status: Option<&str>,
    ) -> Self {
        let mode = effective_mode.and_then(|preset| SessionMode::parse(preset).ok());
        match (mode, lifecycle_status, runtime_status) {
            (None, Some(ACTIVE_STATUS), _) | (None, _, Some(ACTIVE_STATUS)) => Self::AgentRunning {
                mode: SessionMode::Infinite,
            },
            (None, _, _) => Self::Idle,
            (Some(mode), Some(ACTIVE_STATUS), _) | (Some(mode), _, Some(ACTIVE_STATUS)) => {
                Self::AgentRunning { mode }
            }
            (Some(mode @ SessionMode::AwaitReply), Some(WAITING_STATUS), _)
            | (Some(mode @ SessionMode::AwaitReply), Some(STOPPED_STATUS), _) => {
                Self::WaitReply { mode }
            }
            (Some(mode @ SessionMode::AwaitReply), _, _) => Self::WaitReply { mode },
            (Some(mode), _, _) => Self::ModeArmed { mode },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "command")]
pub enum SessionCommand {
    SetMode { mode: Option<SessionMode> },
    SendPrompt { client_mutation_id: String },
    SteerPrompt { client_mutation_id: String },
    SubmitNotificationReply { client_mutation_id: String },
    AgentStarted,
    AgentStopped,
    PromptDelivered,
    PromptDeliveryFailed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionRejectCode {
    InvalidMode,
    ModeRequired,
    SessionBusy,
    IllegalTransition,
}

impl SessionRejectCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidMode => "invalid_mode",
            Self::ModeRequired => "mode_required",
            Self::SessionBusy => "session_busy",
            Self::IllegalTransition => "illegal_transition",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionReject {
    pub code: SessionRejectCode,
    pub reason: String,
    pub current_state: SessionState,
}

impl SessionReject {
    pub fn invalid_mode() -> Self {
        Self {
            code: SessionRejectCode::InvalidMode,
            reason: "session mode must be a known mode or empty".to_owned(),
            current_state: SessionState::Idle,
        }
    }

    pub fn code_str(&self) -> &'static str {
        self.code.as_str()
    }

    pub fn with_current_state(mut self, current_state: SessionState) -> Self {
        self.current_state = current_state;
        self
    }

    pub fn wire_reason(&self) -> String {
        format!(
            "{} (current_state={})",
            self.reason,
            self.current_state.label()
        )
    }

    pub fn status_message(&self) -> String {
        format!(
            "session_fsm_reject code={} current_state={} reason={}",
            self.code_str(),
            self.current_state.label(),
            self.reason
        )
    }

    pub fn from_status_message(message: &str) -> Option<Self> {
        let remainder = message.strip_prefix("session_fsm_reject ")?;
        let mut code = None;
        let mut current_state = None;
        let mut reason = None;
        for field in remainder.split(' ') {
            if let Some(value) = field.strip_prefix("code=") {
                code = Some(value);
            } else if let Some(value) = field.strip_prefix("current_state=") {
                current_state = Some(value);
            } else if let Some(value) = remainder.split(" reason=").nth(1) {
                reason = Some(value);
                break;
            }
        }
        let code = match code? {
            "invalid_mode" => SessionRejectCode::InvalidMode,
            "mode_required" => SessionRejectCode::ModeRequired,
            "session_busy" => SessionRejectCode::SessionBusy,
            "illegal_transition" => SessionRejectCode::IllegalTransition,
            _ => return None,
        };
        Some(Self {
            code,
            reason: reason.unwrap_or_default().to_owned(),
            current_state: state_from_label(current_state.unwrap_or_default()),
        })
    }
}

impl fmt::Display for SessionReject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.wire_reason())
    }
}

impl std::error::Error for SessionReject {}

pub fn next(state: SessionState, command: SessionCommand) -> Result<SessionState, SessionReject> {
    match command {
        SessionCommand::SetMode { mode } => Ok(match mode {
            Some(mode) => SessionState::ModeArmed { mode },
            None => SessionState::Idle,
        }),
        SessionCommand::SendPrompt { client_mutation_id }
        | SessionCommand::SubmitNotificationReply { client_mutation_id } => {
            next_prompt_state(state, client_mutation_id)
        }
        SessionCommand::SteerPrompt { client_mutation_id } => {
            next_steer_prompt_state(state, client_mutation_id)
        }
        SessionCommand::AgentStarted => state
            .mode()
            .cloned()
            .map(|mode| SessionState::AgentRunning { mode })
            .ok_or_else(|| reject_mode_required(state)),
        SessionCommand::AgentStopped => Ok(next_stopped_state(state)),
        SessionCommand::PromptDelivered => state
            .mode()
            .cloned()
            .map(|mode| SessionState::AgentRunning { mode })
            .ok_or_else(|| reject_mode_required(state)),
        SessionCommand::PromptDeliveryFailed => Ok(next_prompt_delivery_failed_state(state)),
    }
}

pub fn accepted_prompt_state(state: SessionState, client_mutation_id: String) -> SessionState {
    let mode = match state {
        SessionState::PromptPending { mode, .. }
        | SessionState::ModeArmed { mode }
        | SessionState::Dispatched { mode, .. }
        | SessionState::AgentRunning { mode }
        | SessionState::ContinuationPending { mode }
        | SessionState::ChecksRunning { mode }
        | SessionState::WaitReply { mode } => mode,
        SessionState::StopRequested { mode } => {
            return mode
                .map(|mode| SessionState::Dispatched {
                    mode,
                    client_mutation_id: Some(client_mutation_id),
                })
                .unwrap_or(SessionState::Done);
        }
        SessionState::Idle | SessionState::Done => return SessionState::Done,
    };
    SessionState::Dispatched {
        mode,
        client_mutation_id: Some(client_mutation_id),
    }
}

pub fn projected_status(
    is_archived: bool,
    effective_mode: Option<&str>,
    lifecycle_status: Option<&str>,
    runtime_status: Option<&str>,
) -> &'static str {
    if is_archived {
        return ARCHIVED_STATUS;
    }
    match lifecycle_status {
        Some(ACTIVE_STATUS) => return ACTIVE_STATUS,
        Some(STOPPED_STATUS) => return inactive_projected_status(effective_mode),
        _ => {}
    }
    if runtime_status == Some(ACTIVE_STATUS) {
        return ACTIVE_STATUS;
    }
    inactive_projected_status(effective_mode)
}

pub fn hook_lifecycle_state(hook_event_name: &str, did_continue: bool) -> Option<SessionState> {
    let mode = SessionMode::Infinite;
    match hook_event_name {
        AGENT_START_HOOK | USER_PROMPT_SUBMIT_HOOK => Some(SessionState::AgentRunning { mode }),
        STOP_HOOK if did_continue => Some(SessionState::ContinuationPending { mode }),
        STOP_HOOK => Some(SessionState::Done),
        _ => None,
    }
}

pub fn hook_lifecycle_status(hook_event_name: &str, did_continue: bool) -> Option<&'static str> {
    match hook_lifecycle_state(hook_event_name, did_continue)? {
        SessionState::AgentRunning { .. } | SessionState::ContinuationPending { .. } => {
            Some(ACTIVE_STATUS)
        }
        _ => Some(STOPPED_STATUS),
    }
}

fn next_prompt_state(
    state: SessionState,
    client_mutation_id: String,
) -> Result<SessionState, SessionReject> {
    match state {
        SessionState::ModeArmed { mode } => Ok(SessionState::PromptPending {
            mode,
            client_mutation_id,
        }),
        SessionState::WaitReply { mode } | SessionState::ContinuationPending { mode } => {
            Ok(SessionState::Dispatched {
                mode,
                client_mutation_id: Some(client_mutation_id),
            })
        }
        SessionState::AgentRunning { mode } => Ok(SessionState::ContinuationPending { mode }),
        SessionState::Idle | SessionState::Done => Err(reject_mode_required(state)),
        SessionState::PromptPending { .. }
        | SessionState::Dispatched { .. }
        | SessionState::StopRequested { .. }
        | SessionState::ChecksRunning { .. } => Err(SessionReject {
            code: SessionRejectCode::SessionBusy,
            reason: "session cannot receive another prompt until the current transition settles"
                .to_owned(),
            current_state: state,
        }),
    }
}

fn next_steer_prompt_state(
    state: SessionState,
    client_mutation_id: String,
) -> Result<SessionState, SessionReject> {
    match state {
        SessionState::AgentRunning { mode }
        | SessionState::WaitReply { mode }
        | SessionState::ContinuationPending { mode } => Ok(SessionState::Dispatched {
            mode,
            client_mutation_id: Some(client_mutation_id),
        }),
        // Arm-then-prompt is the advertised mobile flow; on a stopped session there is
        // nothing to steer yet, so a steer prompt degrades to the queue behavior
        // instead of rejecting the user's message.
        SessionState::ModeArmed { mode } => Ok(SessionState::PromptPending {
            mode,
            client_mutation_id,
        }),
        SessionState::Idle | SessionState::Done => Err(reject_mode_required(state)),
        SessionState::PromptPending { .. }
        | SessionState::Dispatched { .. }
        | SessionState::StopRequested { .. }
        | SessionState::ChecksRunning { .. } => Err(SessionReject {
            code: SessionRejectCode::SessionBusy,
            reason: "session cannot receive another prompt until the current transition settles"
                .to_owned(),
            current_state: state,
        }),
    }
}

fn next_stopped_state(state: SessionState) -> SessionState {
    let mode = state.mode().cloned();
    match mode {
        Some(SessionMode::Infinite)
        | Some(SessionMode::MaxTurns1)
        | Some(SessionMode::MaxTurns2)
        | Some(SessionMode::MaxTurns3) => SessionState::ContinuationPending {
            mode: mode.expect("mode matched above"),
        },
        Some(mode @ SessionMode::CompletionChecks) => SessionState::ChecksRunning { mode },
        Some(mode @ SessionMode::AwaitReply) => SessionState::WaitReply { mode },
        None => SessionState::Done,
    }
}

fn next_prompt_delivery_failed_state(state: SessionState) -> SessionState {
    match state {
        SessionState::PromptPending { mode, .. } | SessionState::Dispatched { mode, .. } => {
            SessionState::ModeArmed { mode }
        }
        state => state,
    }
}

fn reject_mode_required(current_state: SessionState) -> SessionReject {
    SessionReject {
        code: SessionRejectCode::ModeRequired,
        reason: "set a session mode before sending a prompt".to_owned(),
        current_state,
    }
}

fn inactive_projected_status(effective_mode: Option<&str>) -> &'static str {
    match effective_mode {
        Some(MODE_AWAIT_REPLY) => WAITING_STATUS,
        _ => STOPPED_STATUS,
    }
}

fn state_from_label(label: &str) -> SessionState {
    match label {
        "mode_armed" => SessionState::ModeArmed {
            mode: SessionMode::Infinite,
        },
        "prompt_pending" => SessionState::PromptPending {
            mode: SessionMode::Infinite,
            client_mutation_id: String::new(),
        },
        "dispatched" => SessionState::Dispatched {
            mode: SessionMode::Infinite,
            client_mutation_id: None,
        },
        "agent_running" => SessionState::AgentRunning {
            mode: SessionMode::Infinite,
        },
        "stop_requested" => SessionState::StopRequested {
            mode: Some(SessionMode::Infinite),
        },
        "continuation_pending" => SessionState::ContinuationPending {
            mode: SessionMode::Infinite,
        },
        "checks_running" => SessionState::ChecksRunning {
            mode: SessionMode::CompletionChecks,
        },
        "wait_reply" => SessionState::WaitReply {
            mode: SessionMode::AwaitReply,
        },
        "done" => SessionState::Done,
        _ => SessionState::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_without_mode_returns_typed_reject() {
        let reject = next(
            SessionState::Idle,
            SessionCommand::SendPrompt {
                client_mutation_id: "cmid-1".to_owned(),
            },
        )
        .expect_err("idle prompt must reject");

        assert_eq!(reject.code, SessionRejectCode::ModeRequired);
        assert_eq!(reject.current_state, SessionState::Idle);
        assert_eq!(reject.code_str(), "mode_required");
        assert!(reject.wire_reason().contains("current_state=idle"));
    }

    #[test]
    fn mode_then_prompt_uses_named_transition() {
        let armed = next(
            SessionState::Idle,
            SessionCommand::SetMode {
                mode: Some(SessionMode::AwaitReply),
            },
        )
        .expect("arm mode");
        assert_eq!(
            armed,
            SessionState::ModeArmed {
                mode: SessionMode::AwaitReply
            }
        );

        let pending = next(
            armed,
            SessionCommand::SendPrompt {
                client_mutation_id: "cmid-2".to_owned(),
            },
        )
        .expect("send prompt");
        assert_eq!(
            pending,
            SessionState::PromptPending {
                mode: SessionMode::AwaitReply,
                client_mutation_id: "cmid-2".to_owned()
            }
        );
        assert_eq!(
            accepted_prompt_state(pending, "cmid-2".to_owned()),
            SessionState::Dispatched {
                mode: SessionMode::AwaitReply,
                client_mutation_id: Some("cmid-2".to_owned())
            }
        );
    }

    #[test]
    fn active_session_can_steer_without_arming_mode() {
        let steered = next(
            SessionState::AgentRunning {
                mode: SessionMode::Infinite,
            },
            SessionCommand::SteerPrompt {
                client_mutation_id: "cmid-steer".to_owned(),
            },
        )
        .expect("live session can steer");

        assert_eq!(
            steered,
            SessionState::Dispatched {
                mode: SessionMode::Infinite,
                client_mutation_id: Some("cmid-steer".to_owned())
            }
        );
    }

    #[test]
    fn await_reply_stop_moves_to_wait_reply() {
        let stopped = next(
            SessionState::AgentRunning {
                mode: SessionMode::AwaitReply,
            },
            SessionCommand::AgentStopped,
        )
        .expect("stop");
        assert_eq!(
            stopped,
            SessionState::WaitReply {
                mode: SessionMode::AwaitReply
            }
        );
        assert_eq!(stopped.display_status(), WAITING_STATUS);
    }

    #[test]
    fn projected_status_keeps_live_runtime_active_without_mode() {
        assert_eq!(
            projected_status(false, None, None, Some(ACTIVE_STATUS)),
            ACTIVE_STATUS
        );
        assert_eq!(
            projected_status(false, None, Some(ACTIVE_STATUS), None),
            ACTIVE_STATUS
        );
        assert_eq!(
            projected_status(false, Some(MODE_AWAIT_REPLY), Some(STOPPED_STATUS), None),
            WAITING_STATUS
        );
        assert_eq!(
            SessionState::from_projection(None, None, Some(ACTIVE_STATUS)),
            SessionState::AgentRunning {
                mode: SessionMode::Infinite
            }
        );
    }

    #[test]
    fn prompt_delivery_failed_from_dispatched_rearms_same_mode() {
        let state = next(
            SessionState::Dispatched {
                mode: SessionMode::MaxTurns2,
                client_mutation_id: Some("cmid-dispatched".to_owned()),
            },
            SessionCommand::PromptDeliveryFailed,
        )
        .expect("delivery failure should be handled");

        assert_eq!(
            state,
            SessionState::ModeArmed {
                mode: SessionMode::MaxTurns2
            }
        );
    }

    #[test]
    fn prompt_delivery_failed_from_prompt_pending_rearms_same_mode() {
        let state = next(
            SessionState::PromptPending {
                mode: SessionMode::CompletionChecks,
                client_mutation_id: "cmid-pending".to_owned(),
            },
            SessionCommand::PromptDeliveryFailed,
        )
        .expect("delivery failure should be handled");

        assert_eq!(
            state,
            SessionState::ModeArmed {
                mode: SessionMode::CompletionChecks
            }
        );
    }

    #[test]
    fn prompt_delivery_failed_is_noop_for_settled_states() {
        for state in [
            SessionState::AgentRunning {
                mode: SessionMode::Infinite,
            },
            SessionState::Idle,
            SessionState::Done,
        ] {
            assert_eq!(
                next(state.clone(), SessionCommand::PromptDeliveryFailed)
                    .expect("delivery failure should be a no-op"),
                state
            );
        }
    }
}

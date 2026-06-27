//! Server-facing Session FSM surface.
//!
//! The transition table lives in `looper-session-core` so the control plane and
//! Rust client core compile against the same state machine. Keep this module as
//! the only server import path; do not duplicate transitions in handlers.

pub use looper_session_core::{
    ACTIVE_STATUS, STOPPED_STATUS, SessionCommand, SessionMode, SessionReject, SessionRejectCode,
    SessionState, accepted_prompt_state, hook_lifecycle_state, hook_lifecycle_status, next,
    projected_status,
};

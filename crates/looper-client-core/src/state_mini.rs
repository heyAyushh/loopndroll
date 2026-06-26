use crate::{
    error::ClientCoreError,
    model::{ClientStateMini, ClientStateMiniDelta},
};

const INITIAL_SEQUENCE: i64 = 0;

pub(crate) fn require_valid_sequence(sequence: i64) -> Result<(), ClientCoreError> {
    if sequence < INITIAL_SEQUENCE {
        Err(ClientCoreError::InvalidSequence)
    } else {
        Ok(())
    }
}

pub(crate) fn validate_state_minis(sessions: &[ClientStateMini]) -> Result<(), ClientCoreError> {
    for session in sessions {
        validate_state_mini(session)?;
    }
    Ok(())
}

pub(crate) fn validate_state_mini_delta(
    delta: &ClientStateMiniDelta,
) -> Result<(), ClientCoreError> {
    require_valid_sequence(delta.seq)?;
    require_valid_sequence(delta.latest_seq)?;
    if delta.has_session {
        validate_state_mini(&delta.session)?;
    }
    validate_state_minis(&delta.sessions)
}

pub(crate) fn validate_state_mini(session: &ClientStateMini) -> Result<(), ClientCoreError> {
    require_present(&session.session_id, ClientCoreError::EmptySessionId)?;
    require_valid_sequence(session.seq)
}

pub(crate) fn normalize_state_minis(sessions: Vec<ClientStateMini>) -> Vec<ClientStateMini> {
    let mut normalized = Vec::with_capacity(sessions.len());
    for session in sessions {
        if let Some(index) = normalized
            .iter()
            .position(|current| same_state_mini_key(current, &session))
        {
            normalized[index] = session;
        } else {
            normalized.push(session);
        }
    }
    sort_state_minis(&mut normalized);
    normalized
}

pub(crate) fn sort_state_minis(sessions: &mut [ClientStateMini]) {
    sessions.sort_by(|lhs, rhs| {
        lhs.seq
            .cmp(&rhs.seq)
            .then_with(|| lhs.assistant_surface.cmp(&rhs.assistant_surface))
            .then_with(|| lhs.session_id.cmp(&rhs.session_id))
    });
}

pub(crate) fn same_state_mini_key(lhs: &ClientStateMini, rhs: &ClientStateMini) -> bool {
    lhs.session_id == rhs.session_id && lhs.assistant_surface == rhs.assistant_surface
}

pub(crate) fn latest_state_mini_revision(sessions: &[ClientStateMini]) -> Option<String> {
    sessions
        .iter()
        .filter(|session| !session.revision.is_empty())
        .max_by(|lhs, rhs| {
            lhs.seq
                .cmp(&rhs.seq)
                .then_with(|| lhs.assistant_surface.cmp(&rhs.assistant_surface))
                .then_with(|| lhs.session_id.cmp(&rhs.session_id))
        })
        .map(|session| session.revision.clone())
}

fn require_present(value: &str, error: ClientCoreError) -> Result<(), ClientCoreError> {
    if value.trim().is_empty() {
        Err(error)
    } else {
        Ok(())
    }
}

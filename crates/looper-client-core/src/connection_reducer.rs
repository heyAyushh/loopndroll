use crate::error::ClientCoreError;

const CONNECTING: &str = "connecting";
const CONNECTED: &str = "connected";
const OFFLINE: &str = "offline";
const UNAUTHORIZED: &str = "unauthorized";
const LOCKED: &str = "locked";
const UNPAIRED: &str = "unpaired";

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientSnapshotLoadFailureProjection {
    pub connection_state: String,
    pub preserved_connected_state: bool,
    pub should_clear_route_state: bool,
    pub should_suppress_error: bool,
}

#[uniffi::export]
pub fn reduce_snapshot_load_failure(
    mapped_error_state: String,
    current_connection_state: String,
    has_usable_snapshot: bool,
    has_server_health: bool,
    has_reached_base_url: bool,
) -> Result<ClientSnapshotLoadFailureProjection, ClientCoreError> {
    let error_state = normalize_connection_state(&mapped_error_state)?;
    let current_state = normalize_connection_state(&current_connection_state)?;
    let should_preserve_connected = should_preserve_connected_state(
        error_state,
        current_state,
        has_usable_snapshot,
        has_server_health,
        has_reached_base_url,
    );
    let next_state = if should_preserve_connected {
        CONNECTED
    } else {
        error_state
    };

    Ok(ClientSnapshotLoadFailureProjection {
        connection_state: next_state.to_owned(),
        preserved_connected_state: should_preserve_connected,
        should_clear_route_state: !allows_connection_route_presentation(next_state),
        should_suppress_error: should_suppress_snapshot_load_error(next_state, has_usable_snapshot),
    })
}

fn should_preserve_connected_state(
    error_state: &str,
    current_state: &str,
    has_usable_snapshot: bool,
    has_server_health: bool,
    has_reached_base_url: bool,
) -> bool {
    has_usable_snapshot
        && error_state == OFFLINE
        && (current_state == CONNECTED || has_server_health || has_reached_base_url)
}

fn should_suppress_snapshot_load_error(state: &str, has_usable_snapshot: bool) -> bool {
    has_usable_snapshot && matches!(state, CONNECTED | OFFLINE)
}

fn allows_connection_route_presentation(state: &str) -> bool {
    matches!(state, CONNECTED | UNAUTHORIZED | LOCKED)
}

fn normalize_connection_state(value: &str) -> Result<&'static str, ClientCoreError> {
    match value.trim() {
        CONNECTING => Ok(CONNECTING),
        CONNECTED => Ok(CONNECTED),
        OFFLINE => Ok(OFFLINE),
        UNAUTHORIZED => Ok(UNAUTHORIZED),
        LOCKED => Ok(LOCKED),
        UNPAIRED => Ok(UNPAIRED),
        _ => Err(ClientCoreError::InvalidConnectionState),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_connected_when_cached_state_is_still_usable() {
        let projection = reduce_snapshot_load_failure(
            OFFLINE.to_owned(),
            CONNECTED.to_owned(),
            true,
            false,
            false,
        )
        .expect("projection");

        assert_eq!(projection.connection_state, CONNECTED);
        assert!(projection.preserved_connected_state);
        assert!(!projection.should_clear_route_state);
        assert!(projection.should_suppress_error);
    }

    #[test]
    fn preserves_connected_when_route_metadata_exists() {
        let projection =
            reduce_snapshot_load_failure(OFFLINE.to_owned(), OFFLINE.to_owned(), true, true, false)
                .expect("projection");

        assert_eq!(projection.connection_state, CONNECTED);
        assert!(projection.preserved_connected_state);
        assert!(!projection.should_clear_route_state);
        assert!(projection.should_suppress_error);
    }

    #[test]
    fn offline_without_usable_snapshot_clears_route_and_surfaces_error() {
        let projection = reduce_snapshot_load_failure(
            OFFLINE.to_owned(),
            CONNECTED.to_owned(),
            false,
            true,
            true,
        )
        .expect("projection");

        assert_eq!(projection.connection_state, OFFLINE);
        assert!(!projection.preserved_connected_state);
        assert!(projection.should_clear_route_state);
        assert!(!projection.should_suppress_error);
    }

    #[test]
    fn unauthorized_keeps_route_context_and_surfaces_error() {
        let projection = reduce_snapshot_load_failure(
            UNAUTHORIZED.to_owned(),
            CONNECTED.to_owned(),
            true,
            true,
            true,
        )
        .expect("projection");

        assert_eq!(projection.connection_state, UNAUTHORIZED);
        assert!(!projection.preserved_connected_state);
        assert!(!projection.should_clear_route_state);
        assert!(!projection.should_suppress_error);
    }

    #[test]
    fn invalid_connection_state_is_rejected() {
        let error = reduce_snapshot_load_failure(
            "ready".to_owned(),
            CONNECTED.to_owned(),
            true,
            false,
            false,
        )
        .expect_err("invalid state");

        assert_eq!(error, ClientCoreError::InvalidConnectionState);
    }
}

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

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct ClientConnectionFailureProjection {
    pub connection_state: String,
    pub should_clear_route_state: bool,
    pub should_suppress_error: bool,
}

/// `current_connection_state`, `has_server_health`, and `has_reached_base_url` are part
/// of the stable FFI signature (Swift already builds and passes these) but are
/// deliberately not used to influence the projection: an earlier version of this
/// function used them to report a synthetic "connected" state from cached metadata
/// when a snapshot load failed, which let a genuinely offline client claim it was
/// connected (see the "stop cached routes from reporting connected" fix). Keep
/// reporting the real error state instead of resurrecting that behavior. The
/// parameters are still validated so callers get a consistent error contract for
/// malformed input, even though the values themselves are otherwise unused.
#[uniffi::export]
pub fn reduce_snapshot_load_failure(
    mapped_error_state: String,
    current_connection_state: String,
    has_usable_snapshot: bool,
    has_server_health: bool,
    has_reached_base_url: bool,
) -> Result<ClientSnapshotLoadFailureProjection, ClientCoreError> {
    let error_state = normalize_connection_state(&mapped_error_state)?;
    normalize_connection_state(&current_connection_state)?;
    let _ = (has_server_health, has_reached_base_url);

    Ok(ClientSnapshotLoadFailureProjection {
        connection_state: error_state.to_owned(),
        preserved_connected_state: false,
        should_clear_route_state: !allows_connection_route_presentation(error_state),
        should_suppress_error: should_suppress_snapshot_load_error(
            error_state,
            has_usable_snapshot,
        ),
    })
}

#[uniffi::export]
pub fn reduce_connection_failure(
    mapped_error_state: String,
    has_usable_snapshot: bool,
    suppress_error_when_snapshot_usable: bool,
) -> Result<ClientConnectionFailureProjection, ClientCoreError> {
    let state = normalize_connection_state(&mapped_error_state)?;

    Ok(ClientConnectionFailureProjection {
        connection_state: state.to_owned(),
        should_clear_route_state: !allows_connection_route_presentation(state),
        should_suppress_error: suppress_error_when_snapshot_usable
            && should_suppress_snapshot_load_error(state, has_usable_snapshot),
    })
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
    fn cached_state_does_not_fake_connected_state() {
        let projection = reduce_snapshot_load_failure(
            OFFLINE.to_owned(),
            CONNECTED.to_owned(),
            true,
            false,
            false,
        )
        .expect("projection");

        assert_eq!(projection.connection_state, OFFLINE);
        assert!(!projection.preserved_connected_state);
        assert!(projection.should_clear_route_state);
        assert!(projection.should_suppress_error);
    }

    #[test]
    fn route_metadata_does_not_fake_connected_state() {
        let projection =
            reduce_snapshot_load_failure(OFFLINE.to_owned(), OFFLINE.to_owned(), true, true, false)
                .expect("projection");

        assert_eq!(projection.connection_state, OFFLINE);
        assert!(!projection.preserved_connected_state);
        assert!(projection.should_clear_route_state);
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

    #[test]
    fn generic_failure_can_suppress_cached_offline_error() {
        let projection =
            reduce_connection_failure(OFFLINE.to_owned(), true, true).expect("projection");

        assert_eq!(projection.connection_state, OFFLINE);
        assert!(projection.should_clear_route_state);
        assert!(projection.should_suppress_error);
    }

    #[test]
    fn generic_failure_can_force_error_surface() {
        let projection =
            reduce_connection_failure(OFFLINE.to_owned(), true, false).expect("projection");

        assert_eq!(projection.connection_state, OFFLINE);
        assert!(projection.should_clear_route_state);
        assert!(!projection.should_suppress_error);
    }
}

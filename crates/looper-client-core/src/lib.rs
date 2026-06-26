uniffi::setup_scaffolding!();

mod client;
mod command_batch;
mod connection_reducer;
mod error;
mod local_store;
mod menu_snapshot;
mod mobile_snapshot;
mod model;
mod session_runtime;
mod session_transport;
mod snapshot_reducer;
mod state_mini;
mod transport;

pub use client::LooperClientCore;
pub use connection_reducer::{
    ClientConnectionFailureProjection, ClientSnapshotLoadFailureProjection,
    reduce_connection_failure, reduce_snapshot_load_failure,
};
pub use error::ClientCoreError;
pub use local_store::{DEFAULT_LOCAL_STORE_FILE_NAME, LooperClientCoreLocalStore};
pub use menu_snapshot::{
    ClientMenuBarSessionMini, ClientMenuBarSessionMiniBlockedGoal,
    ClientMenuBarSessionMiniLocalSnapshot, ClientMenuBarSessionMiniNotificationStatus,
    ClientMenuBarSessionMiniPendingCommand, reduce_state_minis_menu_snapshot,
};
pub use mobile_snapshot::{ClientMobileSnapshotProjection, reduce_state_minis_mobile_snapshot};
pub use model::{
    ClientCommandAck, ClientCommandAckEnvelope, ClientCommandBatchResponse, ClientCommandKind,
    ClientCommandMetadata, ClientEndpoint, ClientLocalStateSnapshot, ClientPendingCommand,
    ClientPendingCommandKind, ClientPendingMutation, ClientStateDelta, ClientStateMini,
    ClientStateMiniDelta, ClientStateMiniDeltaApplyResult, ClientStateMiniSnapshot,
    ClientStateSnapshot, ConnectionPhase, OutboundSessionFrame, OutboundSessionFrameKind,
};
pub use session_runtime::LooperClientCoreSessionRuntime;
pub use snapshot_reducer::{
    ClientAssistantSurfaceSelection, ClientDetailCacheProjection, ClientDetailModeProjection,
    ClientOptimisticModeProjection, ClientSessionIndexEntry, ClientSessionIndexProjection,
    ClientSessionSectionsProjection, ClientSiriSessionEntityProjection, ClientSnapshotProjection,
    reduce_assistant_surface_selection, reduce_mobile_snapshot_detail_cache,
    reduce_mobile_snapshot_optimistic_mode, reduce_mobile_snapshot_projection,
    reduce_session_detail_optimistic_mode, reduce_session_index, reduce_session_sections,
    reduce_siri_session_entities,
};

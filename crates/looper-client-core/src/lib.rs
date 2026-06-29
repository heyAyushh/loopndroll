uniffi::setup_scaffolding!();

mod client;
mod command_batch;
mod connection_reducer;
mod error;
mod local_store;
mod menu_snapshot;
mod mobile_snapshot;
mod model;
mod race_plan;
mod session_runtime;
mod session_transport;
mod snapshot_reducer;
mod state_mini;
mod transport;

pub use connection_reducer::{
    ClientConnectionFailureProjection, ClientSnapshotLoadFailureProjection,
    reduce_connection_failure, reduce_snapshot_load_failure,
};
pub use error::ClientCoreError;
pub use local_store::DEFAULT_LOCAL_STORE_FILE_NAME;
pub use menu_snapshot::{
    ClientMenuBarHumanStatusProjection, ClientMenuBarSessionMini,
    ClientMenuBarSessionMiniBlockedGoal, ClientMenuBarSessionMiniLocalSnapshot,
    ClientMenuBarSessionMiniNotificationStatus, ClientMenuBarSessionMiniPendingCommand,
    ClientMenuSnapshotStreamUpdate, reduce_menu_snapshot_human_status,
    reduce_state_minis_menu_snapshot,
};
pub use mobile_snapshot::{
    ClientMobileSnapshotProjection, reduce_state_minis_mobile_snapshot,
    reduce_state_minis_mobile_snapshot_with_pending_commands,
};
pub use model::{
    ClientBaseUrlRaceCandidate, ClientCommandKind, ClientEndpoint, ClientLocalStateSnapshot,
    ClientMobileSnapshotStreamUpdate, ClientNotificationReplyIntentResult,
    ClientNotificationReplyPersistResult, ClientPendingCommand, ClientPendingCommandKind,
    ClientPendingMutation, ClientSessionModeIntentResult, ClientSessionPromptIntentResult,
    ClientStateDelta, ClientStateMini, ClientStateMiniDelta, ClientStateMiniDeltaApplyResult,
    ClientStateMiniSnapshot, ClientStateSnapshot, ConnectionPhase,
};
pub use race_plan::{
    default_base_url_race_fallback_delay_nanoseconds, plan_base_url_race_candidates,
};
pub use session_runtime::LooperClientCoreSessionRuntime;
pub use snapshot_reducer::{
    ClientAssistantSurfaceSelection, ClientDetailCacheProjection, ClientDetailModeProjection,
    ClientOptimisticModeProjection, ClientSessionFreshnessOrderProjection, ClientSessionIndexEntry,
    ClientSessionIndexProjection, ClientSessionSectionsProjection,
    ClientSiriSessionEntityProjection, ClientSnapshotProjection,
    reduce_assistant_surface_selection, reduce_mobile_snapshot_detail_cache,
    reduce_mobile_snapshot_optimistic_mode, reduce_mobile_snapshot_projection,
    reduce_session_detail_optimistic_mode, reduce_session_freshness_order, reduce_session_index,
    reduce_session_sections, reduce_siri_session_entities,
};

uniffi::setup_scaffolding!();

mod client;
mod command_batch;
mod connection_reducer;
mod error;
mod mobile_snapshot;
mod model;
mod mutation_queue;
mod snapshot_reducer;
mod transport;

pub use client::LooperClientCore;
pub use command_batch::build_command_batch_response;
pub use connection_reducer::{
    ClientConnectionFailureProjection, ClientSnapshotLoadFailureProjection,
    reduce_connection_failure, reduce_snapshot_load_failure,
};
pub use error::ClientCoreError;
pub use mobile_snapshot::{ClientMobileSnapshotProjection, reduce_state_minis_mobile_snapshot};
pub use model::{
    ClientCommandAck, ClientCommandAckEnvelope, ClientCommandBatchResponse, ClientCommandKind,
    ClientCommandMetadata, ClientEndpoint, ClientPendingMutation, ClientStateDelta,
    ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot, ClientStateSnapshot,
    ConnectionPhase, OutboundSessionFrame, OutboundSessionFrameKind,
};
pub use mutation_queue::{
    ClientModeMutation, ClientModeMutationBatchFinish, ClientModeMutationDrainFinish,
    ClientModeMutationEnqueueResult, ClientModeMutationOption, ClientModeMutationQueue,
};
pub use snapshot_reducer::{
    ClientDetailCacheProjection, ClientDetailModeProjection, ClientOptimisticModeProjection,
    ClientSnapshotProjection, reduce_mobile_snapshot_detail_cache,
    reduce_mobile_snapshot_optimistic_mode, reduce_mobile_snapshot_projection,
    reduce_session_detail_optimistic_mode,
};

uniffi::setup_scaffolding!();

mod client;
mod command_batch;
mod error;
mod model;
mod transport;

pub use client::LooperClientCore;
pub use command_batch::build_command_batch_response;
pub use error::ClientCoreError;
pub use model::{
    ClientCommandAck, ClientCommandAckEnvelope, ClientCommandBatchResponse, ClientCommandKind,
    ClientCommandMetadata, ClientEndpoint, ClientPendingMutation, ClientStateDelta,
    ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot, ClientStateSnapshot,
    ConnectionPhase, OutboundSessionFrame, OutboundSessionFrameKind,
};

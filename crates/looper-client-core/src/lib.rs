uniffi::setup_scaffolding!();

mod client;
mod error;
mod model;
mod transport;

pub use client::LooperClientCore;
pub use error::ClientCoreError;
pub use model::{
    ClientCommandAck, ClientCommandKind, ClientEndpoint, ClientPendingMutation, ClientStateDelta,
    ClientStateMini, ClientStateMiniDelta, ClientStateMiniSnapshot, ClientStateSnapshot,
    ConnectionPhase, OutboundSessionFrame, OutboundSessionFrameKind,
};

import Foundation
import LooperClientCore

/// Connection phase as the UI understands it. Derived only by
/// `CompanionConnectionReducer` — nothing else may write it.
enum CompanionConnectionPhase: Equatable, Sendable {
    case unconfigured
    case unpaired
    case unauthorized
    case locked
    case connecting
    case live
    case degraded
    case offline
}

/// Data freshness, deliberately separate from the phase: a connected stream
/// is NOT proof the projection is current. `liveFresh` requires a
/// delta/heartbeat/text-chunk observed for the CURRENT stream generation.
enum CompanionDataFreshness: Equatable, Sendable {
    case liveFresh
    case degraded(lastFreshAt: Date?)
    case cachedOnly
}

/// What a single stream update means, derived exactly once from
/// `ClientMobileSnapshotStreamUpdate` (replaces per-call-site string matching
/// on `syncReason`).
enum CompanionConnectionStreamSignal: Equatable, Sendable {
    /// delta / heartbeat / text_chunk — proof the stream is live at this seq.
    case liveActivity(latestSeq: Int64)
    /// A snapshot carried for another reason (cached replay, resync).
    case snapshotReplay(latestSeq: Int64)
    /// Update carried no liveness evidence (e.g. bookkeeping frame).
    case none

    init(streamUpdate: ClientMobileSnapshotStreamUpdate) {
        switch streamUpdate.syncReason {
        case CompanionSessionMiniSyncReason.delta,
             CompanionSessionMiniSyncReason.heartbeat,
             CompanionSessionMiniSyncReason.textChunk:
            self = .liveActivity(latestSeq: streamUpdate.latestSeq)
        default:
            self = streamUpdate.hasSnapshot
                ? .snapshotReplay(latestSeq: streamUpdate.latestSeq)
                : .none
        }
    }
}

/// Typed inputs to the connection reducer. Every source of connection truth
/// funnels through these events; there is no other writer.
enum CompanionConnectionEvent: Equatable, Sendable {
    /// A stream update was observed on the given stream generation.
    case streamUpdate(CompanionConnectionStreamSignal, streamGeneration: UInt64)
    /// The stream loop for the given generation exited.
    case streamExited(streamGeneration: UInt64, isNetworkError: Bool)
    /// A background recovery finished and its snapshot was applied.
    case recoveryCompleted(latestSeq: Int64)
    case recoveryFailed
    /// Cached snapshot replayed from disk before any network evidence.
    case cachedSnapshotRestored
    /// Auth-shaped failures mapped from stream/recovery errors.
    case authorizationRequired
    case lockedByServer
    case unlocked
    /// Connection configuration present/absent at startup or after pairing.
    case configured
    case unconfigured(isPaired: Bool)
}

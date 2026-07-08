import Foundation

/// The reducer's whole world: phase + freshness + the stream generation and
/// seq that justify them. Value type so tests can table-drive transitions.
struct CompanionConnectionMachineState: Equatable, Sendable {
    var phase: CompanionConnectionPhase
    var freshness: CompanionDataFreshness
    /// Generation of the stream loop whose evidence produced `freshness`.
    /// Updates from older generations are ignored — a reconnect must re-prove
    /// liveness before the UI may claim fresh again.
    var currentStreamGeneration: UInt64
    var latestSeq: Int64
    /// Wall-clock moment of the last live-activity proof, kept for the
    /// degraded label ("fresh as of ...").
    var lastFreshAt: Date?

    static let initial = CompanionConnectionMachineState(
        phase: .unconfigured,
        freshness: .cachedOnly,
        currentStreamGeneration: 0,
        latestSeq: 0,
        lastFreshAt: nil
    )
}

/// Pure state machine: `(state, event, now) -> state`. No I/O, no clocks of
/// its own — `now` comes in so tests stay deterministic.
enum CompanionConnectionReducer {
    /// Recovery may be skipped entirely when the stream is live and fresh —
    /// a live stream with a current seq is definitionally up to date.
    static func shouldSkipRefreshRecovery(_ state: CompanionConnectionMachineState) -> Bool {
        state.phase == .live && state.freshness == .liveFresh
    }

    static func reduce(
        _ state: CompanionConnectionMachineState,
        event: CompanionConnectionEvent,
        now: Date
    ) -> CompanionConnectionMachineState {
        var state = state

        switch event {
        case let .streamUpdate(signal, streamGeneration):
            guard streamGeneration >= state.currentStreamGeneration else {
                return state
            }
            state.currentStreamGeneration = streamGeneration
            switch signal {
            case let .liveActivity(latestSeq):
                state.phase = .live
                state.freshness = .liveFresh
                state.latestSeq = max(state.latestSeq, latestSeq)
                state.lastFreshAt = now
            case let .snapshotReplay(latestSeq):
                state.latestSeq = max(state.latestSeq, latestSeq)
                if state.phase == .unconfigured || state.phase == .connecting {
                    state.phase = .connecting
                }
            case .none:
                break
            }

        case let .streamExited(streamGeneration, isNetworkError):
            guard streamGeneration >= state.currentStreamGeneration else {
                return state
            }
            // Data on screen is whatever was last proven; the pipe is gone.
            state.phase = isNetworkError ? .offline : .degraded
            state.freshness = .degraded(lastFreshAt: state.lastFreshAt)

        case let .recoveryCompleted(latestSeq):
            state.latestSeq = max(state.latestSeq, latestSeq)
            // Recovery refreshes data but is not stream liveness. Phase only
            // improves to degraded (from offline); freshness records the
            // recovery moment without claiming liveFresh.
            if state.phase == .offline {
                state.phase = .degraded
            }
            if state.freshness != .liveFresh {
                state.freshness = .degraded(lastFreshAt: now)
                state.lastFreshAt = now
            }

        case .recoveryFailed:
            if state.phase == .live {
                return state
            }
            state.freshness = state.lastFreshAt.map { .degraded(lastFreshAt: $0) } ?? .cachedOnly

        case .cachedSnapshotRestored:
            if state.freshness == .liveFresh {
                return state
            }
            if case .degraded = state.freshness {
                return state
            }
            state.freshness = .cachedOnly

        case .authorizationRequired:
            state.phase = .unauthorized
            state.freshness = .degraded(lastFreshAt: state.lastFreshAt)

        case .lockedByServer:
            state.phase = .locked

        case .unlocked:
            if state.phase == .locked {
                state.phase = .connecting
            }

        case .configured:
            if state.phase == .unconfigured || state.phase == .unpaired {
                state.phase = .connecting
            }

        case let .unconfigured(isPaired):
            state.phase = isPaired ? .unconfigured : .unpaired
            state.freshness = .cachedOnly
        }

        return state
    }

    /// A new stream loop iteration announces itself so stale-generation
    /// updates can be discarded and liveness must be re-proven.
    static func startingStream(
        _ state: CompanionConnectionMachineState,
        generation: UInt64
    ) -> CompanionConnectionMachineState {
        var state = state
        state.currentStreamGeneration = generation
        if state.phase != .locked, state.phase != .unauthorized {
            state.phase = state.phase == .live ? .live : .connecting
        }
        // Fresh claims from the previous stream die with it.
        if state.freshness == .liveFresh {
            state.freshness = .degraded(lastFreshAt: state.lastFreshAt)
        }
        return state
    }
}

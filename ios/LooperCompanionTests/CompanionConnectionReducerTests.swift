import Foundation
import Testing
@testable import Looper

@Suite("Connection reducer")
struct CompanionConnectionReducerTests {
    private let now = Date(timeIntervalSince1970: 1_000_000)
    private let earlier = Date(timeIntervalSince1970: 999_000)

    private var liveState: CompanionConnectionMachineState {
        CompanionConnectionReducer.reduce(
            CompanionConnectionReducer.startingStream(.initial, generation: 1),
            event: .streamUpdate(.liveActivity(latestSeq: 42), streamGeneration: 1),
            now: now
        )
    }

    @Test("Live activity on current generation proves live + fresh")
    func liveActivityProvesFresh() {
        let state = liveState
        #expect(state.phase == .live)
        #expect(state.freshness == .liveFresh)
        #expect(state.latestSeq == 42)
        #expect(state.lastFreshAt == now)
    }

    @Test("Stale-generation update is ignored — stream-connected is not fresh")
    func staleGenerationIgnored() {
        var state = liveState
        state = CompanionConnectionReducer.startingStream(state, generation: 2)
        // Old stream's heartbeat arrives late; it must not re-prove freshness.
        let after = CompanionConnectionReducer.reduce(
            state,
            event: .streamUpdate(.liveActivity(latestSeq: 99), streamGeneration: 1),
            now: now
        )
        #expect(after == state)
        #expect(after.freshness != .liveFresh)
    }

    @Test("Starting a new stream demotes liveFresh to degraded")
    func newStreamDemotesFreshness() {
        let state = CompanionConnectionReducer.startingStream(liveState, generation: 2)
        #expect(state.freshness == .degraded(lastFreshAt: now))
        #expect(state.phase == .live)
    }

    @Test("Network stream exit goes offline; non-network goes degraded")
    func streamExitPhases() {
        let offline = CompanionConnectionReducer.reduce(
            liveState,
            event: .streamExited(streamGeneration: 1, isNetworkError: true),
            now: now
        )
        #expect(offline.phase == .offline)
        #expect(offline.freshness == .degraded(lastFreshAt: now))

        let degraded = CompanionConnectionReducer.reduce(
            liveState,
            event: .streamExited(streamGeneration: 1, isNetworkError: false),
            now: now
        )
        #expect(degraded.phase == .degraded)
    }

    @Test("Recovery refreshes data but never claims liveFresh")
    func recoveryIsNotLiveness() {
        var state = CompanionConnectionReducer.reduce(
            liveState,
            event: .streamExited(streamGeneration: 1, isNetworkError: true),
            now: earlier
        )
        state = CompanionConnectionReducer.reduce(
            state,
            event: .recoveryCompleted(latestSeq: 50),
            now: now
        )
        #expect(state.phase == .degraded)
        #expect(state.freshness == .degraded(lastFreshAt: now))
        #expect(state.latestSeq == 50)
    }

    @Test("Refresh recovery is skipped only when live AND fresh")
    func refreshSkipRule() {
        #expect(CompanionConnectionReducer.shouldSkipRefreshRecovery(liveState))

        let exited = CompanionConnectionReducer.reduce(
            liveState,
            event: .streamExited(streamGeneration: 1, isNetworkError: false),
            now: now
        )
        #expect(!CompanionConnectionReducer.shouldSkipRefreshRecovery(exited))

        let newStream = CompanionConnectionReducer.startingStream(liveState, generation: 2)
        #expect(!CompanionConnectionReducer.shouldSkipRefreshRecovery(newStream))
    }

    @Test("Seq never rewinds")
    func seqMonotonic() {
        let state = CompanionConnectionReducer.reduce(
            liveState,
            event: .streamUpdate(.liveActivity(latestSeq: 7), streamGeneration: 1),
            now: now
        )
        #expect(state.latestSeq == 42)
    }

    @Test("Cached restore cannot downgrade fresher evidence")
    func cachedRestoreDoesNotDowngrade() {
        let fresh = CompanionConnectionReducer.reduce(
            liveState,
            event: .cachedSnapshotRestored,
            now: now
        )
        #expect(fresh.freshness == .liveFresh)

        let cold = CompanionConnectionReducer.reduce(
            .initial,
            event: .cachedSnapshotRestored,
            now: now
        )
        #expect(cold.freshness == .cachedOnly)
    }

    @Test("Auth failure and server lock map to their phases")
    func authAndLockPhases() {
        let unauthorized = CompanionConnectionReducer.reduce(
            liveState,
            event: .authorizationRequired,
            now: now
        )
        #expect(unauthorized.phase == .unauthorized)

        let locked = CompanionConnectionReducer.reduce(
            liveState,
            event: .lockedByServer,
            now: now
        )
        #expect(locked.phase == .locked)

        let unlocked = CompanionConnectionReducer.reduce(
            locked,
            event: .unlocked,
            now: now
        )
        #expect(unlocked.phase == .connecting)
    }

    @Test("Recovery failure with no prior evidence is cachedOnly")
    func recoveryFailureCold() {
        let state = CompanionConnectionReducer.reduce(
            CompanionConnectionReducer.startingStream(.initial, generation: 1),
            event: .recoveryFailed,
            now: now
        )
        #expect(state.freshness == .cachedOnly)
    }
}

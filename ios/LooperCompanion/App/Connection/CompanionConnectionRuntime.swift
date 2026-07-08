import Foundation
import LooperClientCore
import LooperCompanionCore
import Network

typealias CompanionNotificationReplySubmitter = @MainActor @Sendable () async -> Bool

/// Everything the runtime tells the app model when connection truth changes.
/// One callback bundle instead of a delegate protocol per controller — the
/// model wires these once and the runtime is the ONLY writer behind them.
@MainActor
struct CompanionConnectionRuntimeCallbacks {
    let applySnapshotUpdate: (CompanionSessionMiniSyncUpdate, _ streamGeneration: UInt64) -> Void
    let applyLiveness: (CompanionSessionMiniLivenessUpdate) -> Void
    /// Fast path for live-reply streaming: called directly off the raw
    /// `TextChunk` frame, ahead of `applySnapshotUpdate`, so the visible text
    /// never waits on the full snapshot projection.
    let applyTextChunk: (ClientTextChunk) -> Void
    let refreshPendingPromptDeliveryState: () -> Void
    let recoverySnapshotArrived: (CompanionRecoveredSessionMiniSnapshot) -> Void
    /// Replays the local store into the visible snapshot (seq-gated by the
    /// model). Cheap and offline; used before any background recovery so
    /// state written by other processes (notification extension) surfaces
    /// immediately on refresh.
    let replayLocalStore: (_ reason: String) -> Bool
}

/// Single owner of the realtime connection: stream supervision with jittered
/// backoff, network-path restarts, typed phase/freshness state, single-flight
/// refresh, and the notification-reply outbox drain.
///
/// Replaces `CompanionSessionMiniController` (stream forever-loop, fixed
/// 750ms retry, synthetic liveness strings) and
/// `CompanionConnectionController` (delegate-driven liveness/connection-state
/// mutation, connectionRevision, NWPathMonitor).
@MainActor
@Observable
final class CompanionConnectionRuntime {
    private enum Backoff {
        static let floorMilliseconds: Int64 = 250
        static let ceilingMilliseconds: Int64 = 5_000
    }

    private(set) var machine = CompanionConnectionMachineState.initial

    /// When the app last transitioned from inactive to active. The view
    /// layer clamps its freshness clock to this so time spent suspended in
    /// the background never counts against the reconnect grace window.
    private(set) var lastBecameActiveAt: Date?

    let sessionRuntime: CompanionSessionRuntime?

    @ObservationIgnored private var callbacks: CompanionConnectionRuntimeCallbacks?
    @ObservationIgnored private var supervisorTask: Task<Void, Never>?
    @ObservationIgnored private var streamGeneration: UInt64 = 0
    @ObservationIgnored private var refreshRecoveryTask: Task<Void, Never>?
    @ObservationIgnored private var notificationReplyOutboxDrainTask: Task<Bool, Never>?
    @ObservationIgnored private var watchdogTask: Task<Void, Never>?
    @ObservationIgnored private var networkPathMonitor: NWPathMonitor?
    @ObservationIgnored private var lastNetworkPathIdentity: String?
    /// Chains `sessionRuntime.stopStateMiniStream()` FFI calls (which flush
    /// the local store to disk synchronously inside the Rust core) onto a
    /// background executor instead of the main actor. `startSyncIfNeeded()`
    /// awaits this before priming a new stream, so "stop fully finishes
    /// before start reuses the stream identity" still holds even though
    /// neither `stopSync` nor its callers need to become `async` — only the
    /// blocking I/O moves off the main actor, per the fix constraint.
    @ObservationIgnored private var pendingSessionRuntimeStopTask: Task<Void, Never>?

    var isSyncing: Bool {
        supervisorTask != nil
    }

    /// Matches the old `connectionRevision` consumers: identifies which
    /// stream loop produced an update so stale applies can be dropped.
    var currentStreamGeneration: UInt64 {
        streamGeneration
    }

    init(sessionRuntime: CompanionSessionRuntime?) {
        self.sessionRuntime = sessionRuntime
    }

    func configure(callbacks: CompanionConnectionRuntimeCallbacks) {
        self.callbacks = callbacks
    }

    // MARK: - Reducer input

    private func apply(_ event: CompanionConnectionEvent) {
        machine = CompanionConnectionReducer.reduce(machine, event: event, now: Date())
    }

    // MARK: - Stream supervision

    func startSyncIfNeeded() {
        guard supervisorTask == nil else {
            return
        }

        guard let sessionRuntime else {
            CompanionDiagnostics.record("session-mini:sync-unavailable")
            apply(.streamExited(streamGeneration: streamGeneration, isNetworkError: false))
            callbacks?.applyLiveness(CompanionSessionMiniController_runtimeUnavailableLiveness())
            return
        }

        supervisorTask = Task { [weak self] in
            // If a route/network-path restart just cancelled the previous
            // stream, its `stopStateMiniStream()` FFI call (disk flush) may
            // still be draining on a background task. Wait for it here —
            // off the main actor — so this stream doesn't reuse the
            // underlying Rust stream identity before the old one is torn
            // down, without blocking the caller that triggered the restart.
            let priorStopTask = await self?.pendingSessionRuntimeStopTask
            await priorStopTask?.value

            var attempt = 0
            while let self, !Task.isCancelled {
                let generation = self.beginStreamGeneration()

                await sessionRuntime.prepareSessionRuntime()
                await sessionRuntime.runStateMiniSync(
                    onUpdate: { [weak self] update in
                        self?.handleStreamUpdate(update, generation: generation)
                    },
                    onLiveness: { [weak self] liveness in
                        self?.handleLivenessUpdate(liveness, generation: generation)
                    },
                    onTextChunk: { [weak self] chunk in
                        self?.handleTextChunk(chunk, generation: generation)
                    },
                    onDebugMessage: { message in
                        CompanionDiagnostics.record(message)
                    }
                )

                guard !Task.isCancelled else {
                    return
                }

                self.handleStreamExit(generation: generation)
                attempt += 1
                try? await Task.sleep(for: Self.backoffDelay(attempt: attempt))
            }
        }
    }

    func stopSync(reconnectInProgress: Bool = false) {
        let task = supervisorTask
        supervisorTask = nil
        task?.cancel()

        streamGeneration += 1
        apply(.streamExited(
            streamGeneration: streamGeneration,
            isNetworkError: false
        ))
        if reconnectInProgress {
            machine = CompanionConnectionReducer.startingStream(machine, generation: streamGeneration)
        }

        guard let sessionRuntime else {
            return
        }

        // `sessionRuntime.stopStateMiniStream()` calls into the Rust core's
        // `stop()`, which synchronously flushes the local store to disk
        // (crates/looper-client-core/src/session_runtime.rs) before
        // returning. Calling it inline here — as this function used to —
        // blocked the main actor for however long that flush took every
        // time the Settings route picker was toggled. The state-machine
        // writes above still happen synchronously and instantly; only this
        // blocking FFI call moves to a background task, chained after any
        // still-draining prior stop so ordering against a subsequent
        // `startSyncIfNeeded()` is preserved (see the await there).
        let priorStopTask = pendingSessionRuntimeStopTask
        let stopFFIStartedAt = Date()
        pendingSessionRuntimeStopTask = Task.detached(priority: .userInitiated) {
            _ = await priorStopTask?.value
            sessionRuntime.stopStateMiniStream()
            CompanionDiagnostics.record(
                "route-switch:stop-ffi-detached ms=\(CompanionDiagnostics.elapsedMilliseconds(since: stopFFIStartedAt))"
            )
        }
    }

    func restartForRouteChange() {
        if isSyncing {
            stopSync(reconnectInProgress: true)
        }
        startSyncIfNeeded()
    }

    private func beginStreamGeneration() -> UInt64 {
        streamGeneration += 1
        machine = CompanionConnectionReducer.startingStream(machine, generation: streamGeneration)
        return streamGeneration
    }

    private func handleStreamUpdate(
        _ result: CompanionSessionMiniSyncUpdate,
        generation: UInt64
    ) {
        guard generation == streamGeneration else {
            CompanionDiagnostics.record("session-mini:sync-stale-skip")
            return
        }
        callbacks?.refreshPendingPromptDeliveryState()
        callbacks?.applySnapshotUpdate(result, generation)
    }

    private func handleTextChunk(
        _ chunk: ClientTextChunk,
        generation: UInt64
    ) {
        guard generation == streamGeneration else {
            CompanionDiagnostics.record("session-mini:text-chunk-stale-skip")
            return
        }
        callbacks?.applyTextChunk(chunk)
    }

    private func handleLivenessUpdate(
        _ liveness: CompanionSessionMiniLivenessUpdate,
        generation: UInt64
    ) {
        guard generation == streamGeneration else {
            CompanionDiagnostics.record("session-mini:liveness-stale-skip")
            return
        }
        let signal: CompanionConnectionStreamSignal = liveness.isLive
            ? .liveActivity(latestSeq: liveness.latestSeq)
            : .none
        apply(.streamUpdate(signal, streamGeneration: generation))
        callbacks?.applyLiveness(liveness)
    }

    private func handleStreamExit(generation: UInt64) {
        apply(.streamExited(streamGeneration: generation, isNetworkError: false))
        callbacks?.applyLiveness(CompanionSessionMiniController_restartLiveness())
        CompanionDiagnostics.record("session-mini:sync-restarting")
    }

    private static func backoffDelay(attempt: Int) -> Duration {
        let doubled = Backoff.floorMilliseconds << min(attempt, 5)
        let capped = min(doubled, Backoff.ceilingMilliseconds)
        // Half-to-full jitter keeps reconnect herds from synchronizing.
        return .milliseconds(Int64.random(in: (capped / 2)...capped))
    }

    // MARK: - Refresh

    enum RefreshTrigger: String {
        case pullGesture = "pull-gesture"
        case manual = "manual"
        case foreground = "foreground"
        case watchdog = "watchdog"
        case sessionOpen = "session-open"
    }

    /// Never blocks on the network: returns immediately when live and fresh,
    /// otherwise ensures the supervisor is running and kicks ONE background
    /// recovery whose result lands through `recoverySnapshotArrived`.
    func requestRefresh(_ trigger: RefreshTrigger) {
        if CompanionConnectionReducer.shouldSkipRefreshRecovery(machine) {
            CompanionDiagnostics.record("refresh:skip-live-fresh trigger=\(trigger.rawValue)")
            return
        }

        startSyncIfNeeded()

        guard refreshRecoveryTask == nil else {
            return
        }
        guard let sessionRuntime else {
            return
        }

        // Local-first: surface anything already in the on-disk store (other
        // processes write it too) before going near the network.
        _ = callbacks?.replayLocalStore("refresh-\(trigger.rawValue)")

        CompanionDiagnostics.record("refresh:background-recovery trigger=\(trigger.rawValue)")
        refreshRecoveryTask = Task { [weak self] in
            defer {
                Task { @MainActor [weak self] in
                    self?.refreshRecoveryTask = nil
                }
            }
            do {
                guard let recovered = try await sessionRuntime.recoverStateMiniSnapshot() else {
                    await self?.applyRecoveryOutcome(nil)
                    return
                }
                await self?.applyRecoveryOutcome(recovered)
            } catch {
                CompanionDiagnostics.record(
                    "refresh:recovery-failed trigger=\(trigger.rawValue) error=\(error.localizedDescription)"
                )
                await self?.applyRecoveryOutcome(nil)
            }
        }
    }

    private func applyRecoveryOutcome(_ recovered: CompanionRecoveredSessionMiniSnapshot?) {
        guard let recovered else {
            apply(.recoveryFailed)
            return
        }
        apply(.recoveryCompleted(latestSeq: recovered.latestSeq))
        callbacks?.recoverySnapshotArrived(recovered)
    }

    // MARK: - Testing seams

    /// Drives the machine to live+fresh without a running stream, so tests
    /// can exercise refresh-skip semantics deterministically.
    func simulateLiveActivityForTesting(latestSeq: Int64) {
        apply(.streamUpdate(
            .liveActivity(latestSeq: latestSeq),
            streamGeneration: streamGeneration
        ))
    }

    /// Awaits the in-flight background refresh recovery, if any.
    func waitForRefreshRecoveryForTesting() async {
        await refreshRecoveryTask?.value
    }

    /// Sets the activation timestamp directly, bypassing the watchdog task
    /// lifecycle, so tests can exercise the freshness-clamp deterministically
    /// without spinning up the real activity loop.
    func setLastBecameActiveAtForTesting(_ date: Date?) {
        lastBecameActiveAt = date
    }

    // MARK: - Foreground watchdog

    /// Periodic freshness check while the app is active. Replaces the
    /// RootTabView fallback-timer loop; each tick is a no-op when the stream
    /// is live and the seq is current.
    func setAppActive(_ isActive: Bool) {
        guard isActive else {
            watchdogTask?.cancel()
            watchdogTask = nil
            return
        }
        guard watchdogTask == nil else {
            return
        }
        // Only stamp the activation edge (inactive -> active), not every
        // repeated `true` call while already active.
        lastBecameActiveAt = Date()
        watchdogTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: CompanionMetrics.autoRefreshInterval)
                guard !Task.isCancelled else {
                    return
                }
                self?.requestRefresh(.watchdog)
            }
        }
    }

    // MARK: - Network path

    func startNetworkPathMonitoringIfNeeded() {
        guard networkPathMonitor == nil else {
            return
        }

        let monitor = NWPathMonitor()
        networkPathMonitor = monitor
        monitor.pathUpdateHandler = { [weak self] path in
            let identity = Self.networkPathIdentity(path)
            Task { @MainActor [weak self] in
                self?.handleNetworkPathChange(identity: identity)
            }
        }
        monitor.start(queue: DispatchQueue(label: "companion.network-path-monitor"))
    }

    private func handleNetworkPathChange(identity: String) {
        guard let previousIdentity = lastNetworkPathIdentity else {
            lastNetworkPathIdentity = identity
            return
        }
        guard identity != previousIdentity else {
            return
        }

        lastNetworkPathIdentity = identity
        CompanionDiagnostics.record("network:path-changed identity=\(identity)")
        restartForRouteChange()
    }

    private nonisolated static func networkPathIdentity(_ path: NWPath) -> String {
        let interfaces = path.availableInterfaces
            .map { "\($0.type)" }
            .sorted()
            .joined(separator: ",")
        return "\(path.status):\(interfaces)"
    }

    // MARK: - Cached state passthroughs

    @discardableResult
    func restoreCachedSnapshotIfAvailable(
        reason: String,
        applySnapshot: @MainActor (MobileSnapshot, String, Int64) -> Bool
    ) -> Bool {
        guard let sessionRuntime else {
            CompanionDiagnostics.record("session-mini:cache-restore-unavailable reason=\(reason)")
            return false
        }

        do {
            let localSnapshot = try sessionRuntime.currentStateMiniSnapshot()
            guard let cachedSnapshot = try sessionRuntime.cachedSnapshot() else {
                return false
            }

            let didApplySnapshot = applySnapshot(
                cachedSnapshot,
                "session-mini-\(reason)",
                localSnapshot.latestSeq
            )
            guard didApplySnapshot else {
                CompanionDiagnostics.record(
                    "session-mini:cache-restore-stale-skip reason=\(reason) seq=\(localSnapshot.latestSeq)"
                )
                return false
            }
            apply(.cachedSnapshotRestored)
            CompanionDiagnostics.record(
                "session-mini:cache-restore reason=\(reason) sessions=\(cachedSnapshot.sessions.count)"
            )
            return true
        } catch {
            CompanionDiagnostics.record(
                "session-mini:cache-restore-failed reason=\(reason) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    func hasLocalStateMiniEvidence(reason: String) -> Bool {
        guard let sessionRuntime else {
            return false
        }

        do {
            let snapshot = try sessionRuntime.currentStateMiniSnapshot()
            let hasEvidence = snapshot.latestSeq > 0
                || !snapshot.sessions.isEmpty
                || !snapshot.pendingCommands.isEmpty
            if hasEvidence {
                CompanionDiagnostics.record(
                    "session-mini:local-evidence reason=\(reason) seq=\(snapshot.latestSeq) sessions=\(snapshot.sessions.count)"
                )
            }
            return hasEvidence
        } catch {
            CompanionDiagnostics.record(
                "session-mini:local-evidence-failed reason=\(reason) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        guard let sessionRuntime else {
            throw HTTPCompanionServiceError.localStoreUnavailable
        }
        return try sessionRuntime.cachedSnapshot()
    }

    // MARK: - Notification reply outbox

    @discardableResult
    func startNotificationReplyOutboxDrainIfNeeded(
        submit: @escaping @MainActor @Sendable () async -> Bool
    ) -> Task<Bool, Never>? {
        guard notificationReplyOutboxDrainTask == nil else {
            return notificationReplyOutboxDrainTask
        }

        let drainTask = Task { @MainActor [weak self] in
            guard let self else {
                return false
            }
            defer {
                self.notificationReplyOutboxDrainTask = nil
            }
            if !Task.isCancelled {
                return await submit()
            }
            return false
        }
        notificationReplyOutboxDrainTask = drainTask
        return drainTask
    }

    func stopNotificationReplyOutboxDrain() {
        notificationReplyOutboxDrainTask?.cancel()
        notificationReplyOutboxDrainTask = nil
    }
}

// Legacy synthetic liveness updates kept during migration so the model's
// existing liveness pipeline keeps rendering reconnect states; A8 replaces
// these consumers with the machine's phase directly.
private func CompanionSessionMiniController_restartLiveness() -> CompanionSessionMiniLivenessUpdate {
    CompanionSessionMiniLivenessUpdate(
        reason: "runtime-restart",
        latestSeq: 0,
        serverTime: "",
        isLive: false,
        endpointURL: nil
    )
}

private func CompanionSessionMiniController_runtimeUnavailableLiveness() -> CompanionSessionMiniLivenessUpdate {
    CompanionSessionMiniLivenessUpdate(
        reason: "runtime-unavailable",
        latestSeq: 0,
        serverTime: "",
        isLive: false,
        endpointURL: nil
    )
}

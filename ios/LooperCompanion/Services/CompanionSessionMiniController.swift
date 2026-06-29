import Foundation

private enum CompanionSessionMiniControllerRetry {
    static let delay: Duration = .milliseconds(750)
    static let restartReason = "runtime-restart"
    static let restartLatestSeq: Int64 = 0
    static let restartServerTime = ""
}

@MainActor
final class CompanionSessionMiniController {
    typealias SyncUpdateHandler = @MainActor @Sendable (
        CompanionSessionMiniSyncUpdate,
        Int
    ) -> Void
    typealias LivenessUpdateHandler = @MainActor @Sendable (
        CompanionSessionMiniLivenessUpdate,
        Int
    ) -> Void
    typealias SnapshotApplyHandler = @MainActor (MobileSnapshot, String, Int64) -> Bool
    typealias NotificationReplySubmitter = @MainActor @Sendable () async -> Bool

    let sessionRuntime: CompanionSessionRuntime?

    private var syncTask: Task<Void, Never>?
    private var notificationReplyOutboxDrainTask: Task<Bool, Never>?

    var isSyncing: Bool {
        syncTask != nil
    }

    init(sessionRuntime: CompanionSessionRuntime?) {
        self.sessionRuntime = sessionRuntime
    }

    func startSyncIfNeeded(
        connectionRevision: Int,
        onUpdate: @escaping SyncUpdateHandler,
        onLiveness: @escaping LivenessUpdateHandler
    ) {
        guard syncTask == nil, let sessionRuntime else {
            return
        }

        syncTask = Task { @MainActor [weak self] in
            defer {
                self?.syncTask = nil
            }
            while !Task.isCancelled {
                await sessionRuntime.prepareSessionRuntime()
                await sessionRuntime.runStateMiniSync(
                    onUpdate: { update in
                        onUpdate(update, connectionRevision)
                    },
                    onLiveness: { liveness in
                        onLiveness(liveness, connectionRevision)
                    },
                    onDebugMessage: { message in
                        CompanionDiagnostics.record(message)
                    }
                )

                guard !Task.isCancelled else {
                    return
                }

                onLiveness(Self.restartLivenessUpdate(), connectionRevision)
                CompanionDiagnostics.record("session-mini:sync-restarting")
                try? await Task.sleep(for: CompanionSessionMiniControllerRetry.delay)
            }
        }
    }

    static func restartLivenessUpdate() -> CompanionSessionMiniLivenessUpdate {
        CompanionSessionMiniLivenessUpdate(
            reason: CompanionSessionMiniControllerRetry.restartReason,
            latestSeq: CompanionSessionMiniControllerRetry.restartLatestSeq,
            serverTime: CompanionSessionMiniControllerRetry.restartServerTime,
            isLive: false,
            endpointURL: nil
        )
    }

    func stopSync() {
        let task = syncTask
        syncTask = nil
        task?.cancel()
        sessionRuntime?.stopStateMiniStream()
    }

    func stopSyncAndWait() async {
        let task = syncTask
        syncTask = nil
        task?.cancel()
        sessionRuntime?.stopStateMiniStream()
        await task?.value
    }

    @discardableResult
    func restoreCachedSnapshotIfAvailable(
        reason: String,
        applySnapshot: SnapshotApplyHandler
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

    func cachedSnapshot() throws -> MobileSnapshot? {
        guard let sessionRuntime else {
            throw HTTPCompanionServiceError.localStoreUnavailable
        }
        return try sessionRuntime.cachedSnapshot()
    }

    @discardableResult
    func startNotificationReplyOutboxDrainIfNeeded(
        submit: @escaping NotificationReplySubmitter
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

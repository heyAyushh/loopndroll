import Foundation

@MainActor
final class CompanionSessionMiniController {
    typealias SyncUpdateHandler = @MainActor @Sendable (
        CompanionSessionMiniSyncUpdate,
        Int
    ) -> Void
    typealias SnapshotApplyHandler = @MainActor (MobileSnapshot, String) -> Void
    typealias NotificationReplySubmitter = @MainActor @Sendable () async -> Bool

    let localStore: CompanionSessionMiniLocalStore?

    private var syncTask: Task<Void, Never>?
    private var notificationReplyOutboxDrainTask: Task<Void, Never>?
    private var notificationReplyOutboxDrainID: String?

    var isSyncing: Bool {
        syncTask != nil
    }

    init(localStore: CompanionSessionMiniLocalStore?) {
        self.localStore = localStore
    }

    func startSyncIfNeeded(
        service: any CompanionService,
        connectionRevision: Int,
        onUpdate: @escaping SyncUpdateHandler
    ) {
        guard syncTask == nil, let localStore else {
            return
        }

        syncTask = Task { @MainActor in
            await service.prepareRealtimeConnection()
            await localStore.runClientCoreStateMiniSync(
                onUpdate: { update in
                    onUpdate(update, connectionRevision)
                },
                onDebugMessage: { message in
                    CompanionDiagnostics.record(message)
                }
            )
        }
    }

    func stopSync() {
        syncTask?.cancel()
        syncTask = nil
    }

    @discardableResult
    func restoreCachedSnapshotIfAvailable(
        reason: String,
        applySnapshot: SnapshotApplyHandler
    ) -> Bool {
        guard let localStore else {
            return false
        }

        do {
            guard let cachedSnapshot = try localStore.cachedSnapshot() else {
                return false
            }

            applySnapshot(cachedSnapshot, "session-mini-\(reason)")
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
        try localStore?.cachedSnapshot()
    }

    @discardableResult
    func startNotificationReplyOutboxDrainIfNeeded(
        drainID: String,
        submit: @escaping NotificationReplySubmitter
    ) -> Task<Void, Never>? {
        guard notificationReplyOutboxDrainTask == nil else {
            return notificationReplyOutboxDrainTask
        }

        let drainTask = Task { @MainActor [weak self] in
            guard let self else {
                return
            }
            await self.drainNotificationReplyOutbox(
                drainID: drainID,
                submit: submit
            )
        }
        notificationReplyOutboxDrainTask = drainTask
        notificationReplyOutboxDrainID = drainID
        return drainTask
    }

    func stopNotificationReplyOutboxDrain() {
        notificationReplyOutboxDrainTask?.cancel()
        notificationReplyOutboxDrainTask = nil
        notificationReplyOutboxDrainID = nil
    }

    private func drainNotificationReplyOutbox(
        drainID: String,
        submit: NotificationReplySubmitter
    ) async {
        if !Task.isCancelled {
            _ = await submit()
        }
        finishNotificationReplyOutboxDrain(drainID: drainID)
    }

    private func finishNotificationReplyOutboxDrain(drainID: String) {
        guard notificationReplyOutboxDrainID == drainID else {
            CompanionDiagnostics.record("notification-reply:stale-drain-finish-skip")
            return
        }

        notificationReplyOutboxDrainTask = nil
        notificationReplyOutboxDrainID = nil
    }
}

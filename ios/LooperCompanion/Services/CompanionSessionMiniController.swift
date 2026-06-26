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
            await service.prepareSessionRuntime()
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
        submit: @escaping NotificationReplySubmitter
    ) -> Task<Void, Never>? {
        guard notificationReplyOutboxDrainTask == nil else {
            return notificationReplyOutboxDrainTask
        }

        let drainTask = Task { @MainActor [weak self] in
            guard let self else {
                return
            }
            defer {
                self.notificationReplyOutboxDrainTask = nil
            }
            if !Task.isCancelled {
                _ = await submit()
            }
        }
        notificationReplyOutboxDrainTask = drainTask
        return drainTask
    }

    func stopNotificationReplyOutboxDrain() {
        notificationReplyOutboxDrainTask?.cancel()
        notificationReplyOutboxDrainTask = nil
    }
}

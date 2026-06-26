import Foundation

@MainActor
protocol CompanionSessionCommandLocalStore: AnyObject {
    func enqueueNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        clientMutationID: String
    )
    func markCommandDelivered(_ clientMutationID: String?)
}

@MainActor
final class CompanionSessionMiniController: CompanionSessionCommandLocalStore {
    typealias SyncUpdateHandler = @MainActor @Sendable (
        CompanionSessionMiniSyncUpdate,
        Int
    ) -> Void
    typealias SnapshotApplyHandler = @MainActor (MobileSnapshot, String) -> Void
    typealias NotificationReplySubmitter = @MainActor @Sendable (
        CompanionSessionMiniPendingCommand
    ) async -> Bool

    let localStore: CompanionSessionMiniLocalStore?

    private var syncTask: Task<Void, Never>?
    private var notificationReplyOutboxDrainTask: Task<Void, Never>?
    private var notificationReplyOutboxDrainID: String?
    private var notificationReplyOutboxRetryTask: Task<Void, Never>?
    private var notificationReplyOutboxRetryDelayNanoseconds =
        NotificationReplyOutboxRetry.initialDelayNanoseconds

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
            guard let transport = await service.makeClientCoreStateMiniStreamTransport() else {
                CompanionDiagnostics.record("session-mini:client-core-stream-unavailable")
                return
            }
            await localStore.runClientCoreStateMiniSync(
                using: transport,
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

    func pendingNotificationReplyCommand() -> CompanionSessionMiniPendingCommand? {
        localStore?.pendingCommands().first { command in
            command.kind == .submitNotificationReply
        }
    }

    @discardableResult
    func startNotificationReplyOutboxDrainIfNeeded(
        drainID: String,
        submit: @escaping NotificationReplySubmitter
    ) -> Task<Void, Never>? {
        guard notificationReplyOutboxDrainTask == nil,
              pendingNotificationReplyCommand() != nil
        else {
            return notificationReplyOutboxDrainTask
        }

        cancelNotificationReplyOutboxRetry()
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
        cancelNotificationReplyOutboxRetry()
    }

    func scheduleNotificationReplyOutboxRetryIfNeeded(
        drainID: String,
        submit: @escaping NotificationReplySubmitter
    ) {
        guard notificationReplyOutboxRetryTask == nil,
              pendingNotificationReplyCommand() != nil
        else {
            return
        }

        let delayNanoseconds = notificationReplyOutboxRetryDelayNanoseconds
        notificationReplyOutboxRetryDelayNanoseconds = min(
            delayNanoseconds * NotificationReplyOutboxRetry.backoffMultiplier,
            NotificationReplyOutboxRetry.maximumDelayNanoseconds
        )
        CompanionDiagnostics.record(
            "notification-reply:retry-scheduled delayNanoseconds=\(delayNanoseconds)"
        )
        notificationReplyOutboxRetryTask = Task { @MainActor [weak self] in
            do {
                try await Task.sleep(nanoseconds: delayNanoseconds)
            } catch {
                return
            }

            guard !Task.isCancelled else {
                return
            }

            self?.notificationReplyOutboxRetryTask = nil
            self?.startNotificationReplyOutboxDrainIfNeeded(
                drainID: drainID,
                submit: submit
            )
        }
    }

    func resetNotificationReplyOutboxRetry() {
        cancelNotificationReplyOutboxRetry()
        notificationReplyOutboxRetryDelayNanoseconds =
            NotificationReplyOutboxRetry.initialDelayNanoseconds
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        clientMutationID: String
    ) {
        do {
            try localStore?.enqueueNotificationReplyCommand(
                notificationID: notificationID,
                threadID: sessionID,
                prompt: prompt,
                assistantSurface: nil,
                clientMutationID: clientMutationID
            )
        } catch {
            CompanionDiagnostics.record(
                "session-mini:notification-reply-outbox-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
            )
        }
    }

    func markCommandDelivered(_ clientMutationID: String?) {
        guard let trimmedClientMutationID = clientMutationID?
            .trimmingCharacters(in: .whitespacesAndNewlines),
            !trimmedClientMutationID.isEmpty
        else {
            return
        }

        do {
            try localStore?.markDelivered(clientMutationID: trimmedClientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:outbox-delivery-mark-failed id=\(trimmedClientMutationID) error=\(error.localizedDescription)"
            )
        }
    }

    private func drainNotificationReplyOutbox(
        drainID: String,
        submit: NotificationReplySubmitter
    ) async {
        while !Task.isCancelled {
            guard let command = pendingNotificationReplyCommand() else {
                break
            }
            let didSend = await submit(command)
            if !didSend {
                break
            }
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
        if pendingNotificationReplyCommand() == nil {
            resetNotificationReplyOutboxRetry()
        }
    }

    private func cancelNotificationReplyOutboxRetry() {
        notificationReplyOutboxRetryTask?.cancel()
        notificationReplyOutboxRetryTask = nil
    }
}

private enum NotificationReplyOutboxRetry {
    static let initialDelayNanoseconds: UInt64 = 250_000_000
    static let maximumDelayNanoseconds: UInt64 = 30_000_000_000
    static let backoffMultiplier: UInt64 = 2
}

import Foundation
import LooperRealtime

@MainActor
final class CompanionSessionMiniController {
    typealias SyncUpdateHandler = @MainActor @Sendable (
        LooperRealtimeStateMiniSyncUpdate,
        Int
    ) -> Void
    typealias SnapshotApplyHandler = @MainActor (MobileSnapshot, String) -> Void

    let localStore: CompanionSessionMiniLocalStore?

    private var syncTask: Task<Void, Never>?

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

        let synchronizer = LooperRealtimeStateMiniSynchronizer(
            store: localStore.realtimeLocalStore,
            transport: DeferredCompanionStateMiniSyncTransport(service: service)
        )

        syncTask = Task { @MainActor in
            await synchronizer.runUntilCancelled { update in
                await onUpdate(update, connectionRevision)
            }
        }
    }

    func stopSync() {
        syncTask?.cancel()
        syncTask = nil
    }

    #if DEBUG
    func runSyncCycleForSelfTest(
        transport: any LooperRealtimeStateMiniSyncTransport,
        connectionRevision: Int,
        onUpdate: @escaping SyncUpdateHandler
    ) async -> LooperRealtimeStateMiniSyncCycleResult {
        guard let localStore else {
            return .retry(
                latestSeq: 0,
                errorDescription: "session mini local store unavailable"
            )
        }

        let synchronizer = LooperRealtimeStateMiniSynchronizer(
            store: localStore.realtimeLocalStore,
            transport: transport
        )
        return await synchronizer.runOneCycle { update in
            await onUpdate(update, connectionRevision)
        }
    }
    #endif

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

    func enqueueModeCommand(
        sessionID: String,
        preset: SessionMode?,
        clientMutationID: String
    ) {
        do {
            try localStore?.enqueueModeCommand(
                threadID: sessionID,
                preset: preset,
                clientMutationID: clientMutationID
            )
            markCommandAttempted(clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:mode-outbox-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
        }
    }

    func enqueuePromptCommand(
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface,
        clientMutationID: String
    ) {
        do {
            try localStore?.enqueuePromptCommand(
                threadID: sessionID,
                prompt: prompt,
                assistantSurface: assistantSurface,
                clientMutationID: clientMutationID
            )
            markCommandAttempted(clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:prompt-outbox-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
        }
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
            markCommandAttempted(clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:notification-reply-outbox-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
            )
        }
    }

    func markCommandAttempted(_ clientMutationID: String) {
        do {
            try localStore?.markAttempted(clientMutationID: clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:outbox-attempt-mark-failed id=\(clientMutationID) error=\(error.localizedDescription)"
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
}

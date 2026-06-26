import Foundation
import LooperClientCore

struct CompanionSessionMiniPendingCommand: Equatable, Sendable {
    let kind: ClientPendingCommandKind
    let clientMutationID: String
    let threadID: String
    let notificationID: String?
    let prompt: String?
    let attemptCount: Int
}

struct CompanionClientCoreMobileSnapshotStreamResult: Sendable {
    let update: CompanionSessionMiniSyncUpdate?
    let shouldStop: Bool
    let debugMessage: String
}

struct CompanionSessionMiniSyncUpdate: Sendable {
    let reason: String
    let latestSeq: Int64
    let snapshot: MobileSnapshot
}

typealias CompanionSessionMiniSyncUpdateHandler = @MainActor @Sendable (
    CompanionSessionMiniSyncUpdate
) -> Void

typealias CompanionSessionMiniSyncDebugHandler = @MainActor @Sendable (String) -> Void

private final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    fileprivate unowned let sessionManager: LooperClientCoreSessionManager
    private let decoder = JSONDecoder()

    fileprivate init(sessionManager: LooperClientCoreSessionManager) {
        self.sessionManager = sessionManager
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        let localSnapshot = currentStateMiniSnapshot()
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    func pendingCommands() -> [CompanionSessionMiniPendingCommand] {
        (try? sessionManager.localSnapshot().pendingCommands.map(CompanionSessionMiniPendingCommand.init)) ?? []
    }

    fileprivate static func defaultFileURL() throws -> URL {
        try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        .appendingPathComponent(defaultFileName)
    }

    private func mobileSnapshot(
        latestSeq: Int64,
        sessions minis: [ClientStateMini],
        serverTime: String?
    ) throws -> MobileSnapshot? {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: latestSeq,
            sessions: minis,
            serverTime: serverTime ?? ""
        )
        guard projection.hasSnapshot else {
            return nil
        }

        return try decoder.decode(MobileSnapshot.self, from: Data(projection.snapshotJson.utf8))
    }

    fileprivate func mobileSnapshot(from snapshot: ClientLocalStateSnapshot) throws -> MobileSnapshot? {
        try mobileSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            serverTime: snapshot.serverTime
        )
    }

}

final class CompanionSessionRuntime: @unchecked Sendable {
    static let defaultFileName = CompanionSessionMiniLocalStore.defaultFileName

    private let localStore: CompanionSessionMiniLocalStore
    private let decoder = JSONDecoder()
    private let sessionManager: LooperClientCoreSessionManager

    init(fileURL: URL) throws {
        let sessionManager = try LooperClientCoreSessionManager(fileURL: fileURL)
        self.sessionManager = sessionManager
        self.localStore = CompanionSessionMiniLocalStore(sessionManager: sessionManager)
    }

    static func liveDefault() -> CompanionSessionRuntime? {
        do {
            return try CompanionSessionRuntime(fileURL: CompanionSessionMiniLocalStore.defaultFileURL())
        } catch {
            CompanionDiagnostics.record("session-runtime:unavailable error=\(error.localizedDescription)")
            return nil
        }
    }

    @discardableResult
    private func start(
        endpoints: [ClientEndpoint],
        bearerToken: String?,
        mobileSessionHeader: String?
    ) throws -> ClientStateSnapshot {
        try sessionManager.start(
            endpoints: endpoints,
            bearerToken: bearerToken ?? "",
            mobileSessionHeader: mobileSessionHeader ?? ""
        )
    }

    @discardableResult
    func startIfNeeded(
        bearerToken: String?,
        mobileSessionHeader: String?,
        preferredRealtimeEndpointURLs: () async throws -> [URL]
    ) async throws -> ClientStateSnapshot? {
        if (try? isConfigured()) == true {
            return nil
        }
        let endpoints = try await preferredRealtimeEndpointURLs().map {
            ClientEndpoint(url: $0.absoluteString, lastGood: false)
        }
        guard !endpoints.isEmpty else {
            throw CompanionSessionRuntimeError.noRealtimeEndpoint
        }
        return try start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    @discardableResult
    func stop() throws -> ClientStateSnapshot {
        try sessionManager.stop()
    }

    private func isConfigured() throws -> Bool {
        try sessionManager.isRuntimeConfigured()
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        try localStore.cachedSnapshot()
    }

    func currentStateMiniSnapshot() -> ClientLocalStateSnapshot {
        localStore.currentStateMiniSnapshot()
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) throws -> String {
        let queued = try sessionManager.persistNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
        return queued.clientMutationId
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) throws {
        _ = try sessionManager.persistNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
    }

    func pendingCommands() -> [CompanionSessionMiniPendingCommand] {
        localStore.pendingCommands()
    }

    func nextMobileSnapshotStreamResult()
        async throws -> CompanionClientCoreMobileSnapshotStreamResult
    {
        let streamUpdate = try await sessionManager.observeMobileSnapshotChange()
        guard streamUpdate.hasSnapshot else {
            return CompanionClientCoreMobileSnapshotStreamResult(
                update: nil,
                shouldStop: streamUpdate.shouldStop,
                debugMessage: streamUpdate.debugMessage
            )
        }

        let snapshot = try decoder.decode(
            MobileSnapshot.self,
            from: Data(streamUpdate.snapshotJson.utf8)
        )
        return CompanionClientCoreMobileSnapshotStreamResult(
            update: CompanionSessionMiniSyncUpdate(
                reason: streamUpdate.syncReason,
                latestSeq: streamUpdate.latestSeq,
                snapshot: snapshot
            ),
            shouldStop: streamUpdate.shouldStop,
            debugMessage: streamUpdate.debugMessage
        )
    }

    func runStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async {
        defer {
            stopStateMiniStream()
        }

        do {
            try await drainStateMiniSync(
                onUpdate: onUpdate,
                onDebugMessage: onDebugMessage
            )
        } catch {
            await onDebugMessage(
                "session-mini:client-core-stream-failed error=\(error.localizedDescription)"
            )
        }
    }

    func stopStateMiniStream() {
        do {
            _ = try stop()
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-stream-stop-failed error=\(error.localizedDescription)"
            )
        }
    }

    @discardableResult
    func setMode(
        threadID: String,
        preset: SessionMode?
    ) async throws -> ClientSessionModeIntentResult {
        try await sessionManager.setMode(
            threadID: threadID,
            preset: preset?.rawValue ?? ""
        )
    }

    @discardableResult
    func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientSessionPromptIntentResult {
        try await sessionManager.sendPrompt(
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult {
        try await sessionManager.submitNotificationReplyWithGeneratedMutation(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult {
        try await sessionManager.submitNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
    }

    @discardableResult
    func drainNotificationReplyOutbox() async throws -> ClientNotificationReplyIntentResult {
        try await sessionManager.drainNotificationReplyOutbox()
    }

    func outboxDepth() throws -> UInt32 {
        try sessionManager.outboxDepth()
    }

    private func drainStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async throws {
        while !Task.isCancelled {
            let result = try await nextMobileSnapshotStreamResult()
            if let update = result.update {
                await onUpdate(update)
            }
            if !result.debugMessage.isEmpty {
                await onDebugMessage(result.debugMessage)
            }
            if result.shouldStop {
                return
            }
        }
    }
}

enum CompanionSessionRuntimeError: LocalizedError {
    case noRealtimeEndpoint

    var errorDescription: String? {
        switch self {
        case .noRealtimeEndpoint:
            "No realtime endpoint available"
        }
    }
}

private extension CompanionSessionMiniPendingCommand {
    init(_ command: ClientPendingCommand) {
        self.init(
            kind: command.kind,
            clientMutationID: command.clientMutationId,
            threadID: command.threadId,
            notificationID: command.notificationId.nilIfEmpty,
            prompt: command.prompt.nilIfEmpty,
            attemptCount: Int(command.attemptCount)
        )
    }
}

extension CompanionSessionMiniLocalStore {
    func currentStateMiniSnapshot() -> ClientLocalStateSnapshot {
        do {
            return try sessionManager.localSnapshot()
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-snapshot-failed error=\(error.localizedDescription)"
            )
            return ClientLocalStateSnapshot(
                latestSeq: 0,
                sessions: [],
                pendingCommands: [],
                serverTime: ""
            )
        }
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

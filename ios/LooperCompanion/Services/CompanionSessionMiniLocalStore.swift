import Foundation
import LooperClientCore

struct CompanionSessionMiniRecord: Equatable, Sendable {
    let sessionID: String
    let assistantSurface: String
    let seq: Int64
    let revision: String
    let payloadJSON: String
}

struct CompanionSessionMiniPendingCommand: Equatable, Sendable {
    let kind: ClientPendingCommandKind
    let clientMutationID: String
    let threadID: String
    let notificationID: String?
    let prompt: String?
    let attemptCount: Int
}

struct CompanionQueuedModeSnapshot: Sendable {
    let clientMutationID: String
    let snapshot: MobileSnapshot
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

final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    fileprivate let sessionManager: LooperClientCoreSessionManager
    private let decoder = JSONDecoder()

    init(fileURL: URL) throws {
        self.sessionManager = try LooperClientCoreSessionManager(fileURL: fileURL)
    }

    static func liveDefault() -> CompanionSessionMiniLocalStore? {
        do {
            return try CompanionSessionMiniLocalStore(
                fileURL: defaultFileURL()
            )
        } catch {
            CompanionDiagnostics.record("session-mini:store-unavailable error=\(error.localizedDescription)")
            return nil
        }
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        let localSnapshot = currentStateMiniSnapshot()
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    @discardableResult
    func replace(with snapshot: ClientStateMiniSnapshot) throws -> MobileSnapshot? {
        let localSnapshot = try replaceStateMinis(with: snapshot)
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    @discardableResult
    func apply(_ delta: ClientStateMiniDelta) throws -> MobileSnapshot? {
        let localSnapshot = try applyStateMiniDelta(delta)
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    @discardableResult
    fileprivate func queueSetMode(
        threadID: String,
        preset: SessionMode?,
        clientMutationID: String
    ) throws -> MobileSnapshot? {
        let localSnapshot = try sessionManager.queueSetMode(
            threadID: threadID,
            preset: preset?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
        return try mobileSnapshot(from: localSnapshot)
    }

    @discardableResult
    fileprivate func queueSetModeWithGeneratedMutation(
        threadID: String,
        preset: SessionMode?
    ) throws -> CompanionQueuedModeSnapshot? {
        let queued = try sessionManager.queueSetMode(
            threadID: threadID,
            preset: preset?.rawValue ?? ""
        )
        guard let snapshot = try mobileSnapshot(from: queued.snapshot) else {
            return nil
        }
        return CompanionQueuedModeSnapshot(
            clientMutationID: queued.clientMutationId,
            snapshot: snapshot
        )
    }

    @discardableResult
    func replace(
        latestSeq: Int64,
        records: [CompanionSessionMiniRecord],
        serverTime: String? = nil
    ) throws -> MobileSnapshot? {
        try replace(
            with: ClientStateMiniSnapshot(
                latestSeq: latestSeq,
                sessions: records.map(ClientStateMini.init),
                serverTime: serverTime ?? ""
            )
        )
    }

    fileprivate func enqueueNotificationReplyCommand(
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

    fileprivate func enqueueNotificationReplyCommand(
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
        (try? sessionManager.localSnapshot().pendingCommands.map(CompanionSessionMiniPendingCommand.init)) ?? []
    }

    private static func defaultFileURL() throws -> URL {
        try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        .appendingPathComponent(defaultFileName)
    }

    @discardableResult
    private func persistValidated(_ snapshot: ClientStateSnapshot) throws
        -> ClientLocalStateSnapshot
    {
        let sessions = snapshot.stateMinis
        _ = try mobileSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: sessions,
            serverTime: snapshot.serverTime.nilIfEmpty
        )
        return try sessionManager.replaceStateMinis(
            snapshot: ClientStateMiniSnapshot(
                latestSeq: snapshot.latestSeq,
                sessions: sessions,
                serverTime: snapshot.serverTime
            )
        )
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

    private func mobileSnapshot(from snapshot: ClientLocalStateSnapshot) throws -> MobileSnapshot? {
        try mobileSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            serverTime: snapshot.serverTime
        )
    }

}

final class CompanionSessionRuntime: @unchecked Sendable {
    let localStore: CompanionSessionMiniLocalStore
    private let decoder = JSONDecoder()

    private var sessionManager: LooperClientCoreSessionManager {
        localStore.sessionManager
    }

    init(localStore: CompanionSessionMiniLocalStore) {
        self.localStore = localStore
    }

    @discardableResult
    func start(
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
    func stop() throws -> ClientStateSnapshot {
        try sessionManager.stop()
    }

    func isConfigured() throws -> Bool {
        try sessionManager.isRuntimeConfigured()
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        try localStore.cachedSnapshot()
    }

    func currentStateMiniSnapshot() -> ClientLocalStateSnapshot {
        localStore.currentStateMiniSnapshot()
    }

    @discardableResult
    func replaceStateMinis(with snapshot: ClientStateMiniSnapshot) throws -> MobileSnapshot? {
        try localStore.replace(with: snapshot)
    }

    @discardableResult
    func applyStateMiniDelta(_ delta: ClientStateMiniDelta) throws -> MobileSnapshot? {
        try localStore.apply(delta)
    }

    @discardableResult
    func queueSetMode(
        threadID: String,
        preset: SessionMode?,
        clientMutationID: String
    ) throws -> MobileSnapshot? {
        try localStore.queueSetMode(
            threadID: threadID,
            preset: preset,
            clientMutationID: clientMutationID
        )
    }

    @discardableResult
    func queueSetModeWithGeneratedMutation(
        threadID: String,
        preset: SessionMode?
    ) throws -> CompanionQueuedModeSnapshot? {
        try localStore.queueSetModeWithGeneratedMutation(
            threadID: threadID,
            preset: preset
        )
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) throws -> String {
        try localStore.enqueueNotificationReplyCommand(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) throws {
        try localStore.enqueueNotificationReplyCommand(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
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
    ) async throws -> ClientCommandAckEnvelope {
        try await sessionManager.setMode(
            threadID: threadID,
            preset: preset?.rawValue ?? ""
        )
    }

    @discardableResult
    func setMode(
        threadID: String,
        preset: SessionMode?,
        clientMutationID: String
    ) async throws -> ClientCommandAckEnvelope {
        try await sessionManager.setMode(
            threadID: threadID,
            preset: preset?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
    }

    @discardableResult
    func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientCommandAckEnvelope {
        try await sessionManager.sendPrompt(
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
    }

    @discardableResult
    func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientCommandAckEnvelope {
        try await sessionManager.sendPrompt(
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientCommandAckEnvelope {
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
    ) async throws -> ClientCommandAckEnvelope {
        try await sessionManager.submitNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
    }

    @discardableResult
    func drainNotificationReplyOutbox() async throws -> ClientCommandAckEnvelope {
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

private extension ClientStateMini {
    init(_ record: CompanionSessionMiniRecord) {
        self.init(
            sessionId: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJson: record.payloadJSON
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

    @discardableResult
    func replaceStateMinis(with snapshot: ClientStateMiniSnapshot) throws
        -> ClientLocalStateSnapshot
    {
        try sessionManager.replaceStateMinis(
            snapshot: snapshot
        )
    }

    @discardableResult
    func replaceStateMinis(with snapshot: ClientStateSnapshot) throws
        -> ClientLocalStateSnapshot
    {
        try persistValidated(snapshot)
    }

    @discardableResult
    func applyStateMiniDelta(_ delta: ClientStateMiniDelta) throws
        -> ClientLocalStateSnapshot
    {
        try sessionManager.applyStateMiniDelta(delta)
    }
}

private extension ClientStateMiniSnapshot {
    init(_ snapshot: ClientLocalStateSnapshot) {
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            serverTime: snapshot.serverTime
        )
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

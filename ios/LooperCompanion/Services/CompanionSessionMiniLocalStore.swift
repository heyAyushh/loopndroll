import Foundation
import LooperClientCore
import LooperRealtime

struct CompanionSessionMiniRecord: Equatable, Sendable {
    let sessionID: String
    let assistantSurface: String
    let seq: Int64
    let revision: String
    let payloadJSON: String
}

struct CompanionSessionMiniPendingCommand: Equatable, Sendable {
    let kind: LooperRealtimePendingCommand.Kind
    let clientMutationID: String
    let threadID: String
    let notificationID: String?
    let prompt: String?
    let attemptCount: Int
}

struct CompanionClientCoreStateMiniStreamResult: Sendable {
    let reason: ClientStateMiniStreamUpdateReason
    let update: LooperRealtimeStateMiniUpdate?
    let errorDescription: String
}

final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    private let store: LooperClientCoreLocalStore
    private let clientCore: LooperClientCore
    private let decoder = JSONDecoder()

    init(fileURL: URL) throws {
        store = try LooperClientCoreLocalStore(filePath: fileURL.path)
        clientCore = LooperClientCore()
        do {
            _ = try clientCore.replaceStateMinis(
                snapshot: ClientStateMiniSnapshot(store.snapshot())
            )
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-seed-failed error=\(error.localizedDescription)"
            )
        }
    }

    static func liveDefault() -> CompanionSessionMiniLocalStore? {
        do {
            return try CompanionSessionMiniLocalStore(fileURL: defaultFileURL())
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
    func replace(with snapshot: LooperRealtimeStateMiniSnapshot) throws -> MobileSnapshot? {
        let localSnapshot = try replaceStateMinis(with: snapshot)
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    @discardableResult
    func apply(_ delta: LooperRealtimeStateMiniDelta) throws -> MobileSnapshot? {
        let localSnapshot = try applyStateMiniDelta(delta)
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    @discardableResult
    func replace(
        latestSeq: Int64,
        records: [CompanionSessionMiniRecord],
        serverTime: String? = nil
    ) throws -> MobileSnapshot? {
        try replace(
            with: LooperRealtimeStateMiniSnapshot(
                latestSeq: latestSeq,
                sessions: records.map(LooperRealtimeStateMini.init),
                serverTime: serverTime
            )
        )
    }

    func enqueueModeCommand(
        threadID: String,
        preset: SessionMode?,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            command: ClientPendingCommand(
                kind: .setSessionMode,
                clientMutationId: clientMutationID,
                threadId: threadID,
                preset: preset?.rawValue ?? "",
                assistantSurface: "",
                prompt: "",
                notificationId: "",
                attemptCount: 0
            )
        )
    }

    func enqueuePromptCommand(
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            command: ClientPendingCommand(
                kind: .sendSessionPrompt,
                clientMutationId: clientMutationID,
                threadId: threadID,
                preset: "",
                assistantSurface: assistantSurface.rawValue,
                prompt: prompt,
                notificationId: "",
                attemptCount: 0
            )
        )
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            command: ClientPendingCommand(
                kind: .submitNotificationReply,
                clientMutationId: clientMutationID,
                threadId: threadID,
                preset: "",
                assistantSurface: assistantSurface?.rawValue ?? "",
                prompt: prompt,
                notificationId: notificationID,
                attemptCount: 0
            )
        )
    }

    func markAttempted(clientMutationID: String) throws {
        _ = try store.markAttempted(clientMutationId: clientMutationID)
    }

    func markDelivered(clientMutationID: String) throws {
        try store.markDelivered(clientMutationId: clientMutationID)
    }

    func pendingCommands() -> [CompanionSessionMiniPendingCommand] {
        (try? store.snapshot().pendingCommands.map(CompanionSessionMiniPendingCommand.init)) ?? []
    }

    func startClientCoreStateMiniStream(
        using transport: any LooperRealtimeClientCoreStateMiniStreamTransport
    ) async throws {
        _ = try clientCore.replaceStateMinis(snapshot: ClientStateMiniSnapshot(store.snapshot()))
        try await transport.startClientCoreStateMiniStream(clientCore: clientCore)
    }

    func nextClientCoreStateMiniStreamResult(
        using transport: any LooperRealtimeClientCoreStateMiniStreamTransport
    ) async throws -> CompanionClientCoreStateMiniStreamResult {
        let streamUpdate = try await transport.nextClientCoreStateMiniStreamUpdate(
            clientCore: clientCore
        )
        guard streamUpdate.reason == .delta, streamUpdate.didChange else {
            return CompanionClientCoreStateMiniStreamResult(
                reason: streamUpdate.reason,
                update: nil,
                errorDescription: streamUpdate.errorDescription
            )
        }

        let localSnapshot = try persistValidated(streamUpdate.snapshot)
        return CompanionClientCoreStateMiniStreamResult(
            reason: streamUpdate.reason,
            update: LooperRealtimeStateMiniUpdate(
                reason: .delta,
                snapshot: localSnapshot
            ),
            errorDescription: streamUpdate.errorDescription
        )
    }

    func recoverClientCoreStateMiniStream(
        using transport: any LooperRealtimeClientCoreStateMiniStreamTransport
    ) async throws -> LooperRealtimeLocalSnapshot {
        let snapshot = try await transport.recoverClientCoreStateMiniSnapshot(
            clientCore: clientCore
        )
        return try replaceStateMinis(with: snapshot)
    }

    func stopClientCoreStateMiniStream(
        using transport: any LooperRealtimeClientCoreStateMiniStreamTransport
    ) {
        do {
            try transport.stopClientCoreStateMiniStream(clientCore: clientCore)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-stream-stop-failed error=\(error.localizedDescription)"
            )
        }
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

    private func localSnapshot(from snapshot: ClientStateSnapshot) -> LooperRealtimeLocalSnapshot {
        let durableSnapshot = try? store.snapshot()
        return LooperRealtimeLocalSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.stateMinis.map(LooperRealtimeStateMini.init),
            pendingCommands: durableSnapshot?.pendingCommands.map(LooperRealtimePendingCommand.init) ?? [],
            serverTime: snapshot.serverTime.nilIfEmpty
        )
    }

    @discardableResult
    private func persistValidated(_ snapshot: ClientStateSnapshot) throws
        -> LooperRealtimeLocalSnapshot
    {
        let sessions = snapshot.stateMinis.map(LooperRealtimeStateMini.init)
        _ = try mobileSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: sessions,
            serverTime: snapshot.serverTime.nilIfEmpty
        )
        return LooperRealtimeLocalSnapshot(
            try store.replaceStateMinis(
                snapshot: ClientStateMiniSnapshot(
                    latestSeq: snapshot.latestSeq,
                    sessions: sessions.map(ClientStateMini.init),
                    serverTime: snapshot.serverTime
                )
            )
        )
    }

    private func localSnapshot(from snapshot: ClientLocalStateSnapshot) -> LooperRealtimeLocalSnapshot {
        LooperRealtimeLocalSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions.map(LooperRealtimeStateMini.init),
            pendingCommands: snapshot.pendingCommands.map(LooperRealtimePendingCommand.init),
            serverTime: snapshot.serverTime.nilIfEmpty
        )
    }

    private func mobileSnapshot(
        latestSeq: Int64,
        sessions minis: [LooperRealtimeStateMini],
        serverTime: String?
    ) throws -> MobileSnapshot? {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: latestSeq,
            sessions: minis.map(ClientStateMini.init),
            serverTime: serverTime ?? ""
        )
        guard projection.hasSnapshot else {
            return nil
        }

        return try decoder.decode(MobileSnapshot.self, from: Data(projection.snapshotJson.utf8))
    }
}

private extension CompanionSessionMiniPendingCommand {
    init(_ command: ClientPendingCommand) {
        self.init(
            kind: LooperRealtimePendingCommand.Kind(command.kind),
            clientMutationID: command.clientMutationId,
            threadID: command.threadId,
            notificationID: command.notificationId.nilIfEmpty,
            prompt: command.prompt.nilIfEmpty,
            attemptCount: Int(command.attemptCount)
        )
    }
}

private extension LooperRealtimePendingCommand {
    init(_ command: ClientPendingCommand) {
        self.init(
            kind: LooperRealtimePendingCommand.Kind(command.kind),
            clientMutationID: command.clientMutationId,
            threadID: command.threadId,
            preset: command.preset.nilIfEmpty,
            assistantSurface: command.assistantSurface.nilIfEmpty,
            prompt: command.prompt.nilIfEmpty,
            notificationID: command.notificationId.nilIfEmpty,
            attemptCount: Int(command.attemptCount)
        )
    }
}

private extension LooperRealtimePendingCommand.Kind {
    init(_ kind: ClientPendingCommandKind) {
        switch kind {
        case .setSessionMode:
            self = .setSessionMode
        case .sendSessionPrompt:
            self = .sendSessionPrompt
        case .submitNotificationReply:
            self = .submitNotificationReply
        }
    }
}

private extension LooperRealtimeStateMini {
    init(_ record: CompanionSessionMiniRecord) {
        self.init(
            sessionID: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJSON: record.payloadJSON
        )
    }

    init(_ mini: ClientStateMini) {
        self.init(
            sessionID: mini.sessionId,
            assistantSurface: mini.assistantSurface,
            seq: mini.seq,
            revision: mini.revision,
            payloadJSON: mini.payloadJson
        )
    }
}

extension CompanionSessionMiniLocalStore {
    func currentStateMiniSnapshot() -> LooperRealtimeLocalSnapshot {
        do {
            return try localSnapshot(from: clientCore.snapshot())
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-snapshot-failed error=\(error.localizedDescription)"
            )
            return (try? LooperRealtimeLocalSnapshot(store.snapshot()))
                ?? LooperRealtimeLocalSnapshot(
                    latestSeq: 0,
                    sessions: [],
                    pendingCommands: [],
                    serverTime: nil
                )
        }
    }

    @discardableResult
    func replaceStateMinis(with snapshot: LooperRealtimeStateMiniSnapshot) throws
        -> LooperRealtimeLocalSnapshot
    {
        let coreSnapshot = try clientCore.replaceStateMinis(
            snapshot: ClientStateMiniSnapshot(snapshot)
        )
        return try persistValidated(coreSnapshot)
    }

    @discardableResult
    func replaceStateMinis(with snapshot: ClientStateSnapshot) throws
        -> LooperRealtimeLocalSnapshot
    {
        try persistValidated(snapshot)
    }

    @discardableResult
    func applyStateMiniDelta(_ delta: LooperRealtimeStateMiniDelta) throws
        -> LooperRealtimeLocalSnapshot
    {
        let result = try clientCore.applyStateMiniDeltaWithResult(
            delta: ClientStateMiniDelta(delta)
        )
        guard result.didChange else {
            return localSnapshot(from: result.snapshot)
        }
        return try persistValidated(result.snapshot)
    }
}

private extension LooperRealtimeLocalSnapshot {
    init(_ snapshot: ClientLocalStateSnapshot) {
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions.map(LooperRealtimeStateMini.init),
            pendingCommands: snapshot.pendingCommands.map(LooperRealtimePendingCommand.init),
            serverTime: snapshot.serverTime.nilIfEmpty
        )
    }
}

private extension ClientStateMini {
    init(_ mini: LooperRealtimeStateMini) {
        self.init(
            sessionId: mini.sessionID,
            assistantSurface: mini.assistantSurface,
            seq: mini.seq,
            revision: mini.revision,
            payloadJson: mini.payloadJSON
        )
    }

    static let empty = ClientStateMini(
        sessionId: "",
        assistantSurface: "",
        seq: 0,
        revision: "",
        payloadJson: ""
    )
}

private extension ClientStateMiniSnapshot {
    init(_ snapshot: LooperRealtimeStateMiniSnapshot) {
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions.map(ClientStateMini.init),
            serverTime: snapshot.serverTime ?? ""
        )
    }

    init(_ snapshot: ClientLocalStateSnapshot) {
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            serverTime: snapshot.serverTime
        )
    }
}

private extension ClientStateMiniDelta {
    init(_ delta: LooperRealtimeStateMiniDelta) {
        let session = delta.session.map(ClientStateMini.init)
        self.init(
            seq: delta.seq,
            latestSeq: delta.latestSeq,
            entityId: delta.entityID,
            kind: delta.kind,
            revision: delta.revision,
            serverTime: delta.serverTime ?? "",
            hasSession: session != nil,
            session: session ?? .empty,
            sessions: session == nil ? delta.sessions.map(ClientStateMini.init) : []
        )
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

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

struct CompanionClientCoreStateMiniStreamResult: Sendable {
    let reason: ClientStateMiniStreamUpdateReason
    let update: CompanionSessionMiniSyncUpdate?
    let errorDescription: String
}

enum CompanionSessionMiniSyncUpdateReason: String, Equatable, Sendable {
    case delta
    case recovery
}

struct CompanionSessionMiniSyncUpdate: Equatable, Sendable {
    let reason: CompanionSessionMiniSyncUpdateReason
    let snapshot: ClientLocalStateSnapshot
}

typealias CompanionSessionMiniSyncUpdateHandler = @MainActor @Sendable (
    CompanionSessionMiniSyncUpdate
) -> Void

typealias CompanionSessionMiniSyncDebugHandler = @MainActor @Sendable (String) -> Void

final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    let clientCoreLocalStore: LooperClientCoreLocalStore
    let clientCore: LooperClientCore
    let sessionManager: LooperClientCoreSessionManager
    private let decoder = JSONDecoder()

    init(
        fileURL: URL,
        clientCore: LooperClientCore = LooperClientCore()
    ) throws {
        let clientCoreLocalStore = try LooperClientCoreLocalStore(filePath: fileURL.path)
        self.clientCoreLocalStore = clientCoreLocalStore
        self.clientCore = clientCore
        self.sessionManager = LooperClientCoreSessionManager(
            clientCore: clientCore,
            localStore: clientCoreLocalStore
        )
        do {
            _ = try sessionManager.replaceStateMinis(
                snapshot: ClientStateMiniSnapshot(clientCoreLocalStore.snapshot())
            )
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-seed-failed error=\(error.localizedDescription)"
            )
        }
    }

    static func liveDefault(
        clientCore: LooperClientCore = LooperClientCore()
    ) -> CompanionSessionMiniLocalStore? {
        do {
            return try CompanionSessionMiniLocalStore(
                fileURL: defaultFileURL(),
                clientCore: clientCore
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
        (try? clientCoreLocalStore.snapshot().pendingCommands.map(CompanionSessionMiniPendingCommand.init)) ?? []
    }

    func startClientCoreStateMiniStream() async throws {
        _ = try sessionManager.replaceStateMinis(
            snapshot: ClientStateMiniSnapshot(clientCoreLocalStore.snapshot())
        )
        _ = try sessionManager.startConfiguredStateMiniStream()
    }

    func nextClientCoreStateMiniStreamResult() async throws -> CompanionClientCoreStateMiniStreamResult {
        let streamUpdate = try await sessionManager.observe()
        guard
            (streamUpdate.reason == .delta || streamUpdate.reason == .recoveryRequired),
            streamUpdate.didChange
        else {
            return CompanionClientCoreStateMiniStreamResult(
                reason: streamUpdate.reason,
                update: nil,
                errorDescription: streamUpdate.errorDescription
            )
        }

        let localSnapshot = try persistValidated(streamUpdate.snapshot)
        return CompanionClientCoreStateMiniStreamResult(
            reason: streamUpdate.reason,
            update: CompanionSessionMiniSyncUpdate(
                reason: streamUpdate.reason == .recoveryRequired ? .recovery : .delta,
                snapshot: localSnapshot
            ),
            errorDescription: streamUpdate.errorDescription
        )
    }

    func stopClientCoreStateMiniStream() {
        do {
            _ = try sessionManager.stopStateMiniStream()
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-stream-stop-failed error=\(error.localizedDescription)"
            )
        }
    }

    func runClientCoreStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async {
        defer {
            stopClientCoreStateMiniStream()
        }

        do {
            try await startClientCoreStateMiniStream()
            try await drainClientCoreStateMiniSync(
                onUpdate: onUpdate,
                onDebugMessage: onDebugMessage
            )
        } catch {
            await onDebugMessage(
                "session-mini:client-core-stream-failed error=\(error.localizedDescription)"
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

    private func localSnapshot(from snapshot: ClientStateSnapshot) -> ClientLocalStateSnapshot {
        let durableSnapshot = try? clientCoreLocalStore.snapshot()
        return ClientLocalStateSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.stateMinis,
            pendingCommands: durableSnapshot?.pendingCommands ?? [],
            serverTime: snapshot.serverTime
        )
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
        return try clientCoreLocalStore.replaceStateMinis(
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

    private func drainClientCoreStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async throws {
        while !Task.isCancelled {
            let result = try await nextClientCoreStateMiniStreamResult()
            switch result.reason {
            case .delta:
                if let update = result.update {
                    await onUpdate(update)
                }
            case .heartbeat, .reconnecting:
                continue
            case .recoveryRequired:
                if let update = result.update {
                    await onUpdate(update)
                } else if !result.errorDescription.isEmpty {
                    await onDebugMessage(
                        "session-mini:client-core-stream-recovery-waiting error=\(result.errorDescription)"
                    )
                }
            case .stopped:
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
            return try localSnapshot(from: clientCore.snapshot())
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-snapshot-failed error=\(error.localizedDescription)"
            )
            return (try? clientCoreLocalStore.snapshot())
                ?? ClientLocalStateSnapshot(
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
        let coreSnapshot = try clientCore.replaceStateMinis(
            snapshot: snapshot
        )
        return try persistValidated(coreSnapshot)
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
        let result = try clientCore.applyStateMiniDeltaWithResult(
            delta: delta
        )
        guard result.didChange else {
            return localSnapshot(from: result.snapshot)
        }
        return try persistValidated(result.snapshot)
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

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

enum CompanionSessionMiniLocalStoreError: Error, Equatable, Sendable {
    case sessionIDMismatch(expected: String, actual: String)
}

final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = LooperRealtimeLocalStore.defaultFileName

    private enum Defaults {
        static let hostID = "local-session-mini-cache"
        static let hostName = "Looper"
        static let globalScope = "global"
        static let defaultPrompt = "Continue"
        static let revisionPrefix = "mini:"
    }

    private let store: LooperRealtimeLocalStore
    private let clientCore: LooperClientCore
    private let decoder = JSONDecoder()

    init(fileURL: URL) throws {
        store = try LooperRealtimeLocalStore(recovering: fileURL)
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
            LooperRealtimePendingCommand(
                kind: .setSessionMode,
                clientMutationID: clientMutationID,
                threadID: threadID,
                preset: preset?.rawValue
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
            LooperRealtimePendingCommand(
                kind: .sendSessionPrompt,
                clientMutationID: clientMutationID,
                threadID: threadID,
                assistantSurface: assistantSurface.rawValue,
                prompt: prompt
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
            LooperRealtimePendingCommand(
                kind: .submitNotificationReply,
                clientMutationID: clientMutationID,
                threadID: threadID,
                assistantSurface: assistantSurface?.rawValue,
                prompt: prompt,
                notificationID: notificationID
            )
        )
    }

    func markAttempted(clientMutationID: String) throws {
        _ = try store.markAttempted(clientMutationID: clientMutationID)
    }

    func markDelivered(clientMutationID: String) throws {
        try store.markDelivered(clientMutationID: clientMutationID)
    }

    func pendingCommands() -> [CompanionSessionMiniPendingCommand] {
        store.snapshot().pendingCommands.map {
            CompanionSessionMiniPendingCommand(
                kind: $0.kind,
                clientMutationID: $0.clientMutationID,
                threadID: $0.threadID,
                notificationID: $0.notificationID,
                prompt: $0.prompt,
                attemptCount: $0.attemptCount
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
        let durableSnapshot = store.snapshot()
        return LooperRealtimeLocalSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.stateMinis.map(LooperRealtimeStateMini.init),
            pendingCommands: durableSnapshot.pendingCommands,
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
        return try store.replace(
            with: LooperRealtimeStateMiniSnapshot(
                latestSeq: snapshot.latestSeq,
                sessions: sessions,
                serverTime: snapshot.serverTime.nilIfEmpty
            )
        )
    }

    private func mobileSnapshot(
        latestSeq: Int64,
        sessions minis: [LooperRealtimeStateMini],
        serverTime: String?
    ) throws -> MobileSnapshot? {
        guard !minis.isEmpty else {
            return nil
        }

        let sessionsBySurface = try minis.reduce(
            into: [String: [SessionSummary]]()
        ) { partialResult, mini in
            let session = try decodeSessionSummary(from: mini)
            partialResult[mini.assistantSurface, default: []].append(session)
        }
        let selectedSurface = selectedSurface(from: minis)
        let visibleSessions = sessionsBySurface[selectedSurface.rawValue] ?? []

        return MobileSnapshot(
            revision: revision(latestSeq: latestSeq, minis: minis),
            host: HostSummary(
                id: Defaults.hostID,
                name: Defaults.hostName,
                address: "",
                isReachable: false,
                lastSyncedAt: serverTime ?? ""
            ),
            globalSettings: GlobalSettings(
                defaultPrompt: Defaults.defaultPrompt,
                globalMode: nil,
                scope: Defaults.globalScope,
                notificationLabel: nil,
                completionCheckLabel: nil,
                completionCheckWaitForReply: false,
                assistantSurface: selectedSurface
            ),
            sessions: visibleSessions.sorted(by: SessionSummary.isNewerOrLowerRef),
            surfaceSessions: sessionsBySurface.mapValues {
                $0.sorted(by: SessionSummary.isNewerOrLowerRef)
            },
            notifications: [],
            completionChecks: []
        )
    }

    private func decodeSessionSummary(from mini: LooperRealtimeStateMini) throws -> SessionSummary {
        let data = Data(mini.payloadJSON.utf8)
        let session = try decoder.decode(SessionSummary.self, from: data)
        guard session.id == mini.sessionID else {
            throw CompanionSessionMiniLocalStoreError.sessionIDMismatch(
                expected: mini.sessionID,
                actual: session.id
            )
        }
        return session
    }

    private func selectedSurface(from minis: [LooperRealtimeStateMini]) -> CompanionAssistantSurface {
        minis
            .sorted { left, right in
                if left.seq != right.seq {
                    return left.seq > right.seq
                }
                return left.sessionID < right.sessionID
            }
            .lazy
            .compactMap { CompanionAssistantSurface(rawValue: $0.assistantSurface) }
            .first ?? .defaultSurface
    }

    private func revision(latestSeq: Int64, minis: [LooperRealtimeStateMini]) -> String {
        minis
            .max { left, right in left.seq < right.seq }?
            .revision
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty ?? "\(Defaults.revisionPrefix)\(latestSeq)"
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

extension CompanionSessionMiniLocalStore: LooperRealtimeStateMiniLocalState {
    func currentStateMiniSnapshot() -> LooperRealtimeLocalSnapshot {
        do {
            return try localSnapshot(from: clientCore.snapshot())
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-snapshot-failed error=\(error.localizedDescription)"
            )
            return store.snapshot()
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
    func applyStateMiniDelta(_ delta: LooperRealtimeStateMiniDelta) throws
        -> LooperRealtimeLocalSnapshot
    {
        let before = try clientCore.snapshot()
        let coreSnapshot = try clientCore.applyStateMiniDelta(delta: ClientStateMiniDelta(delta))
        guard coreSnapshot.hasStateMiniChanges(comparedTo: before) else {
            return localSnapshot(from: coreSnapshot)
        }
        return try persistValidated(coreSnapshot)
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

    init(_ snapshot: LooperRealtimeLocalSnapshot) {
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions.map(ClientStateMini.init),
            serverTime: snapshot.serverTime ?? ""
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

private extension ClientStateSnapshot {
    func hasStateMiniChanges(comparedTo before: ClientStateSnapshot) -> Bool {
        latestSeq != before.latestSeq
            || serverTime != before.serverTime
            || stateMinis != before.stateMinis
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

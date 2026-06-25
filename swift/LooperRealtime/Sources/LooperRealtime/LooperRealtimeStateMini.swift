import Foundation

public struct LooperRealtimeStateMini: Codable, Equatable, Sendable {
    public let sessionID: String
    public let assistantSurface: String
    public let seq: Int64
    public let revision: String
    public let payloadJSON: String

    public init(
        sessionID: String,
        assistantSurface: String,
        seq: Int64,
        revision: String,
        payloadJSON: String
    ) {
        self.sessionID = sessionID
        self.assistantSurface = assistantSurface
        self.seq = seq
        self.revision = revision
        self.payloadJSON = payloadJSON
    }
}

public struct LooperRealtimeStateMiniSnapshot: Codable, Equatable, Sendable {
    public let latestSeq: Int64
    public let sessions: [LooperRealtimeStateMini]
    public let serverTime: String?

    public init(
        latestSeq: Int64,
        sessions: [LooperRealtimeStateMini],
        serverTime: String?
    ) {
        self.latestSeq = latestSeq
        self.sessions = sessions
        self.serverTime = serverTime
    }
}

public struct LooperRealtimeStateMiniDelta: Codable, Equatable, Sendable {
    public let seq: Int64
    public let latestSeq: Int64
    public let entityID: String
    public let kind: String
    public let revision: String
    public let serverTime: String?
    public let session: LooperRealtimeStateMini?
    public let sessionID: String?
    public let assistantSurface: String?
    public let sessions: [LooperRealtimeStateMini]

    public init(
        seq: Int64,
        latestSeq: Int64,
        entityID: String,
        kind: String,
        revision: String,
        serverTime: String?,
        session: LooperRealtimeStateMini? = nil,
        sessionID: String? = nil,
        assistantSurface: String? = nil,
        sessions: [LooperRealtimeStateMini] = []
    ) {
        self.seq = seq
        self.latestSeq = latestSeq
        self.entityID = entityID
        self.kind = kind
        self.revision = revision
        self.serverTime = serverTime
        self.session = session
        self.sessionID = sessionID
        self.assistantSurface = assistantSurface
        self.sessions = sessions
    }
}

public struct LooperRealtimeLocalSnapshot: Codable, Equatable, Sendable {
    public let latestSeq: Int64
    public let sessions: [LooperRealtimeStateMini]
    public let pendingCommands: [LooperRealtimePendingCommand]
    public let serverTime: String?

    public init(
        latestSeq: Int64,
        sessions: [LooperRealtimeStateMini],
        pendingCommands: [LooperRealtimePendingCommand],
        serverTime: String?
    ) {
        self.latestSeq = latestSeq
        self.sessions = sessions
        self.pendingCommands = pendingCommands
        self.serverTime = serverTime
    }
}

public typealias LooperRealtimeLocalStateSnapshot = LooperRealtimeLocalSnapshot

public struct LooperRealtimePendingCommand: Codable, Equatable, Sendable {
    public enum Kind: String, Codable, Equatable, Sendable {
        case setSessionMode = "SetSessionMode"
        case sendSessionPrompt = "SendSessionPrompt"
        case submitNotificationReply = "SubmitNotificationReply"
    }

    public let kind: Kind
    public let clientMutationID: String
    public let threadID: String
    public let preset: String?
    public let assistantSurface: String?
    public let prompt: String?
    public let notificationID: String?
    public let attemptCount: Int

    public init(
        kind: Kind,
        clientMutationID: String,
        threadID: String,
        preset: String? = nil,
        assistantSurface: String? = nil,
        prompt: String? = nil,
        notificationID: String? = nil,
        attemptCount: Int = 0
    ) {
        self.kind = kind
        self.clientMutationID = clientMutationID
        self.threadID = threadID
        self.preset = preset
        self.assistantSurface = assistantSurface
        self.prompt = prompt
        self.notificationID = notificationID
        self.attemptCount = attemptCount
    }

    func attempted() -> Self {
        Self(
            kind: kind,
            clientMutationID: clientMutationID,
            threadID: threadID,
            preset: preset,
            assistantSurface: assistantSurface,
            prompt: prompt,
            notificationID: notificationID,
            attemptCount: attemptCount + 1
        )
    }
}

public enum LooperRealtimeLocalStoreError: Error, Equatable, Sendable {
    case corruptStore
    case writeFailed
}

public final class LooperRealtimeLocalStore: @unchecked Sendable {
    public static let defaultFileName = "looper-realtime-state-minis.json"

    private struct StoredState: Codable, Equatable, Sendable {
        var latestSeq: Int64
        var sessions: [LooperRealtimeStateMini]
        var pendingCommands: [LooperRealtimePendingCommand]
        var serverTime: String?

        static let empty = StoredState(
            latestSeq: 0,
            sessions: [],
            pendingCommands: [],
            serverTime: nil
        )

        var snapshot: LooperRealtimeLocalSnapshot {
            LooperRealtimeLocalSnapshot(
                latestSeq: latestSeq,
                sessions: sessions.sorted(by: Self.sortSessions),
                pendingCommands: pendingCommands,
                serverTime: serverTime
            )
        }

        private static func sortSessions(
            lhs: LooperRealtimeStateMini,
            rhs: LooperRealtimeStateMini
        ) -> Bool {
            if lhs.seq != rhs.seq {
                return lhs.seq < rhs.seq
            }
            if lhs.assistantSurface != rhs.assistantSurface {
                return lhs.assistantSurface < rhs.assistantSurface
            }
            return lhs.sessionID < rhs.sessionID
        }
    }

    private let fileURL: URL
    private let lock = NSLock()
    private var state: StoredState

    public init(fileURL: URL) throws {
        self.fileURL = fileURL
        state = try Self.load(fileURL: fileURL)
    }

    public convenience init(recovering fileURL: URL) throws {
        do {
            try self.init(fileURL: fileURL)
        } catch {
            try? FileManager.default.removeItem(at: fileURL)
            try self.init(fileURL: fileURL)
        }
    }

    public func snapshot() -> LooperRealtimeLocalSnapshot {
        lock.withLock {
            state.snapshot
        }
    }

    @discardableResult
    public func replace(with snapshot: LooperRealtimeStateMiniSnapshot) throws
        -> LooperRealtimeLocalSnapshot
    {
        try lock.withLock {
            state.latestSeq = snapshot.latestSeq
            state.sessions = snapshot.sessions
            state.serverTime = snapshot.serverTime
            try persistLocked()
            return state.snapshot
        }
    }

    @discardableResult
    public func apply(_ delta: LooperRealtimeStateMiniDelta) throws -> LooperRealtimeLocalSnapshot {
        try lock.withLock {
            guard delta.seq > state.latestSeq else {
                return state.snapshot
            }

            if let session = delta.session {
                upsertLocked(session)
            } else if !delta.sessions.isEmpty {
                state.sessions = delta.sessions
            }

            state.latestSeq = max(delta.latestSeq, delta.seq, state.latestSeq)
            state.serverTime = delta.serverTime ?? state.serverTime
            try persistLocked()
            return state.snapshot
        }
    }

    @discardableResult
    public func enqueue(_ command: LooperRealtimePendingCommand) throws -> LooperRealtimeLocalSnapshot {
        try lock.withLock {
            if let index = state.pendingCommands.firstIndex(where: {
                $0.clientMutationID == command.clientMutationID
            }) {
                let existing = state.pendingCommands[index]
                state.pendingCommands[index] = LooperRealtimePendingCommand(
                    kind: existing.kind,
                    clientMutationID: existing.clientMutationID,
                    threadID: existing.threadID,
                    preset: command.preset ?? existing.preset,
                    assistantSurface: command.assistantSurface ?? existing.assistantSurface,
                    prompt: command.prompt ?? existing.prompt,
                    notificationID: command.notificationID ?? existing.notificationID,
                    attemptCount: existing.attemptCount
                )
            } else {
                state.pendingCommands.append(command)
            }
            try persistLocked()
            return state.snapshot
        }
    }

    @discardableResult
    public func markAttempted(clientMutationID: String) throws -> LooperRealtimeLocalSnapshot {
        try lock.withLock {
            if let index = state.pendingCommands.firstIndex(where: {
                $0.clientMutationID == clientMutationID
            }) {
                state.pendingCommands[index] = state.pendingCommands[index].attempted()
                try persistLocked()
            }
            return state.snapshot
        }
    }

    public func markDelivered(clientMutationID: String) throws {
        try lock.withLock {
            state.pendingCommands.removeAll { $0.clientMutationID == clientMutationID }
            try persistLocked()
        }
    }

    private func upsertLocked(_ session: LooperRealtimeStateMini) {
        if let index = state.sessions.firstIndex(where: {
            $0.sessionID == session.sessionID && $0.assistantSurface == session.assistantSurface
        }) {
            state.sessions[index] = session
        } else {
            state.sessions.append(session)
        }
    }

    private static func load(fileURL: URL) throws -> StoredState {
        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            return .empty
        }

        do {
            let data = try Data(contentsOf: fileURL)
            guard !data.isEmpty else {
                return .empty
            }
            return try JSONDecoder().decode(StoredState.self, from: data)
        } catch {
            throw LooperRealtimeLocalStoreError.corruptStore
        }
    }

    private func persistLocked() throws {
        do {
            let directoryURL = fileURL.deletingLastPathComponent()
            try FileManager.default.createDirectory(
                at: directoryURL,
                withIntermediateDirectories: true
            )
            let data = try JSONEncoder().encode(state)
            try data.write(to: fileURL, options: [.atomic])
        } catch {
            throw LooperRealtimeLocalStoreError.writeFailed
        }
    }
}

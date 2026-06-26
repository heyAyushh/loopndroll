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

}

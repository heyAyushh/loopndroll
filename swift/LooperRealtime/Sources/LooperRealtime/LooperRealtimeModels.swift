import Foundation
import LooperClientCore

public struct LooperRealtimeEndpoint: Codable, Equatable, Hashable, Sendable {
    public let baseURL: URL

    public init(baseURL: URL) {
        self.baseURL = baseURL
    }

    public var host: String? {
        baseURL.host()
    }

    public var port: Int? {
        baseURL.port
    }

    public var usesTLS: Bool {
        baseURL.scheme == "https"
    }
}

public struct LooperRealtimeCredentials: Codable, Equatable, Sendable {
    public let bearerToken: String?
    public let mobileSessionHeader: String?

    public init(bearerToken: String?, mobileSessionHeader: String?) {
        self.bearerToken = bearerToken
        self.mobileSessionHeader = mobileSessionHeader
    }
}

public struct LooperRealtimeEvent: Codable, Equatable, Sendable {
    public let eventName: String
    public let threadID: String?
    public let promptID: String?
    public let detail: String?
    public let serverTime: String?
    public let revision: String?

    public init(
        eventName: String,
        threadID: String?,
        promptID: String?,
        detail: String?,
        serverTime: String?,
        revision: String?
    ) {
        self.eventName = eventName
        self.threadID = threadID
        self.promptID = promptID
        self.detail = detail
        self.serverTime = serverTime
        self.revision = revision
    }
}

public struct LooperRealtimeCommandAck: Codable, Equatable, Sendable {
    public let accepted: Bool
    public let clientMutationID: String
    public let ackSeq: Int64
    public let entityID: String
    public let revision: String
    public let serverTime: String?
    public let idempotentReplay: Bool
    public let errorCode: String?
    public let rejectReason: String?

    public init(
        accepted: Bool,
        clientMutationID: String,
        ackSeq: Int64,
        entityID: String,
        revision: String,
        serverTime: String?,
        idempotentReplay: Bool,
        errorCode: String? = nil,
        rejectReason: String? = nil
    ) {
        self.accepted = accepted
        self.clientMutationID = clientMutationID
        self.ackSeq = ackSeq
        self.entityID = entityID
        self.revision = revision
        self.serverTime = serverTime
        self.idempotentReplay = idempotentReplay
        self.errorCode = errorCode
        self.rejectReason = rejectReason
    }

    public var clientCoreAck: ClientCommandAck {
        ClientCommandAck(
            accepted: accepted,
            clientMutationId: clientMutationID,
            ackSeq: ackSeq,
            entityId: entityID,
            revision: revision,
            serverTime: serverTime ?? "",
            idempotentReplay: idempotentReplay,
            errorCode: errorCode ?? "",
            rejectReason: rejectReason ?? "",
            currentState: ""
        )
    }

    init(_ ack: ClientCommandAck) {
        self.init(
            accepted: ack.accepted,
            clientMutationID: ack.clientMutationId,
            ackSeq: ack.ackSeq,
            entityID: ack.entityId,
            revision: ack.revision,
            serverTime: ack.serverTime.nilIfEmpty,
            idempotentReplay: ack.idempotentReplay,
            errorCode: ack.errorCode.nilIfEmpty,
            rejectReason: ack.rejectReason.nilIfEmpty
        )
    }
}

public struct LooperRealtimeModeResponse: Codable, Equatable, Sendable {
    public let accepted: Bool
    public let threadID: String
    public let preset: String?
    public let serverTime: String?
    public let clientMutationID: String
    public let ackSeq: Int64
    public let entityID: String
    public let revision: String
    public let idempotentReplay: Bool

    public var ack: LooperRealtimeCommandAck {
        LooperRealtimeCommandAck(
            accepted: accepted,
            clientMutationID: clientMutationID,
            ackSeq: ackSeq,
            entityID: entityID,
            revision: revision,
            serverTime: serverTime,
            idempotentReplay: idempotentReplay
        )
    }

    public init(
        accepted: Bool,
        threadID: String,
        preset: String?,
        serverTime: String?,
        clientMutationID: String = "",
        ackSeq: Int64 = 0,
        entityID: String = "",
        revision: String = "",
        idempotentReplay: Bool = false
    ) {
        self.accepted = accepted
        self.threadID = threadID
        self.preset = preset
        self.serverTime = serverTime
        self.clientMutationID = clientMutationID
        self.ackSeq = ackSeq
        self.entityID = entityID
        self.revision = revision
        self.idempotentReplay = idempotentReplay
    }
}

public struct LooperRealtimePromptResponse: Codable, Equatable, Sendable {
    public let accepted: Bool
    public let dispatchKind: String
    public let promptID: String?
    public let serverTime: String?
    public let clientMutationID: String
    public let ackSeq: Int64
    public let entityID: String
    public let revision: String
    public let idempotentReplay: Bool

    public var ack: LooperRealtimeCommandAck {
        LooperRealtimeCommandAck(
            accepted: accepted,
            clientMutationID: clientMutationID,
            ackSeq: ackSeq,
            entityID: entityID,
            revision: revision,
            serverTime: serverTime,
            idempotentReplay: idempotentReplay
        )
    }

    public init(
        accepted: Bool,
        dispatchKind: String,
        promptID: String?,
        serverTime: String? = nil,
        clientMutationID: String = "",
        ackSeq: Int64 = 0,
        entityID: String = "",
        revision: String = "",
        idempotentReplay: Bool = false
    ) {
        self.accepted = accepted
        self.dispatchKind = dispatchKind
        self.promptID = promptID
        self.serverTime = serverTime
        self.clientMutationID = clientMutationID
        self.ackSeq = ackSeq
        self.entityID = entityID
        self.revision = revision
        self.idempotentReplay = idempotentReplay
    }
}

public struct LooperRealtimeNotificationReplyResponse: Codable, Equatable, Sendable {
    public let accepted: Bool
    public let dispatchKind: String
    public let promptID: String?
    public let serverTime: String?
    public let clientMutationID: String
    public let ackSeq: Int64
    public let entityID: String
    public let revision: String
    public let idempotentReplay: Bool
    public let notificationID: String

    public var ack: LooperRealtimeCommandAck {
        LooperRealtimeCommandAck(
            accepted: accepted,
            clientMutationID: clientMutationID,
            ackSeq: ackSeq,
            entityID: entityID,
            revision: revision,
            serverTime: serverTime,
            idempotentReplay: idempotentReplay
        )
    }

    public init(
        accepted: Bool,
        dispatchKind: String,
        promptID: String?,
        serverTime: String?,
        clientMutationID: String,
        ackSeq: Int64,
        entityID: String,
        revision: String,
        idempotentReplay: Bool,
        notificationID: String
    ) {
        self.accepted = accepted
        self.dispatchKind = dispatchKind
        self.promptID = promptID
        self.serverTime = serverTime
        self.clientMutationID = clientMutationID
        self.ackSeq = ackSeq
        self.entityID = entityID
        self.revision = revision
        self.idempotentReplay = idempotentReplay
        self.notificationID = notificationID
    }
}

public enum LooperRealtimeError: Error, Equatable {
    case invalidEndpoint
    case unavailable
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

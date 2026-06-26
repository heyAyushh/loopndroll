import Foundation

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

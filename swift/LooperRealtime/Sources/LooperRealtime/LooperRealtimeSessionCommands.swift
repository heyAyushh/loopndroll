import Foundation

public enum LooperRealtimeSessionCommand: Equatable, Sendable {
    case setSessionMode(threadID: String, preset: String?, clientMutationID: String)
    case sendSessionPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String
    )
    case submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String
    )

    public var commandKind: String {
        switch self {
        case .setSessionMode:
            "SetSessionMode"
        case .sendSessionPrompt:
            "SendSessionPrompt"
        case .submitNotificationReply:
            "SubmitNotificationReply"
        }
    }

    public var clientMutationID: String {
        switch self {
        case let .setSessionMode(_, _, clientMutationID),
             let .sendSessionPrompt(_, _, _, clientMutationID),
             let .submitNotificationReply(_, _, _, _, clientMutationID):
            clientMutationID
        }
    }

    var preset: String? {
        if case let .setSessionMode(_, preset, _) = self {
            return preset
        }
        return nil
    }

    var dispatchKind: String? {
        switch self {
        case .sendSessionPrompt, .submitNotificationReply:
            "accepted"
        case .setSessionMode:
            nil
        }
    }

    var notificationID: String? {
        if case let .submitNotificationReply(notificationID, _, _, _, _) = self {
            return notificationID
        }
        return nil
    }
}

public struct LooperRealtimeCommandAckEnvelope: Equatable, Sendable {
    public let commandKind: String
    public let ack: LooperRealtimeCommandAck
    public let preset: String?
    public let dispatchKind: String?
    public let promptID: String?
    public let notificationID: String?

    public init(
        commandKind: String,
        ack: LooperRealtimeCommandAck,
        preset: String?,
        dispatchKind: String?,
        promptID: String?,
        notificationID: String?
    ) {
        self.commandKind = commandKind
        self.ack = ack
        self.preset = preset
        self.dispatchKind = dispatchKind
        self.promptID = promptID
        self.notificationID = notificationID
    }
}

public struct LooperRealtimeSessionCommandBatchResponse: Equatable, Sendable {
    public let accepted: Bool
    public let commandAcks: [LooperRealtimeCommandAckEnvelope]

    public init(
        accepted: Bool,
        commandAcks: [LooperRealtimeCommandAckEnvelope]
    ) {
        self.accepted = accepted
        self.commandAcks = commandAcks
    }
}

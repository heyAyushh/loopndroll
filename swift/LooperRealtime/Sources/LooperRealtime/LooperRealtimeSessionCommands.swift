import Foundation
import LooperClientCore

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

    public init(outboundFrame frame: OutboundSessionFrame) throws {
        guard frame.frameKind == .command else {
            throw LooperRealtimeSessionCommandFrameError.unexpectedFrameKind(
                frame.frameKind.realtimeFrameKind
            )
        }

        switch frame.commandKind {
        case .setSessionMode:
            self = .setSessionMode(
                threadID: frame.threadId,
                preset: frame.preset.nilIfEmpty,
                clientMutationID: frame.clientMutationId
            )
        case .sendSessionPrompt:
            self = .sendSessionPrompt(
                threadID: frame.threadId,
                prompt: frame.prompt,
                assistantSurface: frame.assistantSurface.nilIfEmpty,
                clientMutationID: frame.clientMutationId
            )
        case .submitNotificationReply:
            self = .submitNotificationReply(
                notificationID: frame.notificationId,
                threadID: frame.threadId,
                prompt: frame.prompt,
                assistantSurface: frame.assistantSurface.nilIfEmpty,
                clientMutationID: frame.clientMutationId
            )
        case .resume:
            throw LooperRealtimeSessionCommandFrameError.unexpectedCommandKind(
                frame.commandKind.realtimeCommandKind
            )
        }
    }

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

public enum LooperRealtimeSessionCommandFrameError: Error, Equatable, Sendable {
    case unexpectedFrameKind(String)
    case unexpectedCommandKind(String)
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

    init(_ envelope: ClientCommandAckEnvelope) {
        self.init(
            commandKind: envelope.commandKind.realtimeCommandKind,
            ack: LooperRealtimeCommandAck(envelope.ack),
            preset: envelope.preset.nilIfEmpty,
            dispatchKind: envelope.dispatchKind.nilIfEmpty,
            promptID: envelope.promptId.nilIfEmpty,
            notificationID: envelope.notificationId.nilIfEmpty
        )
    }

    var clientCoreEnvelope: ClientCommandAckEnvelope {
        get throws {
            guard let commandKind = commandKind.clientCoreCommandKind else {
                throw LooperRealtimeSessionCommandFrameError.unexpectedCommandKind(commandKind)
            }
            return ClientCommandAckEnvelope(
                commandKind: commandKind,
                ack: ack.clientCoreAck,
                preset: preset ?? "",
                dispatchKind: dispatchKind ?? "",
                promptId: promptID ?? "",
                notificationId: notificationID ?? ""
            )
        }
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

    init(_ response: ClientCommandBatchResponse) {
        self.init(
            accepted: response.accepted,
            commandAcks: response.commandAcks.map(LooperRealtimeCommandAckEnvelope.init)
        )
    }

    func clientCoreResponse() throws -> ClientCommandBatchResponse {
        ClientCommandBatchResponse(
            accepted: accepted,
            commandAcks: try commandAcks.map { try $0.clientCoreEnvelope }
        )
    }

    public func expectedAcknowledgement(
        commandKind: ClientCommandKind,
        clientMutationID: String
    ) throws -> LooperRealtimeCommandAckEnvelope {
        try LooperRealtimeCommandAckEnvelope(
            reduceExpectedCommandAck(
                response: clientCoreResponse(),
                commandKind: commandKind,
                expectedClientMutationId: clientMutationID
            )
        )
    }
}

extension LooperRealtimeSessionCommand {
    var clientCoreMetadata: ClientCommandMetadata {
        ClientCommandMetadata(
            commandKind: clientCoreCommandKind,
            clientMutationId: clientMutationID,
            preset: preset ?? "",
            dispatchKind: dispatchKind ?? "",
            notificationId: notificationID ?? ""
        )
    }

    private var clientCoreCommandKind: ClientCommandKind {
        switch self {
        case .setSessionMode:
            .setSessionMode
        case .sendSessionPrompt:
            .sendSessionPrompt
        case .submitNotificationReply:
            .submitNotificationReply
        }
    }
}

private extension ClientCommandKind {
    var realtimeCommandKind: String {
        switch self {
        case .setSessionMode:
            "SetSessionMode"
        case .sendSessionPrompt:
            "SendSessionPrompt"
        case .submitNotificationReply:
            "SubmitNotificationReply"
        case .resume:
            "Resume"
        }
    }
}

private extension String {
    var clientCoreCommandKind: ClientCommandKind? {
        switch self {
        case "SetSessionMode":
            .setSessionMode
        case "SendSessionPrompt":
            .sendSessionPrompt
        case "SubmitNotificationReply":
            .submitNotificationReply
        case "Resume":
            .resume
        default:
            nil
        }
    }
}

private extension OutboundSessionFrameKind {
    var realtimeFrameKind: String {
        switch self {
        case .command:
            "Command"
        case .resume:
            "Resume"
        }
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

import Foundation

public protocol LooperRealtimeSessionCommandSubmitting: Sendable {
    func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse
}

public enum LooperRealtimeSessionCommandFrameError: Error, Equatable, Sendable {
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

    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

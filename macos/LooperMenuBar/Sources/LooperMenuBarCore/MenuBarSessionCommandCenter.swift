import Foundation
import LooperClientCore
import LooperRealtime

public protocol MenuBarSessionCommandClient: Sendable {
    func submitSessionCommandBatch(
        commands: [LooperRealtimeSessionCommand]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse
}

public actor MenuBarRealtimeSessionCommandClient: MenuBarSessionCommandClient {
    private let controlPlaneClient: any ControlPlaneClient
    private var realtimeClient: LooperRealtimeClient?

    public init(controlPlaneClient: any ControlPlaneClient) {
        self.controlPlaneClient = controlPlaneClient
    }

    public func submitSessionCommandBatch(
        commands: [LooperRealtimeSessionCommand]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        let client = try await realtimeSessionClient()
        return try await client.submitSessionCommandBatch(commands: commands)
    }

    public func disconnect() {
        realtimeClient?.disconnect()
        realtimeClient = nil
    }

    private func realtimeSessionClient() async throws -> LooperRealtimeClient {
        if let realtimeClient {
            return realtimeClient
        }

        let health = try await controlPlaneClient.fetchMobileHealth()
        let endpoints = health.preferredRealtimeBaseURLs.map(LooperRealtimeEndpoint.init(baseURL:))
        guard !endpoints.isEmpty else {
            throw LooperRealtimeError.unavailable
        }
        let client = LooperRealtimeClient(
            endpoints: endpoints,
            credentials: LooperRealtimeCredentials(bearerToken: nil, mobileSessionHeader: nil)
        )
        realtimeClient = client
        return client
    }
}

public struct MenuBarSessionModeCommandResult: Equatable, Sendable {
    public let clientMutationID: String
    public let accepted: Bool
    public let delivered: Bool
}

public struct MenuBarSessionPromptCommandResult: Equatable, Sendable {
    public let clientMutationID: String
    public let accepted: Bool
    public let delivered: Bool
    public let dispatchKind: String
}

public struct MenuBarNotificationReplyCommandResult: Equatable, Sendable {
    public let notificationID: String
    public let clientMutationID: String
    public let accepted: Bool
    public let delivered: Bool
    public let dispatchKind: String
}

public enum MenuBarSessionCommandError: Error, Equatable, Sendable {
    case emptyThreadID
    case emptyNotificationID
    case emptyPrompt
    case missingAcknowledgement(clientMutationID: String)
    case acknowledgedDifferentMutation(expected: String, actual: String)
    case unexpectedClientCoreOutboxDepth(Int)
    case unexpectedClientCoreCommandKind(String)
}

public actor MenuBarSessionCommandCenter {
    private enum CommandKind {
        static let setSessionMode = "SetSessionMode"
        static let sendSessionPrompt = "SendSessionPrompt"
        static let submitNotificationReply = "SubmitNotificationReply"
    }

    private let client: any MenuBarSessionCommandClient
    private let clientCore: LooperClientCore
    private let localStore: MenuBarSessionMiniLocalStore?

    public init(
        client: any MenuBarSessionCommandClient,
        localStore: MenuBarSessionMiniLocalStore?,
        clientCore: LooperClientCore = LooperClientCore()
    ) {
        self.client = client
        self.clientCore = clientCore
        self.localStore = localStore
    }

    @discardableResult
    public func setSessionMode(
        threadID: String,
        preset: String?,
        clientMutationID: String = UUID().uuidString
    ) async throws -> MenuBarSessionModeCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        _ = try clientCore.setMode(
            threadId: normalizedThreadID,
            preset: preset?.nilIfBlank ?? "",
            clientMutationId: clientMutationID
        )
        let command = try nextSessionCommand(expected: clientMutationID)
        if case let .setSessionMode(threadID, preset, clientMutationID) = command {
            try localStore?.enqueueModeCommand(
                threadID: threadID,
                preset: preset,
                clientMutationID: clientMutationID
            )
        }
        try localStore?.markAttempted(clientMutationID: clientMutationID)

        let response = try await client.submitSessionCommandBatch(
            commands: [command]
        )
        let envelope = try acknowledgement(
            from: response,
            commandKind: CommandKind.setSessionMode,
            expected: clientMutationID
        )
        let acknowledgedMutationID = envelope.ack.clientMutationID
        _ = try clientCore.applyCommandAck(ack: envelope.ack.clientCoreAck)
        if envelope.ack.accepted {
            try localStore?.markDelivered(clientMutationID: acknowledgedMutationID)
        }
        return MenuBarSessionModeCommandResult(
            clientMutationID: acknowledgedMutationID,
            accepted: envelope.ack.accepted,
            delivered: envelope.ack.accepted
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String = UUID().uuidString
    ) async throws -> MenuBarSessionPromptCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        _ = try clientCore.sendPrompt(
            threadId: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: assistantSurface?.nilIfBlank ?? "",
            clientMutationId: clientMutationID
        )
        let command = try nextSessionCommand(expected: clientMutationID)
        if case let .sendSessionPrompt(threadID, prompt, assistantSurface, clientMutationID) = command {
            try localStore?.enqueuePromptCommand(
                threadID: threadID,
                prompt: prompt,
                assistantSurface: assistantSurface,
                clientMutationID: clientMutationID
            )
        }
        try localStore?.markAttempted(clientMutationID: clientMutationID)

        let response = try await client.submitSessionCommandBatch(
            commands: [command]
        )
        let envelope = try acknowledgement(
            from: response,
            commandKind: CommandKind.sendSessionPrompt,
            expected: clientMutationID
        )
        let acknowledgedMutationID = envelope.ack.clientMutationID
        _ = try clientCore.applyCommandAck(ack: envelope.ack.clientCoreAck)
        if envelope.ack.accepted {
            try localStore?.markDelivered(clientMutationID: acknowledgedMutationID)
        }
        return MenuBarSessionPromptCommandResult(
            clientMutationID: acknowledgedMutationID,
            accepted: envelope.ack.accepted,
            delivered: envelope.ack.accepted,
            dispatchKind: envelope.dispatchKind ?? "accepted"
        )
    }

    @discardableResult
    public func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String = UUID().uuidString
    ) async throws -> MenuBarNotificationReplyCommandResult {
        let normalizedNotificationID = try normalizedRequired(
            notificationID,
            error: .emptyNotificationID
        )
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        _ = try clientCore.submitNotificationReply(
            notificationId: normalizedNotificationID,
            threadId: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: assistantSurface?.nilIfBlank ?? "",
            clientMutationId: clientMutationID
        )
        let command = try nextSessionCommand(expected: clientMutationID)
        if case let .submitNotificationReply(
            notificationID,
            threadID,
            prompt,
            assistantSurface,
            clientMutationID
        ) = command {
            try localStore?.enqueueNotificationReplyCommand(
                notificationID: notificationID,
                threadID: threadID,
                prompt: prompt,
                assistantSurface: assistantSurface,
                clientMutationID: clientMutationID
            )
        }
        try localStore?.markAttempted(clientMutationID: clientMutationID)

        let response = try await client.submitSessionCommandBatch(
            commands: [command]
        )
        let envelope = try acknowledgement(
            from: response,
            commandKind: CommandKind.submitNotificationReply,
            expected: clientMutationID
        )
        let acknowledgedMutationID = envelope.ack.clientMutationID
        _ = try clientCore.applyCommandAck(ack: envelope.ack.clientCoreAck)
        if envelope.ack.accepted {
            try localStore?.markDelivered(clientMutationID: acknowledgedMutationID)
        }
        return MenuBarNotificationReplyCommandResult(
            notificationID: envelope.notificationID ?? normalizedNotificationID,
            clientMutationID: acknowledgedMutationID,
            accepted: envelope.ack.accepted,
            delivered: envelope.ack.accepted,
            dispatchKind: envelope.dispatchKind ?? "accepted"
        )
    }

    private func normalizedRequired(
        _ value: String,
        error: MenuBarSessionCommandError
    ) throws -> String {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            throw error
        }
        return trimmed
    }

    private func nextSessionCommand(expected clientMutationID: String) throws
        -> LooperRealtimeSessionCommand
    {
        let frames = try clientCore.takeOutbox()
        guard frames.count == 1, let frame = frames.first else {
            throw MenuBarSessionCommandError.unexpectedClientCoreOutboxDepth(frames.count)
        }
        guard frame.clientMutationId == clientMutationID else {
            throw MenuBarSessionCommandError.acknowledgedDifferentMutation(
                expected: clientMutationID,
                actual: frame.clientMutationId
            )
        }
        do {
            return try LooperRealtimeSessionCommand(outboundFrame: frame)
        } catch {
            throw MenuBarSessionCommandError.unexpectedClientCoreCommandKind(
                String(describing: error)
            )
        }
    }

    private func acknowledgement(
        from response: LooperRealtimeSessionCommandBatchResponse,
        commandKind: String,
        expected: String
    ) throws -> LooperRealtimeCommandAckEnvelope {
        guard let envelope = response.commandAcks.first(where: {
            $0.commandKind == commandKind && $0.ack.clientMutationID == expected
        }) else {
            throw MenuBarSessionCommandError.missingAcknowledgement(clientMutationID: expected)
        }
        let acknowledged = envelope.ack.clientMutationID.nilIfBlank ?? expected
        guard acknowledged == expected else {
            throw MenuBarSessionCommandError.acknowledgedDifferentMutation(
                expected: expected,
                actual: acknowledged
            )
        }
        return envelope
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

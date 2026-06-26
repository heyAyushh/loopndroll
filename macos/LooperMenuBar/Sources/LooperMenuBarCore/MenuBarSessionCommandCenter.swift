import Foundation
import LooperClientCore

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
    case localStoreUnavailable
}

public actor MenuBarSessionCommandCenter {
    private let localStore: MenuBarSessionMiniLocalStore?

    public init(
        localStore: MenuBarSessionMiniLocalStore?
    ) {
        self.localStore = localStore
    }

    @discardableResult
    public func setSessionMode(
        threadID: String,
        preset: String?,
        clientMutationID: String? = nil
    ) async throws -> MenuBarSessionModeCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let sessionManager = try requiredSessionManager()
        let normalizedPreset = preset?.nilIfBlank ?? ""
        let envelope: ClientCommandAckEnvelope
        if let clientMutationID {
            envelope = try await sessionManager.setMode(
                threadID: normalizedThreadID,
                preset: normalizedPreset,
                clientMutationID: clientMutationID
            )
        } else {
            envelope = try await sessionManager.setMode(
                threadID: normalizedThreadID,
                preset: normalizedPreset
            )
        }
        let acknowledgedMutationID = envelope.ack.clientMutationId
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
        clientMutationID: String? = nil
    ) async throws -> MenuBarSessionPromptCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let sessionManager = try requiredSessionManager()
        let normalizedAssistantSurface = assistantSurface?.nilIfBlank ?? ""
        let envelope: ClientCommandAckEnvelope
        if let clientMutationID {
            envelope = try await sessionManager.sendPrompt(
                threadID: normalizedThreadID,
                prompt: normalizedPrompt,
                assistantSurface: normalizedAssistantSurface,
                clientMutationID: clientMutationID
            )
        } else {
            envelope = try await sessionManager.sendPrompt(
                threadID: normalizedThreadID,
                prompt: normalizedPrompt,
                assistantSurface: normalizedAssistantSurface
            )
        }
        let acknowledgedMutationID = envelope.ack.clientMutationId
        return MenuBarSessionPromptCommandResult(
            clientMutationID: acknowledgedMutationID,
            accepted: envelope.ack.accepted,
            delivered: envelope.ack.accepted,
            dispatchKind: envelope.dispatchKind.nilIfBlank ?? "accepted"
        )
    }

    @discardableResult
    public func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String? = nil
    ) async throws -> MenuBarNotificationReplyCommandResult {
        let normalizedNotificationID = try normalizedRequired(
            notificationID,
            error: .emptyNotificationID
        )
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let sessionManager = try requiredSessionManager()
        let normalizedAssistantSurface = assistantSurface?.nilIfBlank ?? ""
        let envelope: ClientCommandAckEnvelope
        if let clientMutationID {
            envelope = try await sessionManager.submitNotificationReply(
                notificationID: normalizedNotificationID,
                threadID: normalizedThreadID,
                prompt: normalizedPrompt,
                assistantSurface: normalizedAssistantSurface,
                clientMutationID: clientMutationID
            )
        } else {
            envelope = try await sessionManager.submitNotificationReplyWithGeneratedMutation(
                notificationID: normalizedNotificationID,
                threadID: normalizedThreadID,
                prompt: normalizedPrompt,
                assistantSurface: normalizedAssistantSurface
            )
        }
        let acknowledgedMutationID = envelope.ack.clientMutationId
        return MenuBarNotificationReplyCommandResult(
            notificationID: envelope.notificationId.nilIfBlank ?? normalizedNotificationID,
            clientMutationID: acknowledgedMutationID,
            accepted: envelope.ack.accepted,
            delivered: envelope.ack.accepted,
            dispatchKind: envelope.dispatchKind.nilIfBlank ?? "accepted"
        )
    }

    private func requiredSessionManager() throws -> LooperClientCoreSessionManager {
        guard let sessionManager = localStore?.sessionManager else {
            throw MenuBarSessionCommandError.localStoreUnavailable
        }
        return sessionManager
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
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

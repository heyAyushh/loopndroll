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
    private let clientCore: LooperClientCore
    private let localStore: MenuBarSessionMiniLocalStore?

    public init(
        localStore: MenuBarSessionMiniLocalStore?,
        clientCore: LooperClientCore = LooperClientCore()
    ) {
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
        let envelope = try await clientCore.submitSetModeDurable(
            localStore: try requiredLocalStore().clientCoreLocalStore,
            threadId: normalizedThreadID,
            preset: preset?.nilIfBlank ?? "",
            clientMutationId: clientMutationID
        )
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
        clientMutationID: String = UUID().uuidString
    ) async throws -> MenuBarSessionPromptCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let envelope = try await clientCore.submitSendPromptDurable(
            localStore: try requiredLocalStore().clientCoreLocalStore,
            threadId: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: assistantSurface?.nilIfBlank ?? "",
            clientMutationId: clientMutationID
        )
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
        clientMutationID: String = UUID().uuidString
    ) async throws -> MenuBarNotificationReplyCommandResult {
        let normalizedNotificationID = try normalizedRequired(
            notificationID,
            error: .emptyNotificationID
        )
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let envelope = try await clientCore.submitNotificationReplyDurable(
            localStore: try requiredLocalStore().clientCoreLocalStore,
            notificationId: normalizedNotificationID,
            threadId: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: assistantSurface?.nilIfBlank ?? "",
            clientMutationId: clientMutationID
        )
        let acknowledgedMutationID = envelope.ack.clientMutationId
        return MenuBarNotificationReplyCommandResult(
            notificationID: envelope.notificationId.nilIfBlank ?? normalizedNotificationID,
            clientMutationID: acknowledgedMutationID,
            accepted: envelope.ack.accepted,
            delivered: envelope.ack.accepted,
            dispatchKind: envelope.dispatchKind.nilIfBlank ?? "accepted"
        )
    }

    private func requiredLocalStore() throws -> MenuBarSessionMiniLocalStore {
        guard let localStore else {
            throw MenuBarSessionCommandError.localStoreUnavailable
        }
        return localStore
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

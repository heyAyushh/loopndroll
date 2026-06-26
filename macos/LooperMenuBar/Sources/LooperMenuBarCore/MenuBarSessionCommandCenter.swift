import Foundation
import LooperClientCore

public struct MenuBarSessionModeCommandResult: Equatable, Sendable {
    public let accepted: Bool
    public let delivered: Bool
}

public struct MenuBarSessionPromptCommandResult: Equatable, Sendable {
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
    case sessionRuntimeUnavailable
}

public actor MenuBarSessionCommandCenter {
    private let sessionRuntime: MenuBarSessionRuntime?

    public init(
        sessionRuntime: MenuBarSessionRuntime?
    ) {
        self.sessionRuntime = sessionRuntime
    }

    @discardableResult
    public func setSessionMode(
        threadID: String,
        preset: String?
    ) async throws -> MenuBarSessionModeCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let sessionRuntime = try requiredSessionRuntime()
        let normalizedPreset = preset?.nilIfBlank ?? ""
        let result = try await sessionRuntime.setSessionMode(
            threadID: normalizedThreadID,
            preset: normalizedPreset
        )
        return MenuBarSessionModeCommandResult(
            accepted: result.accepted,
            delivered: result.accepted
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String?
    ) async throws -> MenuBarSessionPromptCommandResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let sessionRuntime = try requiredSessionRuntime()
        let normalizedAssistantSurface = assistantSurface?.nilIfBlank ?? ""
        let result = try await sessionRuntime.sendPrompt(
            threadID: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: normalizedAssistantSurface
        )
        return MenuBarSessionPromptCommandResult(
            accepted: result.accepted,
            delivered: result.accepted,
            dispatchKind: result.dispatchKind.nilIfBlank ?? "accepted"
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
        let sessionRuntime = try requiredSessionRuntime()
        let normalizedAssistantSurface = assistantSurface?.nilIfBlank ?? ""
        let result = try await sessionRuntime.submitNotificationReply(
            notificationID: normalizedNotificationID,
            threadID: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: normalizedAssistantSurface,
            clientMutationID: clientMutationID
        )
        return MenuBarNotificationReplyCommandResult(
            notificationID: result.notificationId.nilIfBlank ?? normalizedNotificationID,
            clientMutationID: result.clientMutationId,
            accepted: result.accepted,
            delivered: result.accepted,
            dispatchKind: result.dispatchKind.nilIfBlank ?? "accepted"
        )
    }

    private func requiredSessionRuntime() throws -> MenuBarSessionRuntime {
        guard let sessionRuntime else {
            throw MenuBarSessionCommandError.sessionRuntimeUnavailable
        }
        return sessionRuntime
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

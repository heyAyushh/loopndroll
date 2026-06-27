import Foundation
import LooperClientCore

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
    ) async throws -> ClientSessionModeIntentResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let sessionRuntime = try requiredSessionRuntime()
        let normalizedPreset = preset?.nilIfBlank ?? ""
        return try await sessionRuntime.setSessionMode(
            threadID: normalizedThreadID,
            preset: normalizedPreset
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String?
    ) async throws -> ClientSessionPromptIntentResult {
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let sessionRuntime = try requiredSessionRuntime()
        let normalizedAssistantSurface = assistantSurface?.nilIfBlank ?? ""
        return try await sessionRuntime.sendPrompt(
            threadID: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: normalizedAssistantSurface
        )
    }

    @discardableResult
    public func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String? = nil
    ) async throws -> ClientNotificationReplyIntentResult {
        let normalizedNotificationID = try normalizedRequired(
            notificationID,
            error: .emptyNotificationID
        )
        let normalizedThreadID = try normalizedRequired(threadID, error: .emptyThreadID)
        let normalizedPrompt = try normalizedRequired(prompt, error: .emptyPrompt)
        let sessionRuntime = try requiredSessionRuntime()
        let normalizedAssistantSurface = assistantSurface?.nilIfBlank ?? ""
        return try await sessionRuntime.submitNotificationReply(
            notificationID: normalizedNotificationID,
            threadID: normalizedThreadID,
            prompt: normalizedPrompt,
            assistantSurface: normalizedAssistantSurface,
            clientMutationID: clientMutationID
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

import Foundation
import LooperClientCore

struct CompanionPromptSendResult: Sendable {
    let promptID: String?
    let dispatchKind: String?

    static func accepted(
        promptID: String?,
        dispatchKind: String?
    ) -> Self {
        Self(
            promptID: promptID,
            dispatchKind: dispatchKind
        )
    }
}

struct CompanionSessionModeResult: Sendable {
    let acceptedMode: SessionMode?

    static func accepted(
        mode: SessionMode?
    ) -> Self {
        Self(
            acceptedMode: mode
        )
    }
}

protocol CompanionSessionCommanding: Sendable {
    func setSessionMode(
        id: String,
        preset: SessionMode?
    ) async throws -> CompanionSessionModeResult
    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult
    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult
    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult
    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult
}

struct UnconfiguredCompanionSessionCommands: CompanionSessionCommanding {
    let error: Error

    func setSessionMode(
        id _: String,
        preset _: SessionMode?
    ) async throws -> CompanionSessionModeResult {
        throw error
    }

    func sendSessionPrompt(
        id _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult {
        throw error
    }

    func submitNotificationReply(
        notificationID _: String,
        sessionID _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult {
        throw error
    }

    func submitNotificationReply(
        notificationID _: String,
        sessionID _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?,
        clientMutationID _: String
    ) async throws -> ClientNotificationReplyIntentResult {
        throw error
    }

    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult {
        throw error
    }
}

import Foundation
import LooperClientCore

struct ResolvedCompanionServerHealth: Sendable {
    let health: CompanionServerHealth
    let reachedBaseURL: URL?
}

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

protocol CompanionService: Sendable {
    func prepareSessionRuntime() async
    func loadServerHealth() async throws -> CompanionServerHealth
    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth
    func loadSnapshot() async throws -> MobileSnapshot
    func loadSessionDetail(id: String, surface: CompanionAssistantSurface?) async throws -> SessionDetail
    func setSessionMode(
        id: String,
        preset: SessionMode?
    ) async throws -> CompanionSessionModeResult
    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot
    func deleteSession(id: String) async throws -> MobileSnapshot
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
    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult
    func muteSession(id: String) async throws -> MobileSnapshot
    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot
    func saveAssistantSurface(_ surface: CompanionAssistantSurface) async throws -> MobileSnapshot
    func saveSiriDefaultSession(
        id: String?,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot
    func saveSiriCurrentSession(
        id: String?,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot
    func registerPushDevice(_ request: RemotePushRegistrationRequest) async throws -> RemotePushRegistrationResponse
    func sendTestPush(installationID: String) async throws -> RemotePushTestResponse
}

extension CompanionService {
    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth {
        ResolvedCompanionServerHealth(
            health: try await loadServerHealth(),
            reachedBaseURL: nil
        )
    }

}

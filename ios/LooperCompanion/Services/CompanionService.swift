import Foundation
import LooperClientCore

struct ResolvedCompanionServerHealth: Sendable {
    let health: CompanionServerHealth
    let reachedBaseURL: URL?
}

struct CompanionPromptSendResult: Sendable {
    let promptID: String?
    let dispatchKind: String?
    let serverTime: String?
    let clientMutationID: String?
    let ackSeq: Int64
    let revision: String?

    static func accepted(
        promptID: String?,
        dispatchKind: String?,
        serverTime: String? = nil,
        clientMutationID: String?,
        ackSeq: Int64 = 0,
        revision: String? = nil
    ) -> Self {
        Self(
            promptID: promptID,
            dispatchKind: dispatchKind,
            serverTime: serverTime,
            clientMutationID: clientMutationID,
            ackSeq: ackSeq,
            revision: revision
        )
    }
}

struct CompanionSessionModeResult: Sendable {
    let acceptedMode: SessionMode?
    let serverTime: String?
    let clientMutationID: String?
    let ackSeq: Int64
    let revision: String?

    static func accepted(
        mode: SessionMode?,
        serverTime: String?,
        clientMutationID: String?,
        ackSeq: Int64 = 0,
        revision: String? = nil
    ) -> Self {
        Self(
            acceptedMode: mode,
            serverTime: serverTime,
            clientMutationID: clientMutationID,
            ackSeq: ackSeq,
            revision: revision
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
    func setSessionMode(
        id: String,
        preset: SessionMode?,
        clientMutationID: String
    ) async throws -> CompanionSessionModeResult
    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot
    func deleteSession(id: String) async throws -> MobileSnapshot
    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult
    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> CompanionPromptSendResult
    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> LooperRealtimeNotificationReplyResponse
    func submitPendingNotificationReply() async throws -> LooperRealtimeNotificationReplyResponse
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

    func setSessionMode(
        id: String,
        preset: SessionMode?
    ) async throws -> CompanionSessionModeResult {
        try await setSessionMode(
            id: id,
            preset: preset,
            clientMutationID: UUID().uuidString
        )
    }

    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult {
        try await sendSessionPrompt(
            id: id,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationID: UUID().uuidString
        )
    }

}

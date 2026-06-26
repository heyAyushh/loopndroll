import Foundation
import LooperClientCore

struct ResolvedCompanionServerHealth: Sendable {
    let health: CompanionServerHealth
    let reachedBaseURL: URL?
}

struct CompanionPromptSendResult: Sendable {
    let snapshot: MobileSnapshot?
    let promptID: String?
    let dispatchKind: String?
    let clientMutationID: String?

    static func snapshot(_ snapshot: MobileSnapshot, clientMutationID: String? = nil) -> Self {
        Self(
            snapshot: snapshot,
            promptID: nil,
            dispatchKind: nil,
            clientMutationID: clientMutationID
        )
    }

    static func accepted(
        promptID: String?,
        dispatchKind: String?,
        clientMutationID: String?
    ) -> Self {
        Self(
            snapshot: nil,
            promptID: promptID,
            dispatchKind: dispatchKind,
            clientMutationID: clientMutationID
        )
    }
}

struct CompanionSessionModeResult: Sendable {
    let snapshot: MobileSnapshot?
    let acceptedMode: SessionMode?
    let serverTime: String?
    let clientMutationID: String?

    static func snapshot(_ snapshot: MobileSnapshot, clientMutationID: String? = nil) -> Self {
        Self(
            snapshot: snapshot,
            acceptedMode: nil,
            serverTime: nil,
            clientMutationID: clientMutationID
        )
    }

    static func accepted(
        mode: SessionMode?,
        serverTime: String?,
        clientMutationID: String?
    ) -> Self {
        Self(
            snapshot: nil,
            acceptedMode: mode,
            serverTime: serverTime,
            clientMutationID: clientMutationID
        )
    }
}

struct CompanionModePromptBatchResult: Sendable {
    let mode: CompanionSessionModeResult
    let prompt: CompanionPromptSendResult
}

protocol CompanionService: Sendable {
    var supportsModePromptBatch: Bool { get }
    var sessionCommandClientCore: LooperClientCore? { get }
    func prepareRealtimeConnection() async
    func makeClientCoreStateMiniStreamTransport() async
        -> (any LooperClientCoreStateMiniStreamTransport)?
    func loadServerHealth() async throws -> CompanionServerHealth
    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth
    func loadSnapshot() async throws -> MobileSnapshot
    func loadSessionDetail(id: String, surface: CompanionAssistantSurface?) async throws -> SessionDetail
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
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> CompanionPromptSendResult
    func sendSessionPromptAfterMode(
        id: String,
        modePreset: SessionMode?,
        modeClientMutationID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        promptClientMutationID: String
    ) async throws -> CompanionModePromptBatchResult
    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> LooperRealtimeNotificationReplyResponse
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
    var supportsModePromptBatch: Bool {
        false
    }

    var sessionCommandClientCore: LooperClientCore? {
        nil
    }

    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth {
        ResolvedCompanionServerHealth(
            health: try await loadServerHealth(),
            reachedBaseURL: nil
        )
    }

    func makeClientCoreStateMiniStreamTransport() async
        -> (any LooperClientCoreStateMiniStreamTransport)?
    {
        nil
    }

    func sendSessionPromptAfterMode(
        id: String,
        modePreset: SessionMode?,
        modeClientMutationID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        promptClientMutationID: String
    ) async throws -> CompanionModePromptBatchResult {
        let mode = try await setSessionMode(
            id: id,
            preset: modePreset,
            clientMutationID: modeClientMutationID
        )
        let prompt = try await sendSessionPrompt(
            id: id,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationID: promptClientMutationID
        )
        return CompanionModePromptBatchResult(mode: mode, prompt: prompt)
    }
}

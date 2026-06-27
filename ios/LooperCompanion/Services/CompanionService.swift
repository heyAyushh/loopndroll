import Foundation
struct ResolvedCompanionServerHealth: Sendable {
    let health: CompanionServerHealth
    let reachedBaseURL: URL?
}

protocol CompanionService: Sendable {
    func loadServerHealth() async throws -> CompanionServerHealth
    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth
    func loadSnapshot() async throws -> MobileSnapshot
    func loadSessionDetail(id: String, surface: CompanionAssistantSurface?) async throws -> SessionDetail
    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot
    func deleteSession(id: String) async throws -> MobileSnapshot
    func muteSession(id: String) async throws -> MobileSnapshot
    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot
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

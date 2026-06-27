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

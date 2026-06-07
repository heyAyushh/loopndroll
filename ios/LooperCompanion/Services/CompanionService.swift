import Foundation

protocol CompanionService: Sendable {
    func makeMobileEventStreamClient() -> MobileEventStreamClient
    func loadServerHealth() async throws -> CompanionServerHealth
    func loadSnapshot() async throws -> MobileSnapshot
    func loadSessionDetail(id: String) async throws -> SessionDetail
    func setSessionMode(id: String, preset: SessionMode?) async throws -> MobileSnapshot
    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot
    func deleteSession(id: String) async throws -> MobileSnapshot
    func sendSessionPrompt(id: String, prompt: String) async throws -> MobileSnapshot
    func muteSession(id: String) async throws -> MobileSnapshot
    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot
    func saveAssistantSurface(_ surface: CompanionAssistantSurface) async throws -> MobileSnapshot
    func registerPushDevice(_ request: RemotePushRegistrationRequest) async throws -> RemotePushRegistrationResponse
    func sendTestPush(installationID: String) async throws -> RemotePushTestResponse
}

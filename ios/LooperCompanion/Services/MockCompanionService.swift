import Foundation
import LooperClientCore
import LooperCompanionCore

private let mockCompanionBaseURL = "preview://looper"

actor MockCompanionStore {
    private let allSessions: [SessionSummary]
    var snapshot: MobileSnapshot
    var remotePushRegistration = RemotePushRegistrationResponse(
        state: .enabled,
        environment: .development,
        registeredAt: Date().ISO8601Format(),
        message: "Remote push is ready on this Mac."
    )

    init() {
        let initialSnapshot = PreviewFixtures.snapshot
        allSessions = initialSnapshot.sessions
        snapshot = initialSnapshot
        snapshot.sessions = Self.filteredSessions(
            allSessions,
            surface: initialSnapshot.globalSettings.assistantSurface
        )
        snapshot.surfaceSessions = Self.surfaceSessions(allSessions)
    }

    private static func filteredSessions(
        _ sessions: [SessionSummary],
        surface: CompanionAssistantSurface
    ) -> [SessionSummary] {
        sessions.filter { session in
            CompanionSurfaceFiltering.matches(
                assistantClient: session.assistantClient.rawValue,
                surface: surface.rawValue
            )
        }
    }

    private static func surfaceSessions(
        _ sessions: [SessionSummary]
    ) -> [String: [SessionSummary]] {
        Dictionary(uniqueKeysWithValues: CompanionAssistantSurface.allCases.map { surface in
            (surface.rawValue, filteredSessions(sessions, surface: surface))
        })
    }

    func registerPushDevice(
        _ request: RemotePushRegistrationRequest
    ) -> RemotePushRegistrationResponse {
        remotePushRegistration = RemotePushRegistrationResponse(
            state: .enabled,
            environment: request.environment,
            registeredAt: Date().ISO8601Format(),
            message: "Remote push is ready on this Mac."
        )
        return remotePushRegistration
    }

    func sendTestPush() -> RemotePushTestResponse {
        RemotePushTestResponse(delivered: true, message: "Test push sent.")
    }
}

struct MockCompanionService: CompanionService {
    private let store = MockCompanionStore()

    func loadServerHealth() async throws -> CompanionServerHealth {
        CompanionServerHealth(
            ok: true,
            baseURL: mockCompanionBaseURL,
            baseURLs: [mockCompanionBaseURL],
            grpcBaseURL: "http://127.0.0.1:8766",
            grpcBaseURLs: ["http://127.0.0.1:8766"],
            serverTime: Date().ISO8601Format()
        )
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        await store.snapshot
    }

    func registerPushDevice(
        _ request: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        await store.registerPushDevice(request)
    }

    func sendTestPush(installationID _: String) async throws -> RemotePushTestResponse {
        await store.sendTestPush()
    }
}

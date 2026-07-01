import Foundation
import LooperCompanionCore

struct CompanionEnvironment {
    let service: any CompanionService
    let sessionRuntime: CompanionSessionRuntime?
    let reloadsServiceFromStoredConnection: Bool

    init(
        service: any CompanionService,
        sessionRuntime: CompanionSessionRuntime? = nil,
        reloadsServiceFromStoredConnection: Bool = false
    ) {
        self.service = service
        self.sessionRuntime = sessionRuntime
        self.reloadsServiceFromStoredConnection = reloadsServiceFromStoredConnection
    }

    static func live(
        sessionRuntime: CompanionSessionRuntime? = nil
    ) -> CompanionEnvironment {
        let connection = CompanionConfiguration.resolvedConnection()
        let baseURLs = connection.baseURLs

        if !baseURLs.isEmpty {
            let service = HTTPCompanionService(
                baseURLs: baseURLs,
                bearerToken: connection.bearerToken
            )
            guard let runtime = sessionRuntime ?? CompanionSessionRuntime.liveDefault() else {
                return CompanionEnvironment(
                    service: SessionRuntimeUnavailableCompanionService(
                        fallbackService: service,
                        snapshotError: HTTPCompanionServiceError.localStoreUnavailable
                    ),
                    sessionRuntime: nil,
                    reloadsServiceFromStoredConnection: true
                )
            }
            runtime.configureStart(
                CompanionSessionRuntimeStartConfiguration(
                    bearerToken: connection.bearerToken,
                    endpointResolver: {
                        let health = try? await service.resolveServerHealth().health
                        return CompanionRealtimeEndpointResolver.endpoints(
                            configuredBaseURLs: CompanionConfiguration.uniqueAttemptableBaseURLs(
                                baseURLs.map(CompanionBaseURLRouting.canonicalHTTPAPIBaseURL)
                            ),
                            health: health
                        )
                    }
                )
            )
            return CompanionEnvironment(
                service: service,
                sessionRuntime: runtime,
                reloadsServiceFromStoredConnection: true
            )
        }

        let trimmedBaseURL = CompanionConfiguration.resolvedBaseURLString()
        let error: CompanionConfigurationError =
            trimmedBaseURL.isEmpty ? .apiBaseURLNotConfigured : .invalidAPIBaseURL
        return CompanionEnvironment(
            service: UnconfiguredCompanionService(error: error),
            sessionRuntime: sessionRuntime,
            reloadsServiceFromStoredConnection: true
        )
    }
}

private struct SessionRuntimeUnavailableCompanionService: CompanionService {
    let fallbackService: any CompanionService
    let snapshotError: Error

    func loadServerHealth() async throws -> CompanionServerHealth {
        try await fallbackService.loadServerHealth()
    }

    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth {
        try await fallbackService.resolveServerHealth()
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        throw snapshotError
    }

    func registerPushDevice(
        _ request: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        try await fallbackService.registerPushDevice(request)
    }

    func sendTestPush(installationID: String) async throws -> RemotePushTestResponse {
        try await fallbackService.sendTestPush(installationID: installationID)
    }
}

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
                        // Warm cache answers without a network round-trip so
                        // gesture-triggered recovery never blocks on health.
                        let planEndpoints = await CompanionEndpointPlanCache.shared.endpoints(
                            configuredBaseURLs: CompanionConfiguration.uniqueAttemptableBaseURLs(
                                baseURLs.map(CompanionBaseURLRouting.canonicalHTTPAPIBaseURL)
                            ),
                            healthProvider: {
                                try? await service.resolveServerHealth().health
                            }
                        )
                        if !planEndpoints.isEmpty {
                            return planEndpoints
                        }
                        // Never hand the core an empty candidate list while a
                        // stored connection exists: an unreachable preferred
                        // route or failed health fetch must degrade to the
                        // freshly-read stored base URLs (h2, no pins), not to
                        // "at least one endpoint is required" and a dead
                        // stream — observed on device after a Tailscale
                        // route switch with the VPN off.
                        let storedFallback = CompanionRealtimeEndpointResolver.endpoints(
                            configuredBaseURLs: CompanionConfiguration.uniqueAttemptableBaseURLs(
                                CompanionConfiguration.resolvedBaseURLStrings()
                                    .map(CompanionBaseURLRouting.canonicalHTTPAPIBaseURL)
                            )
                        )
                        if !storedFallback.isEmpty {
                            CompanionDiagnostics.record(
                                "session-runtime:endpoint-fallback-stored count=\(storedFallback.count)"
                            )
                        }
                        return storedFallback
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

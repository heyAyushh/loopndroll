import Foundation
struct CompanionEnvironment {
    let service: any CompanionService
    let sessionCommands: any CompanionSessionCommanding
    let sessionRuntime: CompanionSessionRuntime?
    let reloadsServiceFromStoredConnection: Bool

    init(
        service: any CompanionService,
        sessionCommands: (any CompanionSessionCommanding)? = nil,
        sessionRuntime: CompanionSessionRuntime? = nil,
        reloadsServiceFromStoredConnection: Bool = false
    ) {
        self.service = service
        if let sessionCommands {
            self.sessionCommands = sessionCommands
        } else if let serviceCommands = service as? any CompanionSessionCommanding {
            self.sessionCommands = serviceCommands
        } else {
            self.sessionCommands = UnconfiguredCompanionSessionCommands(
                error: CompanionConfigurationError.apiBaseURLNotConfigured
            )
        }
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
                    service: service,
                    sessionCommands: UnconfiguredCompanionSessionCommands(
                        error: HTTPCompanionServiceError.localStoreUnavailable
                    ),
                    sessionRuntime: nil,
                    reloadsServiceFromStoredConnection: true
                )
            }
            runtime.configureStart(
                CompanionSessionRuntimeStartConfiguration(
                    bearerToken: connection.bearerToken,
                    endpointResolver: {
                        let resolvedHealth = try await service.resolveServerHealth()
                        let health = resolvedHealth.health
                        return CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(
                            ([health.grpcBaseURL] + health.grpcBaseURLs).compactMap(URL.init(string:))
                        )
                    }
                )
            )
            return CompanionEnvironment(
                service: service,
                sessionCommands: runtime,
                sessionRuntime: runtime,
                reloadsServiceFromStoredConnection: true
            )
        }

        let trimmedBaseURL = CompanionConfiguration.resolvedBaseURLString()
        let error: CompanionConfigurationError =
            trimmedBaseURL.isEmpty ? .apiBaseURLNotConfigured : .invalidAPIBaseURL
        return CompanionEnvironment(
            service: UnconfiguredCompanionService(error: error),
            sessionCommands: UnconfiguredCompanionSessionCommands(error: error),
            sessionRuntime: sessionRuntime,
            reloadsServiceFromStoredConnection: true
        )
    }
}

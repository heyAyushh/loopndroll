import Foundation
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
                    service: service,
                    sessionRuntime: nil,
                    reloadsServiceFromStoredConnection: true
                )
            }
            runtime.configureStart(
                CompanionSessionRuntimeStartConfiguration(
                    bearerToken: connection.bearerToken,
                    endpointResolver: {
                        CompanionConfiguration.uniqueAttemptableBaseURLs(baseURLs)
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

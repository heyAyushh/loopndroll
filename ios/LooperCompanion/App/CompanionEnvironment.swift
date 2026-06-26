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
            let runtime = sessionRuntime ?? CompanionSessionRuntime.liveDefault()
            return CompanionEnvironment(
                service: HTTPCompanionService(
                    baseURLs: baseURLs,
                    bearerToken: connection.bearerToken,
                    sessionRuntime: runtime
                ),
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

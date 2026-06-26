import Foundation
struct CompanionEnvironment {
    let service: any CompanionService
    let reloadsServiceFromStoredConnection: Bool

    init(service: any CompanionService, reloadsServiceFromStoredConnection: Bool = false) {
        self.service = service
        self.reloadsServiceFromStoredConnection = reloadsServiceFromStoredConnection
    }

    static func live(
        sessionMiniLocalStore: CompanionSessionMiniLocalStore? = nil
    ) -> CompanionEnvironment {
        let connection = CompanionConfiguration.resolvedConnection()
        let baseURLs = connection.baseURLs

        if !baseURLs.isEmpty {
            let localStore = sessionMiniLocalStore ?? CompanionSessionMiniLocalStore.liveDefault()
            return CompanionEnvironment(
                service: HTTPCompanionService(
                    baseURLs: baseURLs,
                    bearerToken: connection.bearerToken,
                    sessionMiniLocalStore: localStore
                ),
                reloadsServiceFromStoredConnection: true
            )
        }

        let trimmedBaseURL = CompanionConfiguration.resolvedBaseURLString()
        let error: CompanionConfigurationError =
            trimmedBaseURL.isEmpty ? .apiBaseURLNotConfigured : .invalidAPIBaseURL
        return CompanionEnvironment(
            service: UnconfiguredCompanionService(error: error),
            reloadsServiceFromStoredConnection: true
        )
    }
}

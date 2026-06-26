import Foundation
import LooperClientCore

struct CompanionEnvironment {
    let service: any CompanionService
    let reloadsServiceFromStoredConnection: Bool

    init(service: any CompanionService, reloadsServiceFromStoredConnection: Bool = false) {
        self.service = service
        self.reloadsServiceFromStoredConnection = reloadsServiceFromStoredConnection
    }

    static func live(
        commandClientCore: LooperClientCore? = nil,
        sessionMiniLocalStore: CompanionSessionMiniLocalStore? = nil
    ) -> CompanionEnvironment {
        let connection = CompanionConfiguration.resolvedConnection()
        let baseURLs = connection.baseURLs

        if !baseURLs.isEmpty {
            let clientCore = commandClientCore ?? sessionMiniLocalStore?.clientCore ?? LooperClientCore()
            let localStore = sessionMiniLocalStore
                ?? CompanionSessionMiniLocalStore.liveDefault(clientCore: clientCore)
            return CompanionEnvironment(
                service: HTTPCompanionService(
                    baseURLs: baseURLs,
                    bearerToken: connection.bearerToken,
                    commandClientCore: clientCore,
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

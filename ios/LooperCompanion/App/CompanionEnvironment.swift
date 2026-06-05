import Foundation

struct CompanionEnvironment {
    let service: any CompanionService

    static func live() -> CompanionEnvironment {
        let connection = CompanionConfiguration.resolvedConnection()
        let baseURLs = connection.baseURLs

        if !baseURLs.isEmpty {
            return CompanionEnvironment(
                service: HTTPCompanionService(
                    baseURLs: baseURLs,
                    bearerToken: connection.bearerToken
                )
            )
        }

        let trimmedBaseURL = CompanionConfiguration.resolvedBaseURLString()
        let error: CompanionConfigurationError =
            trimmedBaseURL.isEmpty ? .apiBaseURLNotConfigured : .invalidAPIBaseURL
        return CompanionEnvironment(service: UnconfiguredCompanionService(error: error))
    }
}

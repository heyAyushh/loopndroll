import Foundation

struct CompanionEnvironment {
    let service: any CompanionService

    static func live() -> CompanionEnvironment {
        let baseURLs = CompanionConfiguration.resolvedBaseURLStrings()

        if !baseURLs.isEmpty {
            return CompanionEnvironment(service: HTTPCompanionService(baseURLs: baseURLs))
        }

        let trimmedBaseURL = CompanionConfiguration.resolvedBaseURLString()
        let error: CompanionConfigurationError =
            trimmedBaseURL.isEmpty ? .apiBaseURLNotConfigured : .invalidAPIBaseURL
        return CompanionEnvironment(service: UnconfiguredCompanionService(error: error))
    }
}

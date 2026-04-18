import Foundation

struct CompanionEnvironment {
    let service: any CompanionService

    static func live() -> CompanionEnvironment {
        let trimmedBaseURL = CompanionConfiguration.resolvedBaseURLString()

        if let baseURL = URL(string: trimmedBaseURL), !trimmedBaseURL.isEmpty {
            return CompanionEnvironment(service: HTTPCompanionService(baseURL: baseURL))
        }

        let error: CompanionConfigurationError =
            trimmedBaseURL.isEmpty ? .apiBaseURLNotConfigured : .invalidAPIBaseURL
        return CompanionEnvironment(service: UnconfiguredCompanionService(error: error))
    }
}

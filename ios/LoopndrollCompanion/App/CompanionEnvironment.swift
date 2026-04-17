import Foundation

struct CompanionEnvironment {
    let service: any CompanionService

    static func live() -> CompanionEnvironment {
        let configuredBaseURL = Bundle.main.object(forInfoDictionaryKey: "LOOPNDROLL_API_BASE_URL") as? String
        let trimmedBaseURL = configuredBaseURL?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        if let baseURL = URL(string: trimmedBaseURL), !trimmedBaseURL.isEmpty {
            return CompanionEnvironment(service: HTTPCompanionService(baseURL: baseURL))
        }

        return CompanionEnvironment(service: MockCompanionService())
    }
}

import Foundation

public struct CompanionBaseURLRaceCandidate: Equatable, Sendable {
    public let baseURL: URL
    public let delay: Duration

    public init(baseURL: URL, delay: Duration) {
        self.baseURL = baseURL
        self.delay = delay
    }
}

public enum CompanionBaseURLRacePlan {
    private static let fallbackDelayMilliseconds = 350

    public static let defaultFallbackDelay: Duration = .milliseconds(fallbackDelayMilliseconds)

    public static func candidates(
        for baseURLs: [URL],
        fallbackDelay: Duration = defaultFallbackDelay
    ) -> [CompanionBaseURLRaceCandidate] {
        CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(baseURLs)
            .enumerated()
            .map { index, baseURL in
                CompanionBaseURLRaceCandidate(
                    baseURL: baseURL,
                    delay: index == 0 ? .zero : fallbackDelay
                )
            }
    }
}

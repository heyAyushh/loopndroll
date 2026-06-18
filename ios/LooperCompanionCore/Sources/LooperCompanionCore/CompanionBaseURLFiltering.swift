import Foundation

public enum CompanionBaseURLFiltering {
    public static func uniqueAttemptableBaseURLs(_ baseURLs: [URL]) -> [URL] {
        prioritizedBaseURLs(CompanionBaseURLIdentity.unique(baseURLs, including: shouldAttempt))
    }

    private static func prioritizedBaseURLs(_ baseURLs: [URL]) -> [URL] {
        #if targetEnvironment(simulator)
            return baseURLs.sorted { lhs, rhs in
                isLoopback(lhs) && !isLoopback(rhs)
            }
        #else
            return baseURLs
        #endif
    }

    private static func shouldAttempt(_ baseURL: URL) -> Bool {
        #if targetEnvironment(simulator)
            return true
        #else
            return CompanionBaseURLRouting.isAttemptableOnPhysicalDevice(baseURL)
        #endif
    }

    private static func isLoopback(_ url: URL) -> Bool {
        CompanionBaseURLRouting.route(for: url) == .loopback
    }
}

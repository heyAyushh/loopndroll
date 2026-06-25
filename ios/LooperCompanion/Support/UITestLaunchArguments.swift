import Foundation

enum UITestLaunchArguments {
    private static let uiTestEnvironmentKey = "LOOPER_UI_TEST"
    private static let liveBaseURLsEnvironmentKey = "LOOPER_UI_TEST_API_BASE_URLS"
    private static let liveBearerTokenEnvironmentKey = "LOOPER_UI_TEST_API_BEARER_TOKEN"
    private static let liveMobileSessionEnvironmentKey = "LOOPER_UI_TEST_MOBILE_SESSION"

    static let mockMode = "--looper-ui-test-mode"
    static let resetState = "--looper-reset-ui-test-state"
    static let showOnboarding = "--looper-show-onboarding"
    static let enablePinball = "--looper-enable-pinball"
    static let seedRecentSearches = "--looper-seed-recent-searches"

    static var isUITestEnabled: Bool {
        ProcessInfo.processInfo.environment[uiTestEnvironmentKey] == "1"
    }

    static var isMockModeEnabled: Bool {
        ProcessInfo.processInfo.arguments.contains(mockMode)
    }

    static var shouldResetState: Bool {
        ProcessInfo.processInfo.arguments.contains(resetState)
    }

    static var shouldShowOnboarding: Bool {
        ProcessInfo.processInfo.arguments.contains(showOnboarding)
    }

    static var shouldEnablePinball: Bool {
        ProcessInfo.processInfo.arguments.contains(enablePinball)
    }

    static var shouldSeedRecentSearches: Bool {
        ProcessInfo.processInfo.arguments.contains(seedRecentSearches)
    }

    static var liveBaseURLs: String? {
        environmentValue(liveBaseURLsEnvironmentKey)
    }

    static var liveBearerToken: String? {
        environmentValue(liveBearerTokenEnvironmentKey)
    }

    static var liveMobileSession: String? {
        environmentValue(liveMobileSessionEnvironmentKey)
    }

    private static func environmentValue(_ key: String) -> String? {
        guard isUITestEnabled else {
            return nil
        }

        let value = ProcessInfo.processInfo.environment[key]?.trimmingCharacters(in: .whitespacesAndNewlines)
        return value?.isEmpty == false ? value : nil
    }
}

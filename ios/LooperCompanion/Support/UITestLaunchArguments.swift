import Foundation

enum UITestLaunchArguments {
    static let mockMode = "--looper-ui-test-mode"
    static let resetState = "--looper-reset-ui-test-state"
    static let showOnboarding = "--looper-show-onboarding"
    static let enablePinball = "--looper-enable-pinball"
    static let seedRecentSearches = "--looper-seed-recent-searches"

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
}

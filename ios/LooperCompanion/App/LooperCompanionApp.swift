import AppIntents
import CoreSpotlight
import SwiftUI
import UserNotifications

#if DEBUG
private enum UnitTestRuntime {
    static var isRunning: Bool {
        let environment = ProcessInfo.processInfo.environment
        return environment["XCTestConfigurationFilePath"] != nil ||
            environment["XCTestBundlePath"] != nil
    }
}
#endif

@main
struct LooperApp: App {
    @UIApplicationDelegateAdaptor(LooperAppDelegate.self) private var appDelegate
    @AppStorage("appearanceMode") private var appearanceModeRawValue = CompanionAppearanceMode.system.rawValue
    @Environment(\.scenePhase) private var scenePhase
    @StateObject private var rootCommandCenter = LooperRootCommandCenter()
    @State private var authenticator: CompanionAppAuthenticator
    @State private var model: CompanionAppModel

    init() {
        CompanionImageCache.shared.prewarm()

        #if DEBUG
        let isRunningUnitTests = UnitTestRuntime.isRunning
        let g006SelfTestCase = G006LocalFirstSelfTest.requestedCase
        let isRunningG006SelfTest = g006SelfTestCase != nil
        #else
        let isRunningUnitTests = false
        let isRunningG006SelfTest = false
        #endif

        if !isRunningUnitTests, !isRunningG006SelfTest {
            UNUserNotificationCenter.current().delegate = ForegroundNotificationDelegate.shared
            LooperSiriShortcuts.updateAppShortcutParameters()
        }

        Self.prepareUITestStateIfNeeded()
        let sessionRuntime = (isRunningUnitTests || isRunningG006SelfTest)
            ? nil
            : CompanionSessionRuntime.liveDefault()
        _authenticator = State(initialValue: CompanionAppAuthenticator())
        _model = State(
            initialValue: CompanionAppModel(
                environment: Self.environment(
                    sessionRuntime: sessionRuntime
                ),
                sessionRuntime: sessionRuntime
            )
        )

        #if DEBUG
        if let g006SelfTestCase {
            G006LocalFirstSelfTest.runSoon(g006SelfTestCase)
        }
        #endif
    }

    var body: some Scene {
        WindowGroup {
            RootTabView(
                model: model,
                authenticator: authenticator,
                commandCenter: rootCommandCenter
            )
                .preferredColorScheme(appearanceMode.colorScheme)
                .onContinueUserActivity(LooperContinuationActivity.activityType) { activity in
                    handleContinuationActivity(activity)
                }
                .onContinueUserActivity(CSSearchableItemActionType) { activity in
                    handleContinuationActivity(activity)
                }
                .onReceive(NotificationCenter.default.publisher(for: .looperDidReceiveContinuationActivity)) { notification in
                    guard let activity = notification.object as? NSUserActivity else {
                        return
                    }

                    handleContinuationActivity(activity)
                }
                .onOpenURL { url in
                    handleContinuationURL(url)
                }
                .onAppear {
                    drainPendingOpenRequests()
                }
                .task(id: sessionRuntimeLifecycleTaskID) {
                    applySessionRuntimeLifecycle()
                }
                .onChange(of: scenePhase) { _, phase in
                    guard phase == .active else {
                        return
                    }

                    drainPendingOpenRequests()
                }
        }
        .commands {
            LooperSearchCommands(commandCenter: rootCommandCenter)
        }
    }

    private var appearanceMode: CompanionAppearanceMode {
        CompanionAppearanceMode(rawValue: appearanceModeRawValue) ?? .system
    }

    private var sessionRuntimeLifecycleTaskID: String {
        "\(scenePhase)-\(authenticator.isUnlocked)"
    }

    private static func environment(
        sessionRuntime: CompanionSessionRuntime?
    ) -> CompanionEnvironment {
        #if DEBUG
        if G006LocalFirstSelfTest.requestedCase != nil {
            return CompanionEnvironment(service: MockCompanionService())
        }

        if UnitTestRuntime.isRunning {
            return CompanionEnvironment(service: MockCompanionService())
        }

        if UITestLaunchArguments.isMockModeEnabled {
            return CompanionEnvironment(service: MockCompanionService())
        }
        #endif

        return .live(
            sessionRuntime: sessionRuntime
        )
    }

    private static func prepareUITestStateIfNeeded() {
        #if DEBUG
        guard UITestLaunchArguments.isUITestEnabled || UITestLaunchArguments.isMockModeEnabled else {
            return
        }

        if UITestLaunchArguments.shouldResetState {
            let defaults = UserDefaults.standard
            CompanionConfiguration.storeConnection(CompanionConnection(baseURLs: [], bearerToken: nil))
            CompanionSnapshotCache.clear()
            defaults.removeObject(forKey: OnboardingState.completionStorageKey)
            defaults.removeObject(forKey: QuickActionSettings.storageKey)
            defaults.removeObject(forKey: "appearanceMode")
            defaults.removeObject(forKey: PinballSettingsKeys.isGameEnabled)
            defaults.removeObject(forKey: PinballSettingsKeys.isDebugOverlayEnabled)
            defaults.removeObject(forKey: CompanionConfiguration.connectionRoutePreferenceKey)
            defaults.removeObject(forKey: CompanionSearchStorage.recentQueriesKey)
        }

        if UITestLaunchArguments.shouldEnablePinball {
            UserDefaults.standard.set(true, forKey: PinballSettingsKeys.isGameEnabled)
        }

        if UITestLaunchArguments.shouldShowOnboarding {
            UserDefaults.standard.set(false, forKey: OnboardingState.completionStorageKey)
        } else {
            UserDefaults.standard.set(true, forKey: OnboardingState.completionStorageKey)
        }

        if UITestLaunchArguments.shouldSeedRecentSearches {
            UserDefaults.standard.set("looper", forKey: CompanionSearchStorage.recentQueriesKey)
        }
        #endif
    }

    private func handleContinuationActivity(_ activity: NSUserActivity) {
        Task {
            await model.continueFromMacActivity(activity)
        }
    }

    private func handleContinuationURL(_ url: URL) {
        Task {
            await model.handleOpenURL(url)
        }
    }

    private func drainPendingOpenRequests() {
        Task { @MainActor in
            for activity in LooperContinuationInbox.shared.drainActivities() {
                await model.continueFromMacActivity(activity)
            }
            await model.continueFromPendingSiriOpenSessionRequest()
        }
    }

    private func applySessionRuntimeLifecycle() {
        guard scenePhase == .active, authenticator.isUnlocked else {
            model.stopSessionRuntimeSync()
            return
        }

        model.startSessionRuntimeSyncIfNeeded()
    }
}

private struct LooperSearchCommands: Commands {
    let commandCenter: LooperRootCommandCenter

    var body: some Commands {
        CommandGroup(after: .textEditing) {
            Button("Search") {
                commandCenter.requestSearch()
            }
            .keyboardShortcut(
                LooperKeyboardShortcut.searchKey,
                modifiers: LooperKeyboardShortcut.searchModifiers
            )
        }
    }
}

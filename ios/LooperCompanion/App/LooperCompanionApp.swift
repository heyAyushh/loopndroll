import CoreSpotlight
import SwiftUI
import UserNotifications

@main
struct LooperApp: App {
    @UIApplicationDelegateAdaptor(LooperAppDelegate.self) private var appDelegate
    @AppStorage("appearanceMode") private var appearanceModeRawValue = CompanionAppearanceMode.system.rawValue
    @Environment(\.scenePhase) private var scenePhase
    @State private var authenticator: CompanionAppAuthenticator
    @State private var model: CompanionAppModel

    init() {
        UNUserNotificationCenter.current().delegate = ForegroundNotificationDelegate.shared
        _authenticator = State(initialValue: CompanionAppAuthenticator())
        _model = State(initialValue: CompanionAppModel(environment: .live()))
    }

    var body: some Scene {
        WindowGroup {
            RootTabView(model: model, authenticator: authenticator)
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
                .onChange(of: scenePhase) { _, phase in
                    guard phase == .active else {
                        return
                    }

                    drainPendingOpenRequests()
                }
        }
    }

    private var appearanceMode: CompanionAppearanceMode {
        CompanionAppearanceMode(rawValue: appearanceModeRawValue) ?? .system
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
}

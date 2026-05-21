import SwiftUI
import UserNotifications

@main
struct LooperApp: App {
    @UIApplicationDelegateAdaptor(LooperAppDelegate.self) private var appDelegate
    @AppStorage("appearanceMode") private var appearanceModeRawValue = CompanionAppearanceMode.system.rawValue
    @State private var model: CompanionAppModel

    init() {
        UNUserNotificationCenter.current().delegate = ForegroundNotificationDelegate.shared
        _model = State(initialValue: CompanionAppModel(environment: .live()))
    }

    var body: some Scene {
        WindowGroup {
            RootTabView(model: model)
                .preferredColorScheme(appearanceMode.colorScheme)
        }
    }

    private var appearanceMode: CompanionAppearanceMode {
        CompanionAppearanceMode(rawValue: appearanceModeRawValue) ?? .system
    }
}

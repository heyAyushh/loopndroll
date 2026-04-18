import SwiftUI
import UserNotifications

@main
struct LooperApp: App {
    @UIApplicationDelegateAdaptor(LooperAppDelegate.self) private var appDelegate
    @State private var model: CompanionAppModel

    init() {
        UNUserNotificationCenter.current().delegate = ForegroundNotificationDelegate.shared
        _model = State(initialValue: CompanionAppModel(environment: .live()))
    }

    var body: some Scene {
        WindowGroup {
            RootTabView(model: model)
        }
    }
}

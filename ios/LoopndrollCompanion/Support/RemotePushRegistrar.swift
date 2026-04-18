import Foundation
import UIKit

extension Notification.Name {
    static let looperDidRegisterRemotePush = Notification.Name("looper.didRegisterRemotePush")
    static let looperDidFailRemotePushRegistration = Notification.Name("looper.didFailRemotePushRegistration")
}

@MainActor
final class RemotePushRegistrar {
    static let shared = RemotePushRegistrar()

    private let installationIdentifierKey = "looper.pushInstallationIdentifier"

    private init() {}

    func registerForRemoteNotifications() {
        UIApplication.shared.registerForRemoteNotifications()
    }

    func installationIdentifier() -> String {
        if let existingValue = UserDefaults.standard.string(forKey: installationIdentifierKey),
           !existingValue.isEmpty
        {
            return existingValue
        }

        let nextValue = UUID().uuidString
        UserDefaults.standard.set(nextValue, forKey: installationIdentifierKey)
        return nextValue
    }

    func publishRegisteredToken(_ deviceToken: Data) {
        let token = deviceToken.map { String(format: "%02x", $0) }.joined()
        NotificationCenter.default.post(
            name: .looperDidRegisterRemotePush,
            object: nil,
            userInfo: ["deviceToken": token]
        )
    }

    func publishRegistrationError(_ error: any Error) {
        NotificationCenter.default.post(
            name: .looperDidFailRemotePushRegistration,
            object: nil,
            userInfo: ["message": error.localizedDescription]
        )
    }
}

final class LooperAppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        Task { @MainActor in
            RemotePushRegistrar.shared.publishRegisteredToken(deviceToken)
        }
    }

    func application(
        _: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: any Error
    ) {
        Task { @MainActor in
            RemotePushRegistrar.shared.publishRegistrationError(error)
        }
    }
}

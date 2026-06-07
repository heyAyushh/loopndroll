import Foundation
import UIKit

extension Notification.Name {
    static let looperDidRegisterRemotePush = Notification.Name("looper.didRegisterRemotePush")
    static let looperDidFailRemotePushRegistration = Notification.Name("looper.didFailRemotePushRegistration")
    static let looperDidReceiveContinuationActivity = Notification.Name("looper.didReceiveContinuationActivity")
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

@MainActor
final class LooperContinuationInbox {
    static let shared = LooperContinuationInbox()

    private var pendingActivities: [NSUserActivity] = []

    private init() {}

    func enqueue(_ activity: NSUserActivity) {
        pendingActivities.append(activity)
        CompanionDiagnostics.lifecycle.info(
            "Queued continuation activity type=\(activity.activityType, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:inbox-enqueue type=\(activity.activityType)")
        NotificationCenter.default.post(
            name: .looperDidReceiveContinuationActivity,
            object: activity
        )
    }

    func drainActivities() -> [NSUserActivity] {
        let activities = pendingActivities
        pendingActivities = []
        return activities
    }
}

final class LooperAppDelegate: NSObject, UIApplicationDelegate {
    func application(
        _: UIApplication,
        willContinueUserActivityWithType userActivityType: String
    ) -> Bool {
        let canContinue = LooperContinuationActivity.isSupportedActivityType(userActivityType)
        CompanionDiagnostics.lifecycle.info(
            "Continuation willContinue type=\(userActivityType, privacy: .public) accepted=\(canContinue, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:will-continue type=\(userActivityType) accepted=\(canContinue)")
        return canContinue
    }

    func application(
        _: UIApplication,
        continue userActivity: NSUserActivity,
        restorationHandler: @escaping ([any UIUserActivityRestoring]?) -> Void
    ) -> Bool {
        guard LooperContinuationActivity.isSupportedActivityType(userActivity.activityType) else {
            CompanionDiagnostics.record("continuation:delegate-ignore type=\(userActivity.activityType)")
            return false
        }

        CompanionDiagnostics.lifecycle.info(
            "Continuation delegate continue type=\(userActivity.activityType, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:delegate-continue type=\(userActivity.activityType)")
        Task { @MainActor in
            LooperContinuationInbox.shared.enqueue(userActivity)
        }
        restorationHandler([])
        return true
    }

    func application(
        _: UIApplication,
        didFailToContinueUserActivityWithType userActivityType: String,
        error: any Error
    ) {
        guard LooperContinuationActivity.isSupportedActivityType(userActivityType) else {
            return
        }

        CompanionDiagnostics.lifecycle.error(
            "Continuation failed error=\(error.localizedDescription, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:continue-failed error=\(error.localizedDescription)")
    }

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

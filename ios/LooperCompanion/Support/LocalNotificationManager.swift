import Foundation
import UserNotifications

extension Notification.Name {
    static let looperDidReceiveSessionQuickAction = Notification.Name(
        "looper.didReceiveSessionQuickAction"
    )
}

enum LooperNotificationPayloadKey {
    static let action = "action"
    static let notificationKind = "notificationKind"
    static let prompt = "prompt"
    static let sessionId = "sessionId"
    static let sessionRef = "sessionRef"
}

enum LooperNotificationCategory {
    static let sessionStop = "looper-session-stop"
}

private enum LooperNotificationKind {
    static let sessionStop = "session-stop"
}

@MainActor
final class LocalNotificationManager {
    private let notificationCenter: UNUserNotificationCenter

    init(notificationCenter: UNUserNotificationCenter = .current()) {
        self.notificationCenter = notificationCenter
    }

    func currentAuthorizationStatus() async -> UNAuthorizationStatus {
        let settings = await notificationCenter.notificationSettings()
        return settings.authorizationStatus
    }

    func configureStopQuickActions(_ actions: Set<QuickActionOption>) {
        let orderedActions = QuickActionOption.allCases
            .filter { actions.contains($0) }
            .map(notificationAction)

        notificationCenter.setNotificationCategories([
            UNNotificationCategory(
                identifier: LooperNotificationCategory.sessionStop,
                actions: orderedActions,
                intentIdentifiers: [],
                options: []
            )
        ])
    }

    func requestAuthorizationIfNeeded() async -> UNAuthorizationStatus {
        let currentStatus = await currentAuthorizationStatus()

        if currentStatus == .notDetermined {
            _ = try? await notificationCenter.requestAuthorization(
                options: [.alert, .badge, .sound]
            )
        }

        return await currentAuthorizationStatus()
    }

    func sendTestNotification() async -> Bool {
        guard isAuthorized(await currentAuthorizationStatus()) else {
            return false
        }

        let content = UNMutableNotificationContent()
        content.title = "Looper"
        content.subtitle = "Local alerts enabled"
        content.body = "You will get an alert when a session newly stops while looper is connected."
        content.sound = .default
        content.interruptionLevel = .active

        return await scheduleNotification(
            id: "looper-local-alert-test",
            content: content
        )
    }

    func deliverStopNotifications(
        previousSnapshot: MobileSnapshot?,
        currentSnapshot: MobileSnapshot
    ) async {
        guard let previousSnapshot else {
            return
        }

        guard isAuthorized(await currentAuthorizationStatus()) else {
            return
        }

        let previousSessionsByID = Dictionary(
            uniqueKeysWithValues: previousSnapshot.sessions.map { ($0.id, $0) }
        )

        for session in currentSnapshot.sessions {
            guard shouldNotify(for: session, previous: previousSessionsByID[session.id]) else {
                continue
            }

            let content = UNMutableNotificationContent()
            content.title = session.ref
            content.subtitle = "Session stopped"
            content.body = session.title
            content.sound = .default
            content.interruptionLevel = .active
            content.threadIdentifier = session.id
            content.categoryIdentifier = LooperNotificationCategory.sessionStop
            content.userInfo = [
                LooperNotificationPayloadKey.notificationKind: LooperNotificationKind.sessionStop,
                LooperNotificationPayloadKey.sessionId: session.id,
                LooperNotificationPayloadKey.sessionRef: session.ref
            ]

            _ = await scheduleNotification(
                id: stopNotificationIdentifier(for: session),
                content: content
            )
        }
    }

    private func shouldNotify(
        for session: SessionSummary,
        previous: SessionSummary?
    ) -> Bool {
        guard !session.isArchived else {
            return false
        }

        guard session.status == .stopped else {
            return false
        }

        guard let previous else {
            return false
        }

        return previous.status != .stopped && previous.status != .archived
    }

    private func stopNotificationIdentifier(for session: SessionSummary) -> String {
        "looper-stop-\(session.id)-\(session.lastUpdatedAt)"
    }

    private func notificationAction(for quickAction: QuickActionOption) -> UNNotificationAction {
        switch quickAction {
        case .openSession:
            return UNNotificationAction(
                identifier: quickAction.rawValue,
                title: quickAction.label,
                options: [.foreground]
            )
        case .continueChat:
            return UNNotificationAction(
                identifier: quickAction.rawValue,
                title: quickAction.label,
                options: []
            )
        case .reply:
            return UNTextInputNotificationAction(
                identifier: quickAction.rawValue,
                title: quickAction.label,
                options: [.foreground],
                textInputButtonTitle: "Send",
                textInputPlaceholder: "Message"
            )
        case .archive:
            return UNNotificationAction(
                identifier: quickAction.rawValue,
                title: quickAction.label,
                options: [.destructive]
            )
        case .muteSession:
            return UNNotificationAction(
                identifier: quickAction.rawValue,
                title: quickAction.label,
                options: []
            )
        }
    }

    private func scheduleNotification(
        id: String,
        content: UNNotificationContent
    ) async -> Bool {
        let request = UNNotificationRequest(
            identifier: id,
            content: content,
            trigger: nil
        )

        do {
            try await withCheckedThrowingContinuation {
                (continuation: CheckedContinuation<Void, Error>) in
                notificationCenter.add(request) { error in
                    if let error {
                        continuation.resume(throwing: error)
                    } else {
                        continuation.resume(returning: ())
                    }
                }
            }
            return true
        } catch {
            return false
        }
    }

    private func isAuthorized(_ status: UNAuthorizationStatus) -> Bool {
        switch status {
        case .authorized, .ephemeral, .provisional:
            return true
        case .denied, .notDetermined:
            return false
        @unknown default:
            return false
        }
    }
}

final class ForegroundNotificationDelegate: NSObject, @unchecked Sendable, UNUserNotificationCenterDelegate {
    static let shared = ForegroundNotificationDelegate()

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
        _: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        defer { completionHandler() }

        guard
            response.notification.request.content.userInfo[
                LooperNotificationPayloadKey.notificationKind
            ] as? String == LooperNotificationKind.sessionStop,
            let sessionId = response.notification.request.content.userInfo[
                LooperNotificationPayloadKey.sessionId
            ] as? String
        else {
            return
        }

        guard let action = quickAction(from: response) else {
            return
        }

        var userInfo = response.notification.request.content.userInfo
        userInfo[LooperNotificationPayloadKey.action] = action.rawValue
        userInfo[LooperNotificationPayloadKey.sessionId] = sessionId

        if let textResponse = response as? UNTextInputNotificationResponse {
            userInfo[LooperNotificationPayloadKey.prompt] = textResponse.userText
        }

        NotificationCenter.default.post(
            name: .looperDidReceiveSessionQuickAction,
            object: nil,
            userInfo: userInfo
        )
    }

    private nonisolated func quickAction(
        from response: UNNotificationResponse
    ) -> QuickActionOption? {
        switch response.actionIdentifier {
        case UNNotificationDefaultActionIdentifier:
            return .openSession
        case UNNotificationDismissActionIdentifier:
            return nil
        default:
            return QuickActionOption(rawValue: response.actionIdentifier)
        }
    }
}

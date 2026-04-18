import Foundation
import UIKit
import UserNotifications

@MainActor
final class LocalNotificationManager {
    private enum Asset {
        static let notificationLogoName = "LogoOrb"
        static let notificationLogoExtension = "png"
        static let notificationLogoFilenamePrefix = "looper-notification-logo"
    }

    private let notificationCenter: UNUserNotificationCenter

    init(notificationCenter: UNUserNotificationCenter = .current()) {
        self.notificationCenter = notificationCenter
    }

    func currentAuthorizationStatus() async -> UNAuthorizationStatus {
        let settings = await notificationCenter.notificationSettings()
        return settings.authorizationStatus
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
        content.attachments = notificationAttachments()

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
            content.attachments = notificationAttachments()

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

    private func notificationAttachments() -> [UNNotificationAttachment] {
        guard let assetURL = notificationAttachmentFileURL() else {
            return []
        }

        guard let attachment = try? UNNotificationAttachment(
            identifier: Asset.notificationLogoName,
            url: assetURL
        ) else {
            return []
        }

        return [attachment]
    }

    private func notificationAttachmentFileURL() -> URL? {
        let traitCollection = notificationArtworkTraitCollection()

        guard let image = UIImage(
            named: Asset.notificationLogoName,
            in: .main,
            compatibleWith: traitCollection
        ), let pngData = image.pngData() else {
            return nil
        }

        let userInterfaceStyle = traitCollection.userInterfaceStyle == .light ? "light" : "dark"
        let fileURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("\(Asset.notificationLogoFilenamePrefix)-\(userInterfaceStyle)")
            .appendingPathExtension(Asset.notificationLogoExtension)

        do {
            try pngData.write(to: fileURL, options: [.atomic])
            return fileURL
        } catch {
            return nil
        }
    }

    private func notificationArtworkTraitCollection() -> UITraitCollection {
        let currentStyle = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first(where: {
                $0.activationState == .foregroundActive || $0.activationState == .foregroundInactive
            })?
            .traitCollection
            .userInterfaceStyle ?? UIScreen.main.traitCollection.userInterfaceStyle

        let resolvedStyle: UIUserInterfaceStyle = currentStyle == .light ? .light : .dark
        return UITraitCollection(userInterfaceStyle: resolvedStyle)
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
}

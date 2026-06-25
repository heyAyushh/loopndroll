import Foundation
import UIKit
import UserNotifications

private enum NotificationLaunchArgument {
    static let sendTestAlertOnLaunch = "--send-test-alert-on-launch"
}

@MainActor
protocol CompanionNotificationCoordinatorDelegate: AnyObject {
    var notificationService: any CompanionService { get }
    var notificationCanSendLocalNotifications: Bool { get }
    var notificationAreLocalNotificationsDenied: Bool { get }
    var notificationRemotePushRegistration: RemotePushRegistrationResponse? { get }

    func notificationApplyLocalAuthorizationStatus(_ status: UNAuthorizationStatus)
    func notificationSetRemotePushRegistration(_ registration: RemotePushRegistrationResponse?)
    func notificationSetRemotePushRegistrationInFlight(_ isRegistering: Bool)
    func notificationSetRemotePushFailureMessage(_ message: String?)
}

private final class NotificationObserverBag: @unchecked Sendable {
    private let lock = NSLock()
    private var observers: [NSObjectProtocol] = []

    func replace(with nextObservers: [NSObjectProtocol]) {
        lock.lock()
        let oldObservers = observers
        observers = nextObservers
        lock.unlock()

        oldObservers.forEach(NotificationCenter.default.removeObserver)
    }

    deinit {
        lock.lock()
        let oldObservers = observers
        observers = []
        lock.unlock()

        oldObservers.forEach(NotificationCenter.default.removeObserver)
    }
}

@MainActor
final class CompanionNotificationCoordinator {
    private let notificationManager: LocalNotificationManager
    private let remotePushRegistrar: RemotePushRegistrar
    private let launchArguments: [String]
    private weak var delegate: CompanionNotificationCoordinatorDelegate?
    private let notificationObservers = NotificationObserverBag()
    private var didRequestRemotePushRegistrationThisLaunch = false
    private var didSendLaunchVerificationAlertThisLaunch = false

    init(
        notificationManager: LocalNotificationManager,
        remotePushRegistrar: RemotePushRegistrar,
        launchArguments: [String] = ProcessInfo.processInfo.arguments,
        delegate: CompanionNotificationCoordinatorDelegate
    ) {
        self.notificationManager = notificationManager
        self.remotePushRegistrar = remotePushRegistrar
        self.launchArguments = launchArguments
        self.delegate = delegate
        registerNotificationObservers()
    }

    func configureStopQuickActions() {
        notificationManager.configureStopQuickActions(QuickActionSettings.loadSelectedActions())
    }

    func refreshLocalNotificationStatus() async {
        let status = await notificationManager.currentAuthorizationStatus()
        delegate?.notificationApplyLocalAuthorizationStatus(status)
    }

    func enableLocalNotifications() async {
        guard let delegate else {
            return
        }

        configureStopQuickActions()
        #if DEBUG
        if UITestLaunchArguments.isMockModeEnabled {
            delegate.notificationApplyLocalAuthorizationStatus(.authorized)
            Haptics.success()
            return
        }
        #endif

        let authorizationStatus = await notificationManager.requestAuthorizationIfNeeded()
        delegate.notificationApplyLocalAuthorizationStatus(authorizationStatus)

        if delegate.notificationCanSendLocalNotifications {
            Haptics.success()
            await registerForRemoteNotificationsIfPossible(force: true)
        } else if delegate.notificationAreLocalNotificationsDenied {
            Haptics.warning()
        }
    }

    func sendTestAlert() async {
        guard let delegate else {
            return
        }

        if !delegate.notificationCanSendLocalNotifications {
            await enableLocalNotifications()
        }

        guard delegate.notificationCanSendLocalNotifications else {
            return
        }

        if delegate.notificationRemotePushRegistration?.state == .enabled {
            await sendRemoteTestPush()
            return
        }

        await sendLocalTestNotification()
    }

    func sendLaunchVerificationAlertIfRequested() async {
        guard launchArguments.contains(NotificationLaunchArgument.sendTestAlertOnLaunch) else {
            return
        }

        guard !didSendLaunchVerificationAlertThisLaunch else {
            return
        }

        didSendLaunchVerificationAlertThisLaunch = true

        guard let delegate else {
            return
        }

        if !delegate.notificationCanSendLocalNotifications {
            let authorizationStatus = await notificationManager.requestAuthorizationIfNeeded()
            delegate.notificationApplyLocalAuthorizationStatus(authorizationStatus)
        }

        guard delegate.notificationCanSendLocalNotifications else {
            NSLog("looper: launch verification alert skipped because notifications are disabled")
            return
        }

        try? await Task.sleep(for: .seconds(1))

        let didSend = await notificationManager.sendTestNotification()
        NSLog(
            didSend
                ? "looper: launch verification notification scheduled"
                : "looper: launch verification notification failed"
        )
        if didSend {
            Haptics.success()
        } else {
            Haptics.error()
        }
    }

    func registerForRemoteNotificationsInBackground() {
        Task { @MainActor [weak self] in
            await self?.registerForRemoteNotificationsIfPossible()
        }
    }

    private func registerNotificationObservers() {
        let center = NotificationCenter.default
        notificationObservers.replace(with: [
            center.addObserver(
                forName: .looperDidRegisterRemotePush,
                object: nil,
                queue: .main
            ) { [weak self] notification in
                guard let self,
                      let deviceToken = notification.userInfo?["deviceToken"] as? String
                else {
                    return
                }

                Task { @MainActor in
                    await self.registerRemotePushToken(deviceToken)
                }
            },
            center.addObserver(
                forName: .looperDidFailRemotePushRegistration,
                object: nil,
                queue: .main
            ) { [weak self] notification in
                guard let self,
                      let message = notification.userInfo?["message"] as? String
                else {
                    return
                }

                Task { @MainActor in
                    self.delegate?.notificationSetRemotePushRegistrationInFlight(false)
                    self.delegate?.notificationSetRemotePushFailureMessage(message)
                }
            },
        ])
    }

    private func registerForRemoteNotificationsIfPossible(force: Bool = false) async {
        guard let delegate, delegate.notificationCanSendLocalNotifications else {
            return
        }

        guard force || !didRequestRemotePushRegistrationThisLaunch else {
            return
        }

        didRequestRemotePushRegistrationThisLaunch = true
        delegate.notificationSetRemotePushRegistrationInFlight(true)
        delegate.notificationSetRemotePushFailureMessage(nil)
        remotePushRegistrar.registerForRemoteNotifications()
    }

    private func registerRemotePushToken(_ deviceToken: String) async {
        guard let delegate else {
            return
        }

        guard let bundleID = Bundle.main.bundleIdentifier?.trimmingCharacters(
            in: .whitespacesAndNewlines
        ), !bundleID.isEmpty else {
            delegate.notificationSetRemotePushRegistrationInFlight(false)
            delegate.notificationSetRemotePushFailureMessage("The app bundle ID is missing.")
            return
        }

        do {
            let registration = try await delegate.notificationService.registerPushDevice(
                RemotePushRegistrationRequest(
                    installationId: remotePushRegistrar.installationIdentifier(),
                    deviceToken: deviceToken,
                    bundleId: bundleID,
                    environment: .currentBuild,
                    deviceName: UIDevice.current.name
                )
            )
            delegate.notificationSetRemotePushRegistration(registration)
            delegate.notificationSetRemotePushRegistrationInFlight(false)
            delegate.notificationSetRemotePushFailureMessage(nil)
        } catch {
            delegate.notificationSetRemotePushRegistrationInFlight(false)
            delegate.notificationSetRemotePushFailureMessage(error.localizedDescription)
            Haptics.error()
        }
    }

    private func sendRemoteTestPush() async {
        guard let delegate else {
            return
        }

        do {
            let response = try await delegate.notificationService.sendTestPush(
                installationID: remotePushRegistrar.installationIdentifier()
            )
            delegate.notificationSetRemotePushFailureMessage(response.delivered ? nil : response.message)
            if response.delivered {
                Haptics.success()
            } else {
                Haptics.warning()
            }
        } catch {
            delegate.notificationSetRemotePushFailureMessage(error.localizedDescription)
            Haptics.error()
        }
    }

    private func sendLocalTestNotification() async {
        let didSend = await notificationManager.sendTestNotification()
        if didSend {
            Haptics.success()
        } else {
            Haptics.error()
        }
    }
}

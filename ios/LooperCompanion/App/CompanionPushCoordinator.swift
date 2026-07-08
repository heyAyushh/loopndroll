import Foundation
import UserNotifications

@MainActor
protocol CompanionPushCoordinatorDelegate: AnyObject {
    var pushCoordinatorNotificationService: any CompanionService { get }
    var pushCoordinatorCanSendLocalNotifications: Bool { get }
    var pushCoordinatorAreLocalNotificationsDenied: Bool { get }
    var pushCoordinatorRemotePushRegistration: RemotePushRegistrationResponse? { get }

    func pushCoordinatorApplyLocalAuthorizationStatus(_ status: UNAuthorizationStatus)
    func pushCoordinatorSetRemotePushRegistration(_ registration: RemotePushRegistrationResponse?)
    func pushCoordinatorSetRemotePushRegistrationInFlight(_ isRegistering: Bool)
    func pushCoordinatorSetRemotePushFailureMessage(_ message: String?)
}

@MainActor
final class CompanionPushCoordinator {
    private var notificationCoordinator: CompanionNotificationCoordinator?
    private let connection: CompanionConnectionRuntime
    private weak var delegate: CompanionPushCoordinatorDelegate?

    init(
        notificationManager: LocalNotificationManager,
        remotePushRegistrar: RemotePushRegistrar,
        connection: CompanionConnectionRuntime,
        delegate: CompanionPushCoordinatorDelegate
    ) {
        self.connection = connection
        self.delegate = delegate
        notificationCoordinator = CompanionNotificationCoordinator(
            notificationManager: notificationManager,
            remotePushRegistrar: remotePushRegistrar,
            delegate: self
        )
    }

    private var notifications: CompanionNotificationCoordinator {
        guard let notificationCoordinator else {
            preconditionFailure("Notification coordinator used before initialization")
        }
        return notificationCoordinator
    }

    func configureStopQuickActions() {
        notifications.configureStopQuickActions()
    }

    func refreshLocalNotificationStatus() async {
        await notifications.refreshLocalNotificationStatus()
    }

    func enableLocalNotifications() async {
        await notifications.enableLocalNotifications()
    }

    func sendTestAlert() async {
        await notifications.sendTestAlert()
    }

    func sendLaunchVerificationAlertIfRequested() async {
        await notifications.sendLaunchVerificationAlertIfRequested()
    }

    func registerForRemoteNotificationsInBackground() {
        notifications.registerForRemoteNotificationsInBackground()
    }

    @discardableResult
    func drainPendingNotificationReplies(
        submit: @escaping CompanionNotificationReplySubmitter
    ) async -> Bool {
        let drainTask = startNotificationReplyOutboxDrainIfNeeded(submit: submit)
        return await drainTask?.value ?? false
    }

    @discardableResult
    func startNotificationReplyOutboxDrainIfNeeded(
        submit: @escaping CompanionNotificationReplySubmitter
    ) -> Task<Bool, Never>? {
        connection.startNotificationReplyOutboxDrainIfNeeded(submit: submit)
    }

    func stopNotificationReplyOutboxDrain() {
        connection.stopNotificationReplyOutboxDrain()
    }
}

extension CompanionPushCoordinator: CompanionNotificationCoordinatorDelegate {
    var notificationService: any CompanionService {
        guard let delegate else {
            preconditionFailure("Push coordinator used after delegate deallocation")
        }
        return delegate.pushCoordinatorNotificationService
    }

    var notificationCanSendLocalNotifications: Bool {
        delegate?.pushCoordinatorCanSendLocalNotifications ?? false
    }

    var notificationAreLocalNotificationsDenied: Bool {
        delegate?.pushCoordinatorAreLocalNotificationsDenied ?? false
    }

    var notificationRemotePushRegistration: RemotePushRegistrationResponse? {
        delegate?.pushCoordinatorRemotePushRegistration
    }

    func notificationApplyLocalAuthorizationStatus(_ status: UNAuthorizationStatus) {
        delegate?.pushCoordinatorApplyLocalAuthorizationStatus(status)
    }

    func notificationSetRemotePushRegistration(_ registration: RemotePushRegistrationResponse?) {
        delegate?.pushCoordinatorSetRemotePushRegistration(registration)
    }

    func notificationSetRemotePushRegistrationInFlight(_ isRegistering: Bool) {
        delegate?.pushCoordinatorSetRemotePushRegistrationInFlight(isRegistering)
    }

    func notificationSetRemotePushFailureMessage(_ message: String?) {
        delegate?.pushCoordinatorSetRemotePushFailureMessage(message)
    }
}

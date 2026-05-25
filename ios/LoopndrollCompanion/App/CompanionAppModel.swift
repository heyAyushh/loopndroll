import Foundation
import Observation
import UIKit
import UserNotifications

private enum LaunchArgument {
    static let sendTestAlertOnLaunch = "--send-test-alert-on-launch"
}

@MainActor
@Observable
final class CompanionAppModel {
    var configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
    var snapshot: MobileSnapshot?
    var detailBySessionID: [String: SessionDetail] = [:]
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
    var isLoading = false
    var lastUpdatedAt: Date?
    var localNotificationStatus: UNAuthorizationStatus = .notDetermined
    var remotePushRegistration: RemotePushRegistrationResponse?
    var remotePushFailureMessage: String?
    var isRegisteringRemotePush = false

    @ObservationIgnored private var service: any CompanionService
    @ObservationIgnored private let notificationManager: LocalNotificationManager
    @ObservationIgnored private let remotePushRegistrar: RemotePushRegistrar
    @ObservationIgnored private let spotlightIndexer: SessionSpotlightIndexer
    @ObservationIgnored private var notificationObservers: [NSObjectProtocol] = []
    @ObservationIgnored private var spotlightRecordsBySessionID: [String: SessionSpotlightRecord] = [:]
    @ObservationIgnored private var didRequestRemotePushRegistrationThisLaunch = false
    @ObservationIgnored private var didSendLaunchVerificationAlertThisLaunch = false

    init(
        environment: CompanionEnvironment,
        notificationManager: LocalNotificationManager = LocalNotificationManager(),
        remotePushRegistrar: RemotePushRegistrar = .shared,
        spotlightIndexer: SessionSpotlightIndexer = .shared
    ) {
        service = environment.service
        self.notificationManager = notificationManager
        self.remotePushRegistrar = remotePushRegistrar
        self.spotlightIndexer = spotlightIndexer

        if !configuredBaseURL.isEmpty {
            snapshot = CompanionSnapshotCache.load()
        }

        registerNotificationObservers()
    }

    var activeSessions: [SessionSummary] {
        snapshot?.sessions.filter { !$0.isArchived } ?? []
    }

    var runningSessions: [SessionSummary] {
        activeSessions.filter { $0.status == .active }
    }

    var waitingSessions: [SessionSummary] {
        activeSessions.filter { $0.status == .waiting }
    }

    var stoppedSessions: [SessionSummary] {
        activeSessions.filter { $0.status == .stopped }
    }

    var needsAttentionSessions: [SessionSummary] {
        activeSessions.filter { $0.status == .waiting || $0.status == .stopped }
    }

    var archivedSessions: [SessionSummary] {
        snapshot?.sessions.filter(\.isArchived) ?? []
    }

    var sessionsBadgeCount: Int {
        stoppedSessions.count
    }

    var connectivityHeadline: String {
        switch connectionState {
        case .connected:
            return snapshot?.host.name ?? "Connected"
        case .connecting:
            return "Connecting to your Mac"
        case .offline:
            return "Mac connection offline"
        case .unauthorized:
            return "Connection needs approval"
        case .unpaired:
            return "Set up your Mac link"
        }
    }

    var connectivitySummary: String {
        if connectionState == .connected, snapshot != nil {
            return "Connected and ready to monitor sessions."
        }

        return connectionState.summary
    }

    var localNotificationStatusLabel: String {
        switch localNotificationStatus {
        case .authorized:
            return "Allowed"
        case .provisional:
            return "Provisional"
        case .ephemeral:
            return "Temporary"
        case .denied:
            return "Off"
        case .notDetermined:
            return "Not set"
        @unknown default:
            return "Unknown"
        }
    }

    var remotePushStatusLabel: String {
        if isRegisteringRemotePush {
            return "Registering"
        }

        if let remotePushRegistration {
            return remotePushRegistration.state.label
        }

        if remotePushFailureMessage != nil {
            return "Registration failed"
        }

        return canSendLocalNotifications ? "Waiting for APNs" : "Not set"
    }

    var remotePushDetailMessage: String {
        if let remotePushFailureMessage {
            return remotePushFailureMessage
        }

        if let remotePushRegistration {
            return remotePushRegistration.message
        }

        return canSendLocalNotifications
            ? "looper will keep local fallback alerts until APNs is ready on the Mac."
            : "Enable notifications on iPhone to receive stop alerts."
    }

    var shouldUseLocalFallbackNotifications: Bool {
        remotePushRegistration?.state != .enabled
    }

    var canSendLocalNotifications: Bool {
        switch localNotificationStatus {
        case .authorized, .ephemeral, .provisional:
            return true
        case .denied, .notDetermined:
            return false
        @unknown default:
            return false
        }
    }

    var areLocalNotificationsDenied: Bool {
        localNotificationStatus == .denied
    }

    func detail(for sessionID: String) -> SessionDetail? {
        detailBySessionID[sessionID]
    }

    func prepareForActiveState() async {
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        service = CompanionEnvironment.live().service
        await refreshLocalNotificationStatus()

        if snapshot == nil {
            await loadSnapshot()
        } else {
            await refresh()
        }

        await registerForRemoteNotificationsIfPossible()
    }

    func saveConnectionBaseURL(_ value: String) async {
        CompanionConfiguration.storeBaseURLString(value)
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        service = CompanionEnvironment.live().service
        snapshot = nil
        detailBySessionID = [:]
        errorMessage = nil
        CompanionSnapshotCache.clear()
        await loadSnapshot()
    }

    func refreshLocalNotificationStatus() async {
        localNotificationStatus = await notificationManager.currentAuthorizationStatus()
    }

    func enableLocalNotifications() async {
        localNotificationStatus = await notificationManager.requestAuthorizationIfNeeded()

        if canSendLocalNotifications {
            Haptics.success()
            await registerForRemoteNotificationsIfPossible(force: true)
        } else if areLocalNotificationsDenied {
            Haptics.warning()
        }
    }

    func sendTestAlert() async {
        if !canSendLocalNotifications {
            await enableLocalNotifications()
        }

        guard canSendLocalNotifications else {
            return
        }

        if remotePushRegistration?.state == .enabled {
            do {
                let response = try await service.sendTestPush(
                    installationID: remotePushRegistrar.installationIdentifier()
                )
                remotePushFailureMessage = response.delivered ? nil : response.message

                if response.delivered {
                    Haptics.success()
                } else {
                    Haptics.warning()
                }

                return
            } catch {
                remotePushFailureMessage = error.localizedDescription
                Haptics.error()
                return
            }
        }

        let didSend = await notificationManager.sendTestNotification()

        if didSend {
            Haptics.success()
        } else {
            Haptics.error()
        }
    }

    func sendLaunchVerificationAlertIfRequested() async {
        guard ProcessInfo.processInfo.arguments.contains(LaunchArgument.sendTestAlertOnLaunch) else {
            return
        }

        guard !didSendLaunchVerificationAlertThisLaunch else {
            return
        }

        didSendLaunchVerificationAlertThisLaunch = true

        if !canSendLocalNotifications {
            localNotificationStatus = await notificationManager.requestAuthorizationIfNeeded()
        }

        guard canSendLocalNotifications else {
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

    func loadSnapshot() async {
        guard !isLoading else {
            return
        }

        isLoading = true
        errorMessage = nil

        do {
            let nextSnapshot = try await service.loadSnapshot()
            await applySnapshot(nextSnapshot)
        } catch {
            connectionState = error is CompanionConfigurationError ? .unpaired : .offline
            errorMessage = error.localizedDescription
        }

        isLoading = false
    }

    func refresh() async {
        await loadSnapshot()
    }

    func loadSessionDetail(id: String) async {
        if detailBySessionID[id] != nil {
            return
        }

        await refreshSessionDetail(id: id)
    }

    func refreshSessionDetail(id: String) async {
        do {
            detailBySessionID[id] = try await service.loadSessionDetail(id: id)
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    func applyMode(_ preset: SessionMode?, to sessionID: String) async {
        await mutateSnapshot {
            try await service.setSessionMode(id: sessionID, preset: preset)
        }

        if detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID)
        }
    }

    func setSessionArchived(_ archived: Bool, sessionID: String) async {
        await mutateSnapshot {
            try await service.setSessionArchived(id: sessionID, archived: archived)
        }

        if detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID)
        }
    }

    func deleteSession(_ sessionID: String) async {
        await mutateSnapshot {
            try await service.deleteSession(id: sessionID)
        }

        detailBySessionID[sessionID] = nil
    }

    func saveDefaultPrompt(_ defaultPrompt: String) async {
        await mutateSnapshot {
            try await service.saveDefaultPrompt(defaultPrompt)
        }
    }

    private func mutateSnapshot(_ operation: () async throws -> MobileSnapshot) async {
        errorMessage = nil

        do {
            let nextSnapshot = try await operation()
            await applySnapshot(nextSnapshot)
        } catch {
            errorMessage = error.localizedDescription
            Haptics.error()
        }
    }

    private func applySnapshot(_ nextSnapshot: MobileSnapshot) async {
        let previousSnapshot = snapshot
        snapshot = nextSnapshot
        connectionState = .connected
        lastUpdatedAt = Date()
        CompanionSnapshotCache.save(nextSnapshot)
        syncDetailCache(with: nextSnapshot)

        await syncSpotlightIndex(with: nextSnapshot.sessions)

        guard shouldUseLocalFallbackNotifications else {
            return
        }

        await notificationManager.deliverStopNotifications(
            previousSnapshot: previousSnapshot,
            currentSnapshot: nextSnapshot
        )
    }

    private func syncSpotlightIndex(with sessions: [SessionSummary]) async {
        let nextRecords = Dictionary(uniqueKeysWithValues: sessions.map { session in
            (session.id, SessionSpotlightRecord(session: session))
        })
        let removedIDs = Set(spotlightRecordsBySessionID.keys).subtracting(nextRecords.keys)
        let changedSessions = sessions.filter { session in
            nextRecords[session.id] != spotlightRecordsBySessionID[session.id]
        }

        guard !removedIDs.isEmpty || !changedSessions.isEmpty else {
            return
        }

        do {
            if !removedIDs.isEmpty {
                try await spotlightIndexer.deleteSessions(withIDs: Array(removedIDs))
            }

            if !changedSessions.isEmpty {
                try await spotlightIndexer.indexSessions(changedSessions)
            }

            spotlightRecordsBySessionID = nextRecords
        } catch {
            print("Failed to index sessions to Spotlight: \(error)")
        }
    }

    private func syncDetailCache(with nextSnapshot: MobileSnapshot) {
        let sessionIDs = Set(nextSnapshot.sessions.map(\.id))
        detailBySessionID = detailBySessionID.filter { sessionIDs.contains($0.key) }

        for session in nextSnapshot.sessions {
            guard var detail = detailBySessionID[session.id] else {
                continue
            }

            detail.status = session.status
            detail.effectiveMode = session.effectiveMode
            detail.lastUpdatedAt = session.lastUpdatedAt
            detail.assistantPreview = session.assistantPreview
            detail.isArchived = session.isArchived
            detailBySessionID[session.id] = detail
        }
    }

    private func registerNotificationObservers() {
        let center = NotificationCenter.default
        notificationObservers = [
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
                    self.isRegisteringRemotePush = false
                    self.remotePushFailureMessage = message
                }
            }
        ]
    }

    private func registerForRemoteNotificationsIfPossible(force: Bool = false) async {
        guard canSendLocalNotifications else {
            return
        }

        guard force || !didRequestRemotePushRegistrationThisLaunch else {
            return
        }

        didRequestRemotePushRegistrationThisLaunch = true
        isRegisteringRemotePush = true
        remotePushFailureMessage = nil
        remotePushRegistrar.registerForRemoteNotifications()
    }

    private func registerRemotePushToken(_ deviceToken: String) async {
        guard let bundleID = Bundle.main.bundleIdentifier?.trimmingCharacters(
            in: .whitespacesAndNewlines
        ), !bundleID.isEmpty else {
            isRegisteringRemotePush = false
            remotePushFailureMessage = "The app bundle ID is missing."
            return
        }

        do {
            remotePushRegistration = try await service.registerPushDevice(
                RemotePushRegistrationRequest(
                    installationId: remotePushRegistrar.installationIdentifier(),
                    deviceToken: deviceToken,
                    bundleId: bundleID,
                    environment: .currentBuild,
                    deviceName: UIDevice.current.name
                )
            )
            isRegisteringRemotePush = false
            remotePushFailureMessage = nil
        } catch {
            isRegisteringRemotePush = false
            remotePushFailureMessage = error.localizedDescription
            Haptics.error()
        }
    }
}

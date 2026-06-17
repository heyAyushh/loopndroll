import AppIntents
import Foundation
import LooperCompanionCore
import Observation
import UIKit
import UserNotifications

private enum LaunchArgument {
    static let sendTestAlertOnLaunch = "--send-test-alert-on-launch"
}

private enum CachedSnapshotRestoreReason {
    static let appLaunch = "app-launch"
    static let bundledConnectionChange = "bundled-connection-change"
    static let handoffConnectionChange = "handoff-connection-change"
    static let loadFailure = "load-failure"
}

private enum SiriDonationEvent {
    static let openSession = "open-session"
    static let setDefaultSession = "set-default-session"
}

@MainActor
@Observable
final class CompanionAppModel {
    var configuredBaseURL = ""
    var snapshot: MobileSnapshot?
    var serverHealth: CompanionServerHealth?
    var reachedBaseURL: URL?
    var detailBySessionID: [String: SessionDetail] = [:]
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
    var isLoading = false
    var lastUpdatedAt: Date?
    var localNotificationStatus: UNAuthorizationStatus = .notDetermined
    var remotePushRegistration: RemotePushRegistrationResponse?
    var remotePushFailureMessage: String?
    var isRegisteringRemotePush = false
    private(set) var isSavingDefaultPrompt = false
    private(set) var mutatingSessionIDs: Set<String> = []
    var pendingOpenSessionID: String?
    private(set) var sessionSections = SessionSections.empty
    var selectedAssistantSurface = CompanionAssistantSurface.defaultSurface

    @ObservationIgnored private var service: any CompanionService
    @ObservationIgnored private let notificationManager: LocalNotificationManager
    @ObservationIgnored private let remotePushRegistrar: RemotePushRegistrar
    @ObservationIgnored private let spotlightIndexer: SessionSpotlightIndexer
    @ObservationIgnored private var notificationObservers: [NSObjectProtocol] = []
    @ObservationIgnored private var spotlightRecordsBySessionID: [String: SessionSpotlightRecord] = [:]
    @ObservationIgnored private var loadingSessionDetailIDs: Set<String> = []
    @ObservationIgnored private var hasRebuiltSpotlightIndexThisLaunch = false
    @ObservationIgnored private var didRequestRemotePushRegistrationThisLaunch = false
    @ObservationIgnored private var didSendLaunchVerificationAlertThisLaunch = false
    @ObservationIgnored private var mobileEventStreamTask: Task<Void, Never>?
    @ObservationIgnored private var eventStreamRevision = 0
    @ObservationIgnored private var snapshotLoadTask: Task<Void, Never>?
    @ObservationIgnored private var nextSnapshotLoadID = 0
    @ObservationIgnored private var activeSnapshotLoadID = 0
    @ObservationIgnored private var hasUserSelectedAssistantSurface = false
    @ObservationIgnored private var pendingAssistantSurfaceSave: CompanionAssistantSurface?
    @ObservationIgnored private var isSavingAssistantSurface = false
    @ObservationIgnored private var connectionRevision = 0
    @ObservationIgnored private var donatedOpenedSiriSessionIDs: Set<String> = []
    @ObservationIgnored private let spotlightSyncWorker = SpotlightIndexSyncWorker()
    @ObservationIgnored private var didClearSpotlightIndexForCachedSnapshotThisLaunch = false

    init(
        environment: CompanionEnvironment,
        notificationManager: LocalNotificationManager = LocalNotificationManager(),
        remotePushRegistrar: RemotePushRegistrar = .shared,
        spotlightIndexer: SessionSpotlightIndexer = .shared
    ) {
        self.notificationManager = notificationManager
        self.remotePushRegistrar = remotePushRegistrar
        self.spotlightIndexer = spotlightIndexer

        let didActivateBundledConnection = CompanionConfiguration.activateBundledConnectionIfNeeded()
        service = didActivateBundledConnection ? CompanionEnvironment.live().service : environment.service
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        if didActivateBundledConnection {
            CompanionDiagnostics.record("model:bundled-connection-activated")
        }

        if !configuredBaseURL.isEmpty {
            restoreCachedSnapshotIfAvailable(reason: CachedSnapshotRestoreReason.appLaunch)
        }

        configureStopQuickActions()
        registerNotificationObservers()
        CompanionDiagnostics.lifecycle.info(
            "Model initialized baseURL=\(self.configuredBaseURL, privacy: .public) cachedSnapshot=\(self.snapshot != nil, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "model:init baseURL=\(configuredBaseURL) cachedSnapshot=\(snapshot != nil)"
        )
    }

    var activeSessions: [SessionSummary] {
        sessionSections.active
    }

    var runningSessions: [SessionSummary] {
        sessionSections.running
    }

    var waitingSessions: [SessionSummary] {
        sessionSections.waiting
    }

    var stoppedSessions: [SessionSummary] {
        sessionSections.stopped
    }

    var needsAttentionSessions: [SessionSummary] {
        sessionSections.needsAttention
    }

    var archivedSessions: [SessionSummary] {
        sessionSections.archived
    }

    var sessionsBadgeCount: Int {
        sessionSections.needsAttentionCount
    }

    var canSwitchAssistantSurface: Bool {
        snapshot != nil || connectionState == .connected
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
        case .locked:
            return "Unlock looper"
        case .unpaired:
            return "Set up your Mac link"
        }
    }

    var connectivitySummary: String {
        if connectionState == .connected, snapshot != nil {
            if let connectionRoutePresentation {
                return "\(connectionRoutePresentation.title) route at \(connectionRoutePresentation.detail)."
            }

            if let serverHealth, serverHealth.ok {
                return "API running at \(serverHealth.baseURL)."
            }

            return "Connected and ready to monitor sessions."
        }

        return connectionState.summary
    }

    var connectionRoutePresentation: CompanionConnectionRoutePresentation? {
        guard let baseURL = activeConnectionRouteBaseURL else {
            return nil
        }

        return CompanionConnectionRoutePresentation(
            baseURL: baseURL,
            tailscaleDetail: serverHealth?.tailscale?.detailLabel
        )
    }

    var activeConnectionRouteBaseURLString: String? {
        activeConnectionRouteBaseURL?.absoluteString
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

    func isMutatingSession(_ sessionID: String) -> Bool {
        mutatingSessionIDs.contains(sessionID)
    }

    func prepareForActiveState() async {
        if CompanionConfiguration.activateBundledConnectionIfNeeded() {
            CompanionDiagnostics.lifecycle.info("Bundled connection changed during active-state preparation")
            _ = resetConnectionStateForStoredConnection(
                clearsSnapshotCache: false,
                cachedSnapshotRestoreReason: CachedSnapshotRestoreReason.bundledConnectionChange
            )
        }

        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        service = CompanionEnvironment.live().service
        CompanionDiagnostics.lifecycle.info(
            "Preparing active state baseURL=\(self.configuredBaseURL, privacy: .public) hasSnapshot=\(self.snapshot != nil, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "model:prepare baseURL=\(configuredBaseURL) hasSnapshot=\(snapshot != nil)"
        )
        configureStopQuickActions()
        await refreshLocalNotificationStatus()

        if snapshot == nil {
            await loadSnapshot()
        } else {
            await refresh()
        }

        await registerForRemoteNotificationsIfPossible()
        startMobileEventStreamIfNeeded()
    }

    func stopMobileEventStream() {
        eventStreamRevision += 1
        mobileEventStreamTask?.cancel()
        mobileEventStreamTask = nil
    }

    func startMobileEventStreamIfNeeded() {
        guard mobileEventStreamTask == nil else {
            return
        }

        let eventStreamClient = service.makeMobileEventStreamClient()
        eventStreamRevision += 1
        let streamRevision = eventStreamRevision
        let streamConnectionRevision = connectionRevision
        mobileEventStreamTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self else {
                    return
                }
                guard streamRevision == self.eventStreamRevision,
                      streamConnectionRevision == self.connectionRevision
                else {
                    return
                }

                do {
                    try await eventStreamClient.streamEvents { event in
                        await self.handleMobileStreamEvent(
                            event,
                            streamRevision: streamRevision,
                            streamConnectionRevision: streamConnectionRevision
                        )
                    }
                } catch {
                    guard !Task.isCancelled else {
                        return
                    }

                    if !isCancellationError(error) {
                        CompanionDiagnostics.record(
                            "events:stream-error error=\(error.localizedDescription)"
                        )
                    }
                }

                guard !Task.isCancelled,
                      streamRevision == self.eventStreamRevision,
                      streamConnectionRevision == self.connectionRevision
                else {
                    return
                }

                try? await Task.sleep(for: CompanionMetrics.eventStreamReconnectDelay)
            }
        }
    }

    private func handleMobileStreamEvent(
        _ event: MobileStreamEvent,
        streamRevision: Int,
        streamConnectionRevision: Int
    ) async {
        guard streamRevision == eventStreamRevision,
              streamConnectionRevision == connectionRevision
        else {
            CompanionDiagnostics.record("events:stale-skip")
            return
        }

        CompanionDiagnostics.record(
            "events:received type=\(event.eventType.rawValue) thread=\(event.threadID ?? "none")"
        )

        switch event.eventType {
        case .sessionChanged, .promptQueued, .promptDelivered, .lifecycleChanged:
            await refresh()
            guard streamRevision == eventStreamRevision,
                  streamConnectionRevision == connectionRevision
            else {
                CompanionDiagnostics.record("events:stale-detail-skip")
                return
            }
            if let threadID = event.threadID,
               detailBySessionID[threadID] != nil {
                let sessionSurface = snapshot?.assistantSurface(containingSessionID: threadID)
                await refreshSessionDetail(id: threadID, assistantSurface: sessionSurface)
            }
        }
    }

    func saveConnectionBaseURL(_ value: String) async {
        let baseURLs = CompanionConfiguration.normalizedBaseURLsForUserInput(value)
        CompanionConfiguration.storeConnection(
            CompanionConnection(baseURLs: baseURLs, bearerToken: nil)
        )
        await reloadConnection()
    }

    func saveConnection(_ connection: CompanionConnection) async {
        CompanionConfiguration.storeConnection(connection)
        await reloadConnection()
    }

    func setConnectionRoutePreference(_ preference: CompanionConnectionRoutePreference) async {
        let currentConnection = CompanionConfiguration.resolvedConnection()
        let currentPreference = CompanionConfiguration.connectionRoutePreference()
        guard preference != currentPreference else {
            return
        }

        CompanionConfiguration.storeConnectionRoutePreference(preference)
        CompanionConfiguration.storeConnection(
            currentConnection,
            mobileSessionPolicy: .preserveIfBearerTokenUnchanged
        )
        await reloadConnection()
        CompanionDiagnostics.record(
            "connection:route-preference preference=\(preference.rawValue) primary=\(configuredBaseURL)"
        )
    }

    func saveConnectionCode(_ connectionCode: String) async throws {
        let trimmedConnectionCode = connectionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnectionCode.isEmpty else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        let connection = try CompanionConfiguration.resolveConnection(
            fromConnectionCode: trimmedConnectionCode
        )
        await saveConnection(connection)
    }

    func saveConnectionOrbID(_ orbID: String) async throws {
        let resolver = HTTPCompanionService(
            baseURLs: CompanionConfiguration.resolvedBaseURLStrings(),
            bearerToken: nil
        )
        let connectionCode = try await resolver.resolveConnectionCode(orbID: orbID)
        try await saveConnectionCode(connectionCode.code)

        guard connectionState == .connected else {
            throw CompanionConnectionResolutionError.savedConnectionUnavailable(errorMessage)
        }
    }

    private func reloadConnection() async {
        let shouldRestartEventStream = resetConnectionStateForStoredConnection(
            clearsSnapshotCache: true,
            cachedSnapshotRestoreReason: nil
        )
        await loadSnapshot(allowsConcurrentConnectionReload: true)
        if shouldRestartEventStream {
            startMobileEventStreamIfNeeded()
        }
    }

    @discardableResult
    private func resetConnectionStateForStoredConnection(
        clearsSnapshotCache: Bool,
        cachedSnapshotRestoreReason: String?
    ) -> Bool {
        let shouldRestartEventStream = mobileEventStreamTask != nil
        connectionRevision += 1
        cancelSnapshotLoad()
        stopMobileEventStream()
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        service = CompanionEnvironment.live().service
        selectedAssistantSurface = .defaultSurface
        hasUserSelectedAssistantSurface = false
        pendingAssistantSurfaceSave = nil
        isSavingAssistantSurface = false
        serverHealth = nil
        reachedBaseURL = nil
        detailBySessionID = [:]
        pendingOpenSessionID = nil
        errorMessage = nil

        if clearsSnapshotCache {
            CompanionSnapshotCache.clear()
        }

        resetSnapshotState(cachedSnapshotRestoreReason: cachedSnapshotRestoreReason)
        return shouldRestartEventStream
    }

    private func resetSnapshotState(cachedSnapshotRestoreReason: String?) {
        serverHealth = nil
        reachedBaseURL = nil
        detailBySessionID = [:]
        errorMessage = nil
        if let cachedSnapshotRestoreReason,
           restoreCachedSnapshotIfAvailable(reason: cachedSnapshotRestoreReason) {
            return
        }

        snapshot = nil
        sessionSections = .empty
    }

    private func cancelSnapshotLoad() {
        snapshotLoadTask?.cancel()
        snapshotLoadTask = nil
        isLoading = false
    }

    private func nextSnapshotLoadIdentifier() -> Int {
        nextSnapshotLoadID += 1
        activeSnapshotLoadID = nextSnapshotLoadID
        return nextSnapshotLoadID
    }

    private func finishSnapshotLoad(id: Int, loadRevision: Int) {
        guard id == activeSnapshotLoadID else {
            return
        }

        snapshotLoadTask = nil
        if loadRevision == connectionRevision {
            isLoading = false
        }
    }

    func refreshLocalNotificationStatus() async {
        localNotificationStatus = await notificationManager.currentAuthorizationStatus()
    }

    func enableLocalNotifications() async {
        configureStopQuickActions()
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

    func loadSnapshot(allowsConcurrentConnectionReload: Bool = false) async {
        guard !isLoading || allowsConcurrentConnectionReload else {
            CompanionDiagnostics.lifecycle.info("Snapshot load skipped because another load is active")
            return
        }

        snapshotLoadTask?.cancel()
        let loadRevision = connectionRevision
        let loadID = nextSnapshotLoadIdentifier()
        isLoading = true
        errorMessage = nil

        let task = Task { @MainActor [weak self] in
            guard let self else {
                return
            }

            await self.performSnapshotLoad(loadRevision: loadRevision, loadID: loadID)
        }
        snapshotLoadTask = task

        await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
    }

    private func performSnapshotLoad(loadRevision: Int, loadID: Int) async {
        defer {
            finishSnapshotLoad(id: loadID, loadRevision: loadRevision)
        }

        do {
            try Task.checkCancellation()
            CompanionDiagnostics.lifecycle.info(
                "Snapshot load starting baseURL=\(self.configuredBaseURL, privacy: .public)"
            )
            CompanionDiagnostics.record("snapshot:load-start baseURL=\(configuredBaseURL)")
            do {
                let resolvedHealth = try await service.resolveServerHealth()
                guard loadRevision == connectionRevision else {
                    CompanionDiagnostics.record("snapshot:load-stale-health-skip")
                    return
                }
                let health = resolvedHealth.health
                serverHealth = health
                reachedBaseURL = resolvedHealth.reachedBaseURL
                adoptServerHealthBaseURLsIfNeeded(resolvedHealth)
            } catch {
                guard !isCancellationError(error) else {
                    throw error
                }

                guard loadRevision == connectionRevision else {
                    CompanionDiagnostics.record(
                        "snapshot:load-stale-health-error-skip error=\(error.localizedDescription)"
                    )
                    return
                }

                serverHealth = nil
                reachedBaseURL = nil
                CompanionDiagnostics.record(
                    "snapshot:health-load-failed-clear-route error=\(error.localizedDescription)"
                )
            }

            try Task.checkCancellation()
            let nextSnapshot = try await service.loadSnapshot()
            guard loadRevision == connectionRevision else {
                CompanionDiagnostics.record("snapshot:load-stale-skip")
                return
            }
            CompanionDiagnostics.lifecycle.info(
                "Snapshot load succeeded sessions=\(nextSnapshot.sessions.count, privacy: .public)"
            )
            CompanionDiagnostics.record("snapshot:load-success sessions=\(nextSnapshot.sessions.count)")
            await applySnapshot(nextSnapshot)
        } catch {
            guard !isCancellationError(error) else {
                CompanionDiagnostics.lifecycle.info("Snapshot load cancelled")
                CompanionDiagnostics.record("snapshot:load-cancelled")
                return
            }
            guard loadRevision == connectionRevision else {
                CompanionDiagnostics.record("snapshot:load-stale-error-skip error=\(error.localizedDescription)")
                return
            }

            let didRestoreCachedSnapshot = snapshot == nil &&
                restoreCachedSnapshotIfAvailable(reason: CachedSnapshotRestoreReason.loadFailure)
            let hasUsableSnapshot = snapshot != nil
            let nextConnectionState = connectionState(for: error)
            connectionState = nextConnectionState
            clearConnectionRouteStateIfNeeded(for: nextConnectionState)
            errorMessage = shouldSuppressSnapshotLoadError(
                state: nextConnectionState,
                hasUsableSnapshot: hasUsableSnapshot
            ) ? nil : error.localizedDescription
            CompanionDiagnostics.lifecycle.error(
                "Snapshot load failed state=\(nextConnectionState.rawValue, privacy: .public) restoredCache=\(didRestoreCachedSnapshot, privacy: .public) error=\(error.localizedDescription, privacy: .public)"
            )
            CompanionDiagnostics.record(
                "snapshot:load-failed state=\(nextConnectionState.rawValue) restoredCache=\(didRestoreCachedSnapshot) error=\(error.localizedDescription)"
            )
        }
    }

    func refresh() async {
        await loadSnapshot()
    }

    private func adoptServerHealthBaseURLsIfNeeded(_ resolvedHealth: ResolvedCompanionServerHealth) {
        let health = resolvedHealth.health
        let activeTailscaleBaseURL = health.tailscale?.running == true ? health.tailscale?.baseURL : nil
        let advertisedBaseURLValues = [health.baseURL] +
            health.baseURLs +
            [activeTailscaleBaseURL].compactMap(\.self)
        let discoveredBaseURLs = CompanionConfiguration.normalizedBaseURLsForUserInput(
            advertisedBaseURLValues.joined(separator: "\n")
        )
        guard resolvedHealth.reachedBaseURL != nil || !discoveredBaseURLs.isEmpty else {
            return
        }

        let currentConnection = CompanionConfiguration.resolvedConnection()
        let nextBaseURLs = CompanionBaseURLSelection.mergedPreferredBaseURLs(
            reached: resolvedHealth.reachedBaseURL,
            advertised: discoveredBaseURLs,
            existing: currentConnection.baseURLs,
            preference: CompanionConfiguration.connectionRoutePreference()
        )
        guard nextBaseURLs.map(\.absoluteString) != currentConnection.baseURLs.map(\.absoluteString) else {
            return
        }

        CompanionConfiguration.storeConnection(
            CompanionConnection(
                baseURLs: nextBaseURLs,
                bearerToken: currentConnection.bearerToken
            ),
            mobileSessionPolicy: .preserveIfBearerTokenUnchanged
        )
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        service = CompanionEnvironment.live().service
        restartMobileEventStreamIfActive()
        CompanionDiagnostics.record(
            "health:base-urls-adopted count=\(nextBaseURLs.count) primary=\(configuredBaseURL)"
        )
    }

    private var activeConnectionRouteBaseURL: URL? {
        guard connectionState.allowsConnectionRoutePresentation else {
            return nil
        }

        return CompanionConnectionRoutePresentationSelection.activeDisplayBaseURL(
            reachedBaseURL: reachedBaseURL,
            configuredBaseURL: Self.nonEmptyURL(from: configuredBaseURL),
            healthBaseURL: Self.nonEmptyURL(from: serverHealth?.baseURL),
            tailscaleHealthBaseURL: Self.nonEmptyURL(from: serverHealth?.tailscale?.baseURL),
            isTailscaleRunning: serverHealth?.tailscale?.running == true
        )
    }

    private func clearConnectionRouteStateIfNeeded(for state: ConnectivityState) {
        guard !state.allowsConnectionRoutePresentation else {
            return
        }

        serverHealth = nil
        reachedBaseURL = nil
    }

    private static func nonEmptyURL(from value: String?) -> URL? {
        guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !value.isEmpty
        else {
            return nil
        }

        return URL(string: value)
    }

    func continueFromMacActivity(_ activity: NSUserActivity) async {
        guard LooperContinuationActivity.isSupportedActivityType(activity.activityType) else {
            CompanionDiagnostics.record("continuation:model-ignore type=\(activity.activityType)")
            return
        }

        guard let sessionID = LooperContinuationActivity.sessionID(from: activity) else {
            CompanionDiagnostics.lifecycle.info("Continuation activity had no session id; refreshing snapshot")
            CompanionDiagnostics.record("continuation:model-refresh-no-session")
            await refresh()
            return
        }

        adoptHandoffBaseURLIfAvailable(from: activity)
        CompanionDiagnostics.lifecycle.info(
            "Continuation opening session id=\(sessionID, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:model-session id=\(sessionID)")
        await continueFromMacSession(id: sessionID)
    }

    func handleOpenURL(_ url: URL) async {
        if await connectFromMacURL(url) {
            return
        }

        await continueFromMacURL(url)
    }

    func continueFromMacURL(_ url: URL) async {
        guard let sessionID = LooperContinuationActivity.sessionID(from: url) else {
            CompanionDiagnostics.record("continuation:url-ignore url=\(url.absoluteString)")
            return
        }

        adoptHandoffBaseURLIfAvailable(from: url)
        CompanionDiagnostics.lifecycle.info(
            "Continuation URL opening session id=\(sessionID, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:url-session id=\(sessionID)")
        await continueFromMacSession(id: sessionID)
    }

    func continueFromPendingSiriOpenSessionRequest() async {
        guard let request = LooperSiriOpenSessionRequestStore.drain() else {
            return
        }

        let sessionID = request.sessionID.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !sessionID.isEmpty else {
            CompanionDiagnostics.record("siri-open:pending-invalid-session")
            return
        }

        CompanionDiagnostics.record("siri-open:pending-session id=\(sessionID)")
        if snapshot == nil || snapshot?.session(withID: sessionID) == nil {
            await loadSnapshot()
        }

        let sessionSurface = requestedAssistantSurfaceIfAvailable(
            request.assistantSurface,
            sessionID: sessionID
        ) ?? selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
        pendingOpenSessionID = sessionID
        await refreshSessionDetail(id: sessionID, assistantSurface: sessionSurface)
    }

    private func connectFromMacURL(_ url: URL) async -> Bool {
        guard let connectionCode = LooperConnectionDeepLink.connectionCode(from: url) else {
            return false
        }

        do {
            try await saveConnectionCode(connectionCode)
            CompanionDiagnostics.record("connect:url-applied")
        } catch {
            errorMessage = error.localizedDescription
            CompanionDiagnostics.record("connect:url-failed error=\(error.localizedDescription)")
        }
        return true
    }

    private func continueFromMacSession(id sessionID: String) async {
        pendingOpenSessionID = sessionID
        if snapshot == nil || snapshot?.session(withID: sessionID) == nil {
            await loadSnapshot()
        }
        let sessionSurface = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)

        await refreshSessionDetail(id: sessionID, assistantSurface: sessionSurface)
    }

    private func requestedAssistantSurfaceIfAvailable(
        _ requestedSurface: CompanionAssistantSurface?,
        sessionID: String
    ) -> CompanionAssistantSurface? {
        guard let requestedSurface,
              snapshot?.sessions(for: requestedSurface).contains(where: { session in
                  session.id == sessionID && !session.isArchived
              }) == true
        else {
            return nil
        }

        selectedAssistantSurface = requestedSurface
        applyVisibleAssistantSurface(requestedSurface)
        CompanionDiagnostics.record(
            "siri-open:surface-match sessionID=\(sessionID) surface=\(requestedSurface.rawValue)"
        )
        return requestedSurface
    }

    private func adoptHandoffBaseURLIfAvailable(from activity: NSUserActivity) {
        guard let handoffBaseURL = LooperContinuationActivity.baseURL(from: activity) else {
            return
        }

        adoptHandoffBaseURL(handoffBaseURL)
    }

    private func adoptHandoffBaseURLIfAvailable(from url: URL) {
        guard let handoffBaseURL = LooperContinuationActivity.baseURL(from: url) else {
            return
        }

        adoptHandoffBaseURL(handoffBaseURL)
    }

    private func adoptHandoffBaseURL(_ handoffBaseURL: URL) {
        let currentConnection = CompanionConfiguration.resolvedConnection()
        let nextBaseURLs = CompanionBaseURLSelection.mergedPreferredBaseURLs(
            reached: handoffBaseURL,
            advertised: [],
            existing: currentConnection.baseURLs,
            preference: CompanionConfiguration.connectionRoutePreference()
        )
        guard nextBaseURLs.map(\.absoluteString) != currentConnection.baseURLs.map(\.absoluteString) else {
            CompanionDiagnostics.record("handoff:base-url-unchanged baseURL=\(handoffBaseURL.absoluteString)")
            return
        }

        CompanionConfiguration.storeConnection(
            CompanionConnection(
                baseURLs: nextBaseURLs,
                bearerToken: currentConnection.bearerToken
            ),
            mobileSessionPolicy: .preserveIfBearerTokenUnchanged
        )
        let shouldRestartEventStream = resetConnectionStateForStoredConnection(
            clearsSnapshotCache: false,
            cachedSnapshotRestoreReason: CachedSnapshotRestoreReason.handoffConnectionChange
        )
        if shouldRestartEventStream {
            startMobileEventStreamIfNeeded()
        }
        CompanionDiagnostics.lifecycle.info(
            "Handoff adopted baseURL=\(handoffBaseURL.absoluteString, privacy: .public)"
        )
        CompanionDiagnostics.record("handoff:base-url-adopted baseURL=\(handoffBaseURL.absoluteString)")
    }

    private func restartMobileEventStreamIfActive() {
        guard mobileEventStreamTask != nil else {
            return
        }

        stopMobileEventStream()
        startMobileEventStreamIfNeeded()
    }

    func loadSessionDetail(id: String) async {
        if detailBySessionID[id] != nil {
            return
        }

        await refreshSessionDetail(id: id)
    }

    func refreshSessionDetail(
        id: String,
        assistantSurface: CompanionAssistantSurface? = nil
    ) async {
        guard !loadingSessionDetailIDs.contains(id) else {
            return
        }

        let detailLoadRevision = connectionRevision
        loadingSessionDetailIDs.insert(id)
        defer {
            loadingSessionDetailIDs.remove(id)
        }

        var lastError: Error?
        for surface in detailQuerySurfaces(for: id, preferredSurface: assistantSurface) {
            do {
                let detail = try await service.loadSessionDetail(
                    id: id,
                    surface: Optional(surface)
                )
                guard detailLoadRevision == connectionRevision else {
                    CompanionDiagnostics.record("session-detail:stale-skip id=\(id)")
                    return
                }

                detailBySessionID[id] = detail
                lastError = nil
                break
            } catch {
                guard detailLoadRevision == connectionRevision else {
                    CompanionDiagnostics.record("session-detail:stale-error-skip id=\(id)")
                    return
                }

                lastError = error
                CompanionDiagnostics.record(
                    "session-detail:load-failed id=\(id) surface=\(surface.rawValue) error=\(error.localizedDescription)"
                )
            }
        }

        if let lastError {
            errorMessage = lastError.localizedDescription
        }
    }

    private func detailQuerySurfaces(
        for sessionID: String,
        preferredSurface: CompanionAssistantSurface?
    ) -> [CompanionAssistantSurface] {
        var surfaces: [CompanionAssistantSurface] = []
        if let preferredSurface {
            surfaces.append(preferredSurface)
        } else if let detectedSurface = snapshot?.assistantSurface(containingSessionID: sessionID) {
            surfaces.append(detectedSurface)
        }

        if !selectedAssistantSurfaceIn(surfaces) {
            surfaces.append(selectedAssistantSurface)
        }

        surfaces.append(contentsOf: CompanionAssistantSurface.allCases.filter { surface in
            !surfaces.contains(surface)
        })

        return surfaces
    }

    private func selectedAssistantSurfaceIn(_ surfaces: [CompanionAssistantSurface]) -> Bool {
        surfaces.contains(selectedAssistantSurface)
    }

    func applyMode(_ preset: SessionMode?, to sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.setSessionMode(id: sessionID, preset: preset)
        }

        if didMutate, detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID)
        }
    }

    func setSessionArchived(_ archived: Bool, sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.setSessionArchived(id: sessionID, archived: archived)
        }

        if didMutate, detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID)
        }
    }

    func deleteSession(_ sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.deleteSession(id: sessionID)
        }

        if didMutate {
            detailBySessionID[sessionID] = nil
        }
    }

    @discardableResult
    func sendSessionPrompt(_ prompt: String, to sessionID: String) async -> Bool {
        let targetSurface = snapshot?.assistantSurface(containingSessionID: sessionID)
            ?? selectedAssistantSurface

        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPrompt.isEmpty else {
            errorMessage = "Prompt is required."
            Haptics.warning()
            return false
        }

        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.sendSessionPrompt(
                id: sessionID,
                prompt: trimmedPrompt,
                assistantSurface: targetSurface
            )
        }

        if didMutate, detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID, assistantSurface: targetSurface)
        }

        return didMutate
    }

    func muteSession(_ sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.muteSession(id: sessionID)
        }

        if didMutate, detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID)
        }
    }

    func setSiriDefaultSession(_ session: SessionSummary) async {
        let sessionID = session.id
        let targetSurface = assistantSurface(for: sessionID)
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.saveSiriDefaultSession(
                id: sessionID,
                assistantSurface: targetSurface
            )
        }

        if didMutate {
            Haptics.success()
            await donateSetDefaultSiriSession(session)
        }
    }

    func donateOpenedSiriSession(_ session: SessionSummary) async {
        let sessionID = session.id
        guard !donatedOpenedSiriSessionIDs.contains(sessionID) else {
            return
        }

        let intent = OpenLooperSessionIntent()
        intent.target = siriSessionEntity(for: session)
        let didDonate = await donateSiriIntent(
            intent,
            event: SiriDonationEvent.openSession,
            sessionID: sessionID
        )

        if didDonate {
            donatedOpenedSiriSessionIDs.insert(sessionID)
        }
    }

    func markCurrentSiriSession(_ session: SessionSummary) async {
        let sessionID = session.id
        let targetSurface = assistantSurface(for: sessionID)
        guard snapshot?.globalSettings.siriCurrentSessionId != sessionID ||
            snapshot?.globalSettings.siriCurrentAssistantSurface != targetSurface
        else {
            return
        }

        let mutationRevision = connectionRevision
        do {
            let nextSnapshot = try await service.saveSiriCurrentSession(
                id: sessionID,
                assistantSurface: targetSurface
            )
            guard mutationRevision == connectionRevision else {
                CompanionDiagnostics.record("siri-current:stale-skip sessionID=\(sessionID)")
                return
            }

            await applySnapshot(nextSnapshot)
        } catch {
            guard !isCancellationError(error) else {
                return
            }

            CompanionDiagnostics.record(
                "siri-current:update-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
        }
    }

    func siriAssistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        assistantSurface(for: sessionID)
    }

    func performQuickAction(
        _ action: QuickActionOption,
        sessionID: String,
        prompt: String? = nil
    ) async {
        switch action {
        case .openSession:
            if snapshot == nil || snapshot?.session(withID: sessionID) == nil {
                await loadSnapshot()
            }
            let sessionSurface = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
            pendingOpenSessionID = sessionID
            await refreshSessionDetail(id: sessionID, assistantSurface: sessionSurface)
        case .continueChat:
            if snapshot == nil {
                await loadSnapshot()
            }
            await sendSessionPrompt(snapshot?.globalSettings.defaultPrompt ?? "", to: sessionID)
        case .reply:
            await sendSessionPrompt(prompt ?? "", to: sessionID)
        case .archive:
            await setSessionArchived(true, sessionID: sessionID)
        case .muteSession:
            await muteSession(sessionID)
        }
    }

    func configureStopQuickActions() {
        notificationManager.configureStopQuickActions(QuickActionSettings.loadSelectedActions())
    }

    func consumePendingOpenSessionID() -> String? {
        let sessionID = pendingOpenSessionID
        pendingOpenSessionID = nil
        return sessionID
    }

    func saveDefaultPrompt(_ defaultPrompt: String) async {
        guard !isSavingDefaultPrompt else {
            return
        }

        isSavingDefaultPrompt = true
        defer {
            isSavingDefaultPrompt = false
        }

        await mutateSnapshot {
            try await service.saveDefaultPrompt(defaultPrompt)
        }
    }

    func selectAssistantSurface(_ surface: CompanionAssistantSurface) {
        guard selectedAssistantSurface != surface else {
            return
        }

        hasUserSelectedAssistantSurface = true
        selectedAssistantSurface = surface
        applyVisibleAssistantSurface(surface)
        pendingAssistantSurfaceSave = surface
        startAssistantSurfaceSaveIfNeeded()
    }

    private func mutateSessionSnapshot(
        sessionID: String,
        _ operation: () async throws -> MobileSnapshot
    ) async -> Bool {
        guard !mutatingSessionIDs.contains(sessionID) else {
            return false
        }

        setSessionMutation(true, sessionID: sessionID)
        defer {
            setSessionMutation(false, sessionID: sessionID)
        }

        return await mutateSnapshot(operation)
    }

    @discardableResult
    private func mutateSnapshot(_ operation: () async throws -> MobileSnapshot) async -> Bool {
        let mutationRevision = connectionRevision
        errorMessage = nil

        do {
            let nextSnapshot = try await operation()
            guard mutationRevision == connectionRevision else {
                CompanionDiagnostics.record("snapshot:mutation-stale-skip")
                return false
            }

            await applySnapshot(nextSnapshot)
            return true
        } catch {
            guard mutationRevision == connectionRevision else {
                CompanionDiagnostics.record("snapshot:mutation-stale-error-skip error=\(error.localizedDescription)")
                return false
            }

            connectionState = connectionState(for: error)
            clearConnectionRouteStateIfNeeded(for: connectionState)
            errorMessage = error.localizedDescription
            Haptics.error()
            return false
        }
    }

    private func setSessionMutation(_ isMutating: Bool, sessionID: String) {
        var nextMutatingSessionIDs = mutatingSessionIDs
        if isMutating {
            nextMutatingSessionIDs.insert(sessionID)
        } else {
            nextMutatingSessionIDs.remove(sessionID)
        }
        mutatingSessionIDs = nextMutatingSessionIDs
    }

    private func connectionState(for error: Error) -> ConnectivityState {
        if error is CompanionConfigurationError {
            return .unpaired
        }

        if let httpError = error as? HTTPCompanionServiceError {
            switch httpError {
            case .unauthorized:
                return .unauthorized
            case .passkeySessionRequired:
                return .locked
            case .invalidResponse, .serverError:
                return .connected
            }
        }

        return .offline
    }

    private func isCancellationError(_ error: Error) -> Bool {
        if error is CancellationError {
            return true
        }

        let nsError = error as NSError
        return nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled
    }

    private func shouldSuppressSnapshotLoadError(
        state: ConnectivityState,
        hasUsableSnapshot: Bool
    ) -> Bool {
        hasUsableSnapshot && state == .offline
    }

    @discardableResult
    private func restoreCachedSnapshotIfAvailable(reason: String) -> Bool {
        guard let cachedSnapshot = CompanionSnapshotCache.load() else {
            return false
        }

        applyCachedSnapshot(cachedSnapshot, reason: reason)
        return true
    }

    private func applyCachedSnapshot(_ cachedSnapshot: MobileSnapshot, reason: String) {
        let visibleSnapshot = applySnapshotState(
            cachedSnapshot,
            preferredSurface: cachedSnapshot.globalSettings.assistantSurface
        )
        lastUpdatedAt = Date()
        syncDetailCache(with: visibleSnapshot)
        clearSpotlightIndexForCachedSnapshot()
        CompanionDiagnostics.record(
            "snapshot:cache-restore reason=\(reason) sessions=\(visibleSnapshot.sessions.count)"
        )
    }

    private func applySnapshot(_ nextSnapshot: MobileSnapshot) async {
        let previousSnapshot = snapshot
        let visibleSnapshot = applySnapshotState(
            nextSnapshot,
            preferredSurface: preferredAssistantSurface(for: nextSnapshot)
        )
        connectionState = .connected
        lastUpdatedAt = Date()
        CompanionSnapshotCache.save(visibleSnapshot)
        syncDetailCache(with: visibleSnapshot)

        syncSpotlightIndex(with: visibleSnapshot.sessionsAcrossSurfaces)

        guard shouldUseLocalFallbackNotifications else {
            return
        }

        await notificationManager.deliverStopNotifications(
            previousSnapshot: previousSnapshot,
            currentSnapshot: visibleSnapshot
        )
    }

    private func applyVisibleAssistantSurface(_ surface: CompanionAssistantSurface) {
        guard let snapshot else {
            sessionSections = .empty
            return
        }

        let visibleSnapshot = applySnapshotState(snapshot, preferredSurface: surface)
        syncDetailCache(with: visibleSnapshot)
    }

    @discardableResult
    private func applySnapshotState(
        _ nextSnapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface
    ) -> MobileSnapshot {
        let visibleSnapshot = nextSnapshot.visibleSnapshot(for: preferredSurface)
        selectedAssistantSurface = preferredSurface
        snapshot = visibleSnapshot
        sessionSections = SessionSections(sessions: visibleSnapshot.sessions)
        return visibleSnapshot
    }

    private func preferredAssistantSurface(
        for nextSnapshot: MobileSnapshot
    ) -> CompanionAssistantSurface {
        if hasUserSelectedAssistantSurface {
            return selectedAssistantSurface
        }

        return nextSnapshot.globalSettings.assistantSurface
    }

    private func startAssistantSurfaceSaveIfNeeded() {
        guard !isSavingAssistantSurface else {
            return
        }

        isSavingAssistantSurface = true
        Task { @MainActor in
            await persistPendingAssistantSurfaces()
        }
    }

    private func persistPendingAssistantSurfaces() async {
        while let nextAssistantSurface = pendingAssistantSurfaceSave {
            pendingAssistantSurfaceSave = nil

            do {
                let nextSnapshot = try await service.saveAssistantSurface(nextAssistantSurface)
                await applySnapshot(nextSnapshot)
            } catch {
                guard !isCancellationError(error) else {
                    continue
                }

                handleAssistantSurfaceSaveFailure(error)
            }
        }

        isSavingAssistantSurface = false
        if pendingAssistantSurfaceSave != nil {
            startAssistantSurfaceSaveIfNeeded()
        }
    }

    private func handleAssistantSurfaceSaveFailure(_ error: Error) {
        let nextConnectionState = connectionState(for: error)
        connectionState = nextConnectionState
        clearConnectionRouteStateIfNeeded(for: nextConnectionState)
        errorMessage = shouldSuppressSnapshotLoadError(
            state: nextConnectionState,
            hasUsableSnapshot: snapshot != nil
        ) ? nil : error.localizedDescription
        CompanionDiagnostics.record(
            "assistant-surface:save-failed surface=\(selectedAssistantSurface.rawValue) state=\(nextConnectionState.rawValue) error=\(error.localizedDescription)"
        )
    }

    private func selectAssistantSurfaceContainingSessionIfAvailable(_ sessionID: String) -> CompanionAssistantSurface? {
        guard let surface = snapshot?.assistantSurface(containingSessionID: sessionID) else {
            CompanionDiagnostics.record("continuation:surface-miss sessionID=\(sessionID)")
            return nil
        }

        selectedAssistantSurface = surface
        applyVisibleAssistantSurface(surface)
        CompanionDiagnostics.record(
            "continuation:surface-match sessionID=\(sessionID) surface=\(surface.rawValue)"
        )
        return surface
    }

    private func assistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        snapshot?.assistantSurface(containingSessionID: sessionID) ?? selectedAssistantSurface
    }

    private func siriSessionEntity(for session: SessionSummary) -> LooperSessionEntity {
        LooperSessionEntity(
            session: session,
            assistantSurface: siriAssistantSurface(for: session.id)
        )
    }

    private func donateSetDefaultSiriSession(_ session: SessionSummary) async {
        let intent = SetDefaultLooperSessionIntent()
        intent.session = siriSessionEntity(for: session)
        _ = await donateSiriIntent(
            intent,
            event: SiriDonationEvent.setDefaultSession,
            sessionID: session.id
        )
    }

    private func donateSiriIntent(
        _ intent: some AppIntent,
        event: String,
        sessionID: String
    ) async -> Bool {
        do {
            try await IntentDonationManager.shared.donate(intent: intent)
            CompanionDiagnostics.record("siri-donation:\(event) sessionID=\(sessionID)")
            return true
        } catch {
            CompanionDiagnostics.record(
                "siri-donation:\(event)-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func syncSpotlightIndex(with sessions: [SessionSummary]) {
        let indexableSessions = SessionSpotlightIndexingPolicy.indexableSessions(from: sessions)
        let nextRecords = Dictionary(uniqueKeysWithValues: indexableSessions.map { session in
            (session.id, SessionSpotlightRecord(session: session))
        })
        let removedIDs = Set(spotlightRecordsBySessionID.keys).subtracting(nextRecords.keys)
        let removedSearchableIDs = removedIDs.flatMap { sessionID in
            spotlightRecordsBySessionID[sessionID]?.searchableIdentifiers ?? [sessionID]
        }
        let changedSessions = indexableSessions.filter { session in
            nextRecords[session.id] != spotlightRecordsBySessionID[session.id]
        }
        let shouldRebuildIndex = !hasRebuiltSpotlightIndexThisLaunch

        guard shouldRebuildIndex || !removedIDs.isEmpty || !changedSessions.isEmpty else {
            return
        }

        spotlightRecordsBySessionID = nextRecords
        hasRebuiltSpotlightIndexThisLaunch = true
        let spotlightIndexer = spotlightIndexer
        let spotlightSyncWorker = spotlightSyncWorker

        Task.detached(priority: .utility) {
            await spotlightSyncWorker.syncSessions(
                indexer: spotlightIndexer,
                rebuildsIndex: shouldRebuildIndex,
                removedSearchableIDs: removedSearchableIDs,
                changedSessions: changedSessions,
                indexableSessions: indexableSessions
            )
        }
    }

    private func clearSpotlightIndexForCachedSnapshot() {
        guard !didClearSpotlightIndexForCachedSnapshotThisLaunch ||
            hasRebuiltSpotlightIndexThisLaunch ||
            !spotlightRecordsBySessionID.isEmpty
        else {
            return
        }

        didClearSpotlightIndexForCachedSnapshotThisLaunch = true
        spotlightRecordsBySessionID = [:]
        hasRebuiltSpotlightIndexThisLaunch = false
        let spotlightIndexer = spotlightIndexer
        let spotlightSyncWorker = spotlightSyncWorker

        Task.detached(priority: .utility) {
            await spotlightSyncWorker.clearSessions(indexer: spotlightIndexer)
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
            detail.lastActivityAt = session.lastActivityAt
            detail.lastMessageAt = session.lastMessageAt
            detail.assistantPreview = session.assistantPreview
            detail.isArchived = session.isArchived
            detail.metadata = session.metadata
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
            },
            center.addObserver(
                forName: .looperDidReceiveSessionQuickAction,
                object: nil,
                queue: .main
            ) { [weak self] notification in
                guard let self,
                      let sessionID = notification.userInfo?[
                          LooperNotificationPayloadKey.sessionId
                      ] as? String,
                      let actionValue = notification.userInfo?[
                          LooperNotificationPayloadKey.action
                      ] as? String,
                      let action = QuickActionOption(rawValue: actionValue)
                else {
                    return
                }

                let prompt = notification.userInfo?[LooperNotificationPayloadKey.prompt] as? String
                Task { @MainActor in
                    await self.performQuickAction(action, sessionID: sessionID, prompt: prompt)
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

private actor SpotlightIndexSyncWorker {
    func clearSessions(indexer: SessionSpotlightIndexer) async {
        do {
            try await indexer.deleteAllSessions()
        } catch {
            print("Failed to clear cached sessions from Spotlight: \(error)")
        }
    }

    func syncSessions(
        indexer: SessionSpotlightIndexer,
        rebuildsIndex: Bool,
        removedSearchableIDs: [String],
        changedSessions: [SessionSummary],
        indexableSessions: [SessionSummary]
    ) async {
        do {
            if rebuildsIndex {
                try await indexer.deleteAllSessions()
            } else if !removedSearchableIDs.isEmpty {
                try await indexer.deleteSessions(withIDs: removedSearchableIDs)
            }

            let sessionsToIndex = rebuildsIndex ? indexableSessions : changedSessions
            if !sessionsToIndex.isEmpty {
                try await indexer.indexSessions(sessionsToIndex)
            }
        } catch {
            print("Failed to index sessions to Spotlight: \(error)")
        }
    }
}

private enum CompanionConnectionResolutionError: LocalizedError {
    case savedConnectionUnavailable(String?)

    var errorDescription: String? {
        switch self {
        case let .savedConnectionUnavailable(message):
            return message ?? "The orb was accepted, but looper is not reachable yet."
        }
    }
}

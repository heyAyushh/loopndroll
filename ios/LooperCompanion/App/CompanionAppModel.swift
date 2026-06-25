import AppIntents
import Foundation
import LooperCompanionCore
import LooperRealtime
import Observation
import UserNotifications

private enum CachedSnapshotRestoreReason {
    static let appLaunch = "app-launch"
    static let bundledConnectionChange = "bundled-connection-change"
    static let storedConnectionChange = "stored-connection-change"
    static let handoffConnectionChange = "handoff-connection-change"
    static let loadFailure = "load-failure"
}

private enum SiriDonationEvent {
    static let openSession = "open-session"
    static let setDefaultSession = "set-default-session"
}

private enum PromptDispatchFailure {
    static let resumeFailedDetailPrefix = "prompt-resume-failed:"
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
    var pendingSettingsTarget: SettingsSearchTarget?
    private(set) var sessionSections = SessionSections.empty
    var selectedAssistantSurface = CompanionAssistantSurface.defaultSurface
    private(set) var sessionIndex = SessionIndex.empty

    @ObservationIgnored private var service: any CompanionService
    @ObservationIgnored private let reloadsServiceFromStoredConnection: Bool
    @ObservationIgnored private let notificationManager: LocalNotificationManager
    @ObservationIgnored private let spotlightIndexer: SessionSpotlightIndexer
    @ObservationIgnored private let sessionMiniController: CompanionSessionMiniController
    @ObservationIgnored private var connectionCoordinator: CompanionConnectionCoordinator?
    @ObservationIgnored private var notificationCoordinator: CompanionNotificationCoordinator?
    @ObservationIgnored private var notificationReplyCoordinator: CompanionNotificationReplyCoordinator?
    @ObservationIgnored private var snapshotLoadCoordinator: CompanionSnapshotLoadCoordinator?
    @ObservationIgnored private var sessionMutationCoordinator: CompanionSessionMutationCoordinator?
    @ObservationIgnored private var spotlightRecordsBySessionID: [String: SessionSpotlightRecord] = [:]
    @ObservationIgnored private var loadingSessionDetailIDs: Set<String> = []
    @ObservationIgnored private var hasRebuiltSpotlightIndexThisLaunch = false
    @ObservationIgnored private var lastAppliedRealtimeRevision: String?
    @ObservationIgnored private var hasValidatedCurrentSnapshotWithHTTP = false
    @ObservationIgnored private var hasUserSelectedAssistantSurface = false
    @ObservationIgnored private var pendingAssistantSurfaceSave: CompanionAssistantSurface?
    @ObservationIgnored private var isSavingAssistantSurface = false
    @ObservationIgnored private var connectionRevision = 0
    @ObservationIgnored private var activeServiceConnectionFingerprint = ""
    @ObservationIgnored private var donatedOpenedSiriSessionIDs: Set<String> = []
    @ObservationIgnored private let spotlightSyncWorker = SpotlightIndexSyncWorker()
    @ObservationIgnored private var didClearSpotlightIndexForCachedSnapshotThisLaunch = false

    init(
        environment: CompanionEnvironment,
        notificationManager: LocalNotificationManager = LocalNotificationManager(),
        remotePushRegistrar: RemotePushRegistrar = .shared,
        spotlightIndexer: SessionSpotlightIndexer = .shared,
        sessionMiniLocalStore: CompanionSessionMiniLocalStore? = CompanionSessionMiniLocalStore.liveDefault()
    ) {
        reloadsServiceFromStoredConnection = environment.reloadsServiceFromStoredConnection
        self.notificationManager = notificationManager
        self.spotlightIndexer = spotlightIndexer
        self.sessionMiniController = CompanionSessionMiniController(localStore: sessionMiniLocalStore)

        let didActivateBundledConnection = reloadsServiceFromStoredConnection &&
            CompanionConfiguration.activateBundledConnectionIfNeeded()
        service = didActivateBundledConnection ? CompanionEnvironment.live().service : environment.service
        connectionCoordinator = CompanionConnectionCoordinator(delegate: self)
        notificationCoordinator = CompanionNotificationCoordinator(
            notificationManager: notificationManager,
            remotePushRegistrar: remotePushRegistrar,
            delegate: self
        )
        notificationReplyCoordinator = CompanionNotificationReplyCoordinator(
            sessionMiniController: sessionMiniController,
            delegate: self
        )
        snapshotLoadCoordinator = CompanionSnapshotLoadCoordinator(delegate: self)
        sessionMutationCoordinator = CompanionSessionMutationCoordinator(
            commandStore: sessionMiniController,
            delegate: self
        )
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
        if didActivateBundledConnection {
            CompanionDiagnostics.record("model:bundled-connection-activated")
        }

        let didRestoreSessionMiniSnapshot = restoreCachedSessionMiniSnapshotIfAvailable(
            reason: CachedSnapshotRestoreReason.appLaunch
        )
        let didScheduleCachedSnapshotRestore = !didRestoreSessionMiniSnapshot && !configuredBaseURL.isEmpty
        if didScheduleCachedSnapshotRestore {
            snapshotLoads.scheduleCachedSnapshotRestoreIfAvailable(reason: CachedSnapshotRestoreReason.appLaunch)
        }

        configureStopQuickActions()
        SessionQuickActionCenter.shared.configureLocalStore(sessionMiniController.localStore)
        registerSessionQuickActionHandler()
        CompanionDiagnostics.lifecycle.info(
            "Model initialized baseURL=\(self.configuredBaseURL, privacy: .public) cachedSnapshotRestoreScheduled=\(didScheduleCachedSnapshotRestore, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "model:init baseURL=\(configuredBaseURL) cachedSnapshotRestoreScheduled=\(didScheduleCachedSnapshotRestore)"
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

    private var sessionMutations: CompanionSessionMutationCoordinator {
        guard let sessionMutationCoordinator else {
            preconditionFailure("Session mutation coordinator used before initialization")
        }
        return sessionMutationCoordinator
    }

    private var connectionActions: CompanionConnectionCoordinator {
        guard let connectionCoordinator else {
            preconditionFailure("Connection coordinator used before initialization")
        }
        return connectionCoordinator
    }

    private var notifications: CompanionNotificationCoordinator {
        guard let notificationCoordinator else {
            preconditionFailure("Notification coordinator used before initialization")
        }
        return notificationCoordinator
    }

    private var notificationReplies: CompanionNotificationReplyCoordinator {
        guard let notificationReplyCoordinator else {
            preconditionFailure("Notification reply coordinator used before initialization")
        }
        return notificationReplyCoordinator
    }

    private var snapshotLoads: CompanionSnapshotLoadCoordinator {
        guard let snapshotLoadCoordinator else {
            preconditionFailure("Snapshot load coordinator used before initialization")
        }
        return snapshotLoadCoordinator
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
            return connectedStatusSummary
        }

        return connectionState.summary
    }

    private var connectedStatusSummary: String {
        var parts: [String] = []

        if let connectionRoutePresentation {
            parts.append("\(connectionRoutePresentation.title) route at \(connectionRoutePresentation.detail)")
        } else if let serverHealth, serverHealth.ok {
            parts.append("API running at \(serverHealth.baseURL)")
        }

        if let workSummary = snapshot?.workStatus.displaySummary {
            parts.append(workSummary)
        }

        if let coverageSummary = snapshot?.workStatus.coverageSummary {
            parts.append(coverageSummary)
        }

        if let lastSyncedAt = snapshot?.host.lastSyncedAt, !lastSyncedAt.isEmpty {
            parts.append("synced \(ModelFormatting.relativeTimestamp(lastSyncedAt))")
        }

        return parts.isEmpty ? "Connected and ready to monitor sessions." : "\(parts.joined(separator: " · "))."
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
        let didResetConnectionForActiveState: Bool
        let didActivateBundledConnection = reloadsServiceFromStoredConnection &&
            CompanionConfiguration.activateBundledConnectionIfNeeded()
        if reloadsServiceFromStoredConnection,
           didActivateBundledConnection || shouldReloadServiceFromStoredConnection() {
            CompanionDiagnostics.lifecycle.info("Stored connection changed during active-state preparation")
            _ = await resetConnectionStateForStoredConnection(
                clearsSnapshotCache: false,
                cachedSnapshotRestoreReason: didActivateBundledConnection
                    ? CachedSnapshotRestoreReason.bundledConnectionChange
                    : CachedSnapshotRestoreReason.storedConnectionChange
            )
            didResetConnectionForActiveState = true
        } else {
            didResetConnectionForActiveState = false
        }

        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        CompanionDiagnostics.lifecycle.info(
            "Preparing active state baseURL=\(self.configuredBaseURL, privacy: .public) hasSnapshot=\(self.snapshot != nil, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "model:prepare baseURL=\(configuredBaseURL) hasSnapshot=\(snapshot != nil)"
        )
        configureStopQuickActions()
        await refreshLocalNotificationStatus()
        if !didResetConnectionForActiveState {
            prepareRealtimeConnectionInBackground()
        }
        startRealtimeSessionSyncIfNeeded()
        startNotificationReplyOutboxDrainIfNeeded()

        CompanionDiagnostics.record("snapshot:load-skip-state-mini-prepare")
        notifications.registerForRemoteNotificationsInBackground()
    }

    private func shouldReloadServiceFromStoredConnection() -> Bool {
        CompanionConfiguration.resolvedConnectionFingerprint() != activeServiceConnectionFingerprint
    }

    func stopRealtimeSessionSync() {
        stopRealtimeSessionSync(disconnectCachedClients: true)
    }

    private func stopRealtimeSessionSync(disconnectCachedClients: Bool) {
        sessionMiniController.stopSync()
        if disconnectCachedClients {
            Task {
                await RealtimeCompanionClientFactory.disconnectCachedClients()
            }
        }
    }

    func startRealtimeSessionSyncIfNeeded() {
        sessionMiniController.startSyncIfNeeded(
            service: service,
            connectionRevision: connectionRevision
        ) { [weak self] update, connectionRevision in
            self?.applySessionMiniSyncUpdate(
                update,
                connectionRevision: connectionRevision
            )
        }
    }

    #if DEBUG
    func runSessionMiniSyncCycleForSelfTest(
        transport: any LooperRealtimeStateMiniSyncTransport
    ) async -> LooperRealtimeStateMiniSyncCycleResult {
        await sessionMiniController.runSyncCycleForSelfTest(
            transport: transport,
            connectionRevision: connectionRevision
        ) { [weak self] update, connectionRevision in
            self?.applySessionMiniSyncUpdate(
                update,
                connectionRevision: connectionRevision
            )
        }
    }
    #endif

    private func applySessionMiniSyncUpdate(
        _ update: LooperRealtimeStateMiniSyncUpdate,
        connectionRevision: Int
    ) {
        guard connectionRevision == self.connectionRevision else {
            CompanionDiagnostics.record("session-mini:sync-stale-skip")
            return
        }

        do {
            guard let cachedSnapshot = try sessionMiniController.cachedSnapshot() else {
                return
            }

            applyCachedSnapshot(cachedSnapshot, reason: "session-mini-sync-\(update.reason.rawValue)")
            connectionState = .connected
            lastUpdatedAt = Date()
            CompanionDiagnostics.record(
                "session-mini:sync-applied reason=\(update.reason.rawValue) seq=\(update.snapshot.latestSeq)"
            )
        } catch {
            CompanionDiagnostics.record(
                "session-mini:sync-apply-failed reason=\(update.reason.rawValue) error=\(error.localizedDescription)"
            )
        }
    }

    private func prepareRealtimeConnectionInBackground() {
        let service = service
        Task.detached(priority: .userInitiated) {
            await service.prepareRealtimeConnection()
        }
    }

    private func startSnapshotLoadInBackground(allowsConcurrentConnectionReload: Bool) {
        Task { @MainActor [weak self] in
            await self?.loadSnapshot(
                allowsConcurrentConnectionReload: allowsConcurrentConnectionReload
            )
        }
    }

    func saveConnectionBaseURL(_ value: String) async {
        await connectionActions.saveBaseURL(value)
    }

    func saveConnection(_ connection: CompanionConnection) async {
        await connectionActions.saveConnection(connection)
    }

    func setConnectionRoutePreference(_ preference: CompanionConnectionRoutePreference) async {
        await connectionActions.setRoutePreference(preference)
    }

    func saveConnectionCode(_ connectionCode: String) async throws {
        try await connectionActions.saveConnectionCode(connectionCode)
    }

    func saveConnectionOrbID(_ orbID: String) async throws {
        try await connectionActions.saveConnectionOrbID(orbID)
    }

    private func reloadConnection() async {
        let shouldRestartEventStream = await resetConnectionStateForStoredConnection(
            clearsSnapshotCache: true,
            cachedSnapshotRestoreReason: nil
        )
        if shouldRestartEventStream {
            startRealtimeSessionSyncIfNeeded()
        }
        await loadSnapshot(allowsConcurrentConnectionReload: true)
    }

    @discardableResult
    private func resetConnectionStateForStoredConnection(
        clearsSnapshotCache: Bool,
        cachedSnapshotRestoreReason: String?
    ) async -> Bool {
        let shouldRestartEventStream = sessionMiniController.isSyncing
        connectionRevision += 1
        snapshotLoads.cancelCachedSnapshotRestore()
        snapshotLoads.cancelSnapshotLoad()
        stopRealtimeSessionSync(disconnectCachedClients: false)
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        selectedAssistantSurface = .defaultSurface
        hasUserSelectedAssistantSurface = false
        pendingAssistantSurfaceSave = nil
        isSavingAssistantSurface = false
        await sessionMutations.cancelAllModeMutations(resolveAs: false)
        stopNotificationReplyOutboxDrain()
        lastAppliedRealtimeRevision = nil
        hasValidatedCurrentSnapshotWithHTTP = false
        serverHealth = nil
        reachedBaseURL = nil
        detailBySessionID = [:]
        pendingOpenSessionID = nil
        errorMessage = nil

        if clearsSnapshotCache {
            CompanionSnapshotCache.clear()
        }

        if reloadsServiceFromStoredConnection {
            configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
            service = CompanionEnvironment.live().service
            activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
            resetSnapshotState(cachedSnapshotRestoreReason: cachedSnapshotRestoreReason)
        } else {
            resetSnapshotState(cachedSnapshotRestoreReason: nil)
        }
        await RealtimeCompanionClientFactory.invalidateCachedConnections()
        prepareRealtimeConnectionInBackground()
        return shouldRestartEventStream
    }

    private func resetSnapshotState(cachedSnapshotRestoreReason: String?) {
        serverHealth = nil
        reachedBaseURL = nil
        detailBySessionID = [:]
        errorMessage = nil
        if let cachedSnapshotRestoreReason {
            snapshotLoads.scheduleCachedSnapshotRestoreIfAvailable(reason: cachedSnapshotRestoreReason)
        }

        snapshot = nil
        sessionSections = .empty
        sessionIndex = .empty
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

    func loadSnapshot(allowsConcurrentConnectionReload: Bool = false) async {
        await snapshotLoads.loadSnapshot(allowsConcurrentConnectionReload: allowsConcurrentConnectionReload)
    }

    private func performSnapshotLoad(loadRevision: Int) async {
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
                await adoptServerHealthBaseURLsIfNeeded(resolvedHealth)
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

            let didRestoreCachedSnapshot: Bool
            if snapshot == nil {
                didRestoreCachedSnapshot = await restoreCachedSnapshotIfAvailable(
                    reason: CachedSnapshotRestoreReason.loadFailure,
                    onlyWhenSnapshotMissing: true,
                    restoreRevision: loadRevision
                )
            } else {
                didRestoreCachedSnapshot = false
            }
            let hasUsableSnapshot = snapshot != nil
            let nextConnectionState = connectionStateAfterSnapshotLoadFailure(
                error,
                hasUsableSnapshot: hasUsableSnapshot
            )
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

    private func connectionStateAfterSnapshotLoadFailure(
        _ error: Error,
        hasUsableSnapshot: Bool
    ) -> ConnectivityState {
        let nextState = connectionState(for: error)
        guard hasUsableSnapshot, nextState == .offline else {
            return nextState
        }

        guard connectionState == .connected || serverHealth != nil || reachedBaseURL != nil else {
            return nextState
        }

        CompanionDiagnostics.record(
            "snapshot:load-failed-preserve-connected error=\(error.localizedDescription)"
        )
        return .connected
    }

    func refresh() async {
        await loadSnapshot()
    }

    func refreshFromFallbackTimer() async {
        guard !sessionMiniController.isSyncing || snapshot == nil else {
            CompanionDiagnostics.record("root:refresh-skip session-sync-active")
            return
        }

        await refresh()
    }

    private func adoptServerHealthBaseURLsIfNeeded(
        _ resolvedHealth: ResolvedCompanionServerHealth
    ) async {
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
            preference: CompanionConfiguration.connectionRoutePreference(),
            preservingExistingPorts: true
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
        await RealtimeCompanionClientFactory.invalidateCachedConnections()
        prepareRealtimeConnectionInBackground()
        restartRealtimeSessionSyncIfActive()
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
        guard let value = nonEmptyText(value) else {
            return nil
        }

        return URL(string: value)
    }

    private static func nonEmptyText(_ value: String?) -> String? {
        guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !value.isEmpty
        else {
            return nil
        }

        return value
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

        await adoptHandoffBaseURLIfAvailable(from: activity)
        CompanionDiagnostics.lifecycle.info(
            "Continuation opening session id=\(sessionID, privacy: .public)"
        )
        CompanionDiagnostics.record("continuation:model-session id=\(sessionID)")
        await continueFromMacSession(id: sessionID)
    }

    func handleOpenURL(_ url: URL) async {
        if openSettingsTarget(from: url) {
            return
        }

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

        await adoptHandoffBaseURLIfAvailable(from: url)
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
        if snapshot == nil || sessionIndex.session(withID: sessionID) == nil {
            await loadSnapshot()
        }

        let sessionSurface = requestedAssistantSurfaceIfAvailable(
            request.assistantSurface,
            sessionID: sessionID
        ) ?? selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
        pendingOpenSessionID = sessionID
        await refreshSessionDetail(id: sessionID, assistantSurface: sessionSurface)
    }

    func consumePendingSettingsTarget() -> SettingsSearchTarget? {
        let target = pendingSettingsTarget
        pendingSettingsTarget = nil
        return target
    }

    private func openSettingsTarget(from url: URL) -> Bool {
        guard let target = LooperSettingsDeepLink.target(from: url) else {
            return false
        }

        pendingSettingsTarget = target
        CompanionDiagnostics.record("settings-url:target target=\(target.rawValue)")
        return true
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
        if snapshot == nil || sessionIndex.session(withID: sessionID) == nil {
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

    private func adoptHandoffBaseURLIfAvailable(from activity: NSUserActivity) async {
        guard let handoffBaseURL = LooperContinuationActivity.baseURL(from: activity) else {
            return
        }

        await adoptHandoffBaseURL(handoffBaseURL)
    }

    private func adoptHandoffBaseURLIfAvailable(from url: URL) async {
        guard let handoffBaseURL = LooperContinuationActivity.baseURL(from: url) else {
            return
        }

        await adoptHandoffBaseURL(handoffBaseURL)
    }

    private func adoptHandoffBaseURL(_ handoffBaseURL: URL) async {
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
        let shouldRestartEventStream = await resetConnectionStateForStoredConnection(
            clearsSnapshotCache: false,
            cachedSnapshotRestoreReason: CachedSnapshotRestoreReason.handoffConnectionChange
        )
        if shouldRestartEventStream {
            startRealtimeSessionSyncIfNeeded()
        }
        CompanionDiagnostics.lifecycle.info(
            "Handoff adopted baseURL=\(handoffBaseURL.absoluteString, privacy: .public)"
        )
        CompanionDiagnostics.record("handoff:base-url-adopted baseURL=\(handoffBaseURL.absoluteString)")
    }

    private func restartRealtimeSessionSyncIfActive() {
        guard sessionMiniController.isSyncing else {
            return
        }

        stopRealtimeSessionSync()
        startRealtimeSessionSyncIfNeeded()
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
        } else if let detectedSurface = sessionIndex.assistantSurface(containingSessionID: sessionID) {
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
        await sessionMutations.applyMode(preset, to: sessionID)
    }

    @discardableResult
    func beginApplyMode(_ preset: SessionMode?, to sessionID: String) -> Task<Bool, Never> {
        sessionMutations.beginApplyMode(preset, to: sessionID)
    }

    private func resolveModeMutationBarriers(_ accepted: Bool) async {
        await sessionMutations.resolveModeMutationBarriers(accepted)
    }

    #if DEBUG
    func setModeDrainBeforeFinishHookForSelfTest(_ hook: (() async -> Void)?) {
        sessionMutations.setModeDrainBeforeFinishHookForSelfTest(hook)
    }
    #endif

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
        await sessionMutations.sendSessionPrompt(prompt, to: sessionID)
    }

    @discardableResult
    func beginSendSessionPrompt(_ prompt: String, to sessionID: String) -> Task<Bool, Never> {
        sessionMutations.beginSendSessionPrompt(prompt, to: sessionID)
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        prompt: String,
        to sessionID: String,
        clientMutationID providedClientMutationID: String? = nil
    ) async -> Bool {
        await notificationReplies.submitNotificationReply(
            notificationID: notificationID,
            prompt: prompt,
            to: sessionID,
            clientMutationID: providedClientMutationID
        )
    }

    func drainPendingNotificationReplies() async {
        await notificationReplies.drainPendingNotificationReplies()
    }

    @discardableResult
    private func startNotificationReplyOutboxDrainIfNeeded() -> Task<Void, Never>? {
        notificationReplies.startOutboxDrainIfNeeded()
    }

    private func stopNotificationReplyOutboxDrain() {
        notificationReplies.stopOutboxDrain()
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
        prompt: String? = nil,
        notificationID: String? = nil,
        clientMutationID: String? = nil
    ) async {
        switch action {
        case .openSession:
            if snapshot == nil || sessionIndex.session(withID: sessionID) == nil {
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
            if let notificationID = notificationID?.trimmingCharacters(in: .whitespacesAndNewlines),
               !notificationID.isEmpty
            {
                await submitNotificationReply(
                    notificationID: notificationID,
                    prompt: prompt ?? "",
                    to: sessionID,
                    clientMutationID: clientMutationID
                )
            } else {
                await sendSessionPrompt(prompt ?? "", to: sessionID)
            }
        case .archive:
            await setSessionArchived(true, sessionID: sessionID)
        case .muteSession:
            await muteSession(sessionID)
        }
    }

    private func registerSessionQuickActionHandler() {
        SessionQuickActionCenter.shared.registerHandler { [weak self] request in
            await self?.performQuickAction(
                request.action,
                sessionID: request.sessionID,
                prompt: request.prompt,
                notificationID: request.notificationID,
                clientMutationID: request.clientMutationID
            )
        }
    }

    func configureStopQuickActions() {
        notifications.configureStopQuickActions()
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

    private func applyOptimisticMode(_ preset: SessionMode?, to sessionID: String) {
        var didUpdate = false

        if var detail = detailBySessionID[sessionID] {
            detail.effectiveMode = preset
            detailBySessionID[sessionID] = detail
            didUpdate = true
        }

        if var nextSnapshot = snapshot {
            updateMode(preset, for: sessionID, in: &nextSnapshot.sessions, didUpdate: &didUpdate)
            for surface in Array(nextSnapshot.surfaceSessions.keys) {
                updateMode(
                    preset,
                    for: sessionID,
                    in: &nextSnapshot.surfaceSessions[surface, default: []],
                    didUpdate: &didUpdate
                )
            }

            let visibleSnapshot = applySnapshotState(
                nextSnapshot,
                preferredSurface: selectedAssistantSurface
            )
            syncDetailCache(with: visibleSnapshot)
        }

        if didUpdate {
            lastUpdatedAt = Date()
        }
    }

    private func restoreOptimisticModeSnapshot(
        _ previousSnapshot: MobileSnapshot?,
        previousDetail: SessionDetail?,
        sessionID: String
    ) {
        if let previousSnapshot {
            let visibleSnapshot = applySnapshotState(
                previousSnapshot,
                preferredSurface: selectedAssistantSurface
            )
            syncDetailCache(with: visibleSnapshot)
        }

        detailBySessionID[sessionID] = previousDetail
    }

    private func updateMode(
        _ preset: SessionMode?,
        for sessionID: String,
        in sessions: inout [SessionSummary],
        didUpdate: inout Bool
    ) {
        guard let sessionIndex = sessions.firstIndex(where: { $0.id == sessionID }) else {
            return
        }

        sessions[sessionIndex].effectiveMode = preset
        didUpdate = true
    }

    private func applyPromptSendResult(
        _ result: CompanionPromptSendResult,
        sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) async {
        if let nextSnapshot = result.snapshot {
            await applySnapshot(nextSnapshot)
            if detailBySessionID[sessionID] != nil {
                await refreshSessionDetail(id: sessionID, assistantSurface: assistantSurface)
            }
            return
        }

        connectionState = .connected
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "prompt:accepted-without-snapshot sessionID=\(sessionID) kind=\(result.dispatchKind ?? "unknown")"
        )
        if detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID, assistantSurface: assistantSurface)
        }
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
        guard hasUsableSnapshot else {
            return false
        }

        switch state {
        case .connected, .offline:
            return true
        case .connecting, .locked, .unauthorized, .unpaired:
            return false
        }
    }

    @discardableResult
    private func restoreCachedSessionMiniSnapshotIfAvailable(reason: String) -> Bool {
        sessionMiniController.restoreCachedSnapshotIfAvailable(reason: reason) { [weak self] cachedSnapshot, reason in
            self?.applyCachedSnapshot(cachedSnapshot, reason: reason)
        }
    }

    private func makeClientMutationID() -> String {
        UUID().uuidString
    }

    @discardableResult
    private func restoreCachedSnapshotIfAvailable(
        reason: String,
        onlyWhenSnapshotMissing: Bool,
        restoreRevision: Int
    ) async -> Bool {
        if onlyWhenSnapshotMissing, snapshot != nil {
            CompanionDiagnostics.record("snapshot:cache-restore-skip reason=\(reason) existingSnapshot=true")
            return false
        }

        guard let cachedSnapshot = await CompanionSnapshotCache.load() else {
            return false
        }

        guard !Task.isCancelled else {
            CompanionDiagnostics.record("snapshot:cache-restore-cancelled reason=\(reason)")
            return false
        }

        guard restoreRevision == connectionRevision else {
            CompanionDiagnostics.record("snapshot:cache-restore-stale-skip reason=\(reason)")
            return false
        }

        if onlyWhenSnapshotMissing, snapshot != nil {
            CompanionDiagnostics.record("snapshot:cache-restore-skip reason=\(reason) existingSnapshot=true")
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
        lastAppliedRealtimeRevision = Self.normalizedRevision(visibleSnapshot.revision)
        hasValidatedCurrentSnapshotWithHTTP = false
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
        lastAppliedRealtimeRevision = Self.normalizedRevision(visibleSnapshot.revision)
        hasValidatedCurrentSnapshotWithHTTP = true

        syncSpotlightIndex(with: sessionIndex.allSessions)
        scheduleLocalFallbackNotificationsIfNeeded(
            previousSnapshot: previousSnapshot,
            currentSnapshot: visibleSnapshot
        )
    }

    private func scheduleLocalFallbackNotificationsIfNeeded(
        previousSnapshot: MobileSnapshot?,
        currentSnapshot: MobileSnapshot
    ) {
        guard shouldUseLocalFallbackNotifications, let previousSnapshot else {
            return
        }

        let notificationManager = notificationManager
        Task { @MainActor in
            await notificationManager.deliverStopNotifications(
                previousSnapshot: previousSnapshot,
                currentSnapshot: currentSnapshot
            )
        }
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
        sessionIndex = SessionIndex(snapshot: visibleSnapshot)
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
        guard let surface = sessionIndex.assistantSurface(containingSessionID: sessionID) else {
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
        sessionIndex.assistantSurface(containingSessionID: sessionID) ?? selectedAssistantSurface
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

        Task { @MainActor in
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

        Task { @MainActor in
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

    private static func normalizedRevision(_ revision: String?) -> String? {
        let trimmedRevision = revision?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedRevision, !trimmedRevision.isEmpty else {
            return nil
        }
        return trimmedRevision
    }
}

extension CompanionAppModel: CompanionSnapshotLoadCoordinatorDelegate {
    var snapshotLoadConnectionRevision: Int {
        connectionRevision
    }

    func snapshotLoadSetLoading(_ isLoading: Bool) {
        self.isLoading = isLoading
    }

    func snapshotLoadClearError() {
        errorMessage = nil
    }

    func snapshotLoadRestoreCachedSnapshot(
        reason: String,
        onlyWhenSnapshotMissing: Bool,
        restoreRevision: Int
    ) async -> Bool {
        await restoreCachedSnapshotIfAvailable(
            reason: reason,
            onlyWhenSnapshotMissing: onlyWhenSnapshotMissing,
            restoreRevision: restoreRevision
        )
    }

    func snapshotLoadPerform(loadRevision: Int) async {
        await performSnapshotLoad(loadRevision: loadRevision)
    }
}

extension CompanionAppModel: CompanionSessionMutationCoordinatorDelegate {
    var sessionMutationService: any CompanionService {
        service
    }

    var sessionMutationConnectionRevision: Int {
        connectionRevision
    }

    func sessionMutationMakeClientMutationID() -> String {
        makeClientMutationID()
    }

    func sessionMutationRollbackState(for sessionID: String) -> ModeRollbackState {
        ModeRollbackState(
            snapshot: snapshot,
            detail: detailBySessionID[sessionID]
        )
    }

    func sessionMutationAssistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        sessionIndex.assistantSurface(containingSessionID: sessionID) ?? selectedAssistantSurface
    }

    func sessionMutationCanSendPrompt(to sessionID: String) -> Bool {
        !mutatingSessionIDs.contains(sessionID)
    }

    func sessionMutationRejectPrompt(_ message: String) {
        errorMessage = message
        Haptics.warning()
    }

    func sessionMutationApplyOptimisticMode(_ preset: SessionMode?, to sessionID: String) {
        applyOptimisticMode(preset, to: sessionID)
    }

    func sessionMutationApplyModeResult(
        _ result: CompanionSessionModeResult,
        sessionID: String
    ) async {
        if let nextSnapshot = result.snapshot {
            await applySnapshot(nextSnapshot)
            if detailBySessionID[sessionID] != nil {
                await refreshSessionDetail(id: sessionID)
            }
            return
        }

        connectionState = .connected
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "mode:accepted-without-snapshot sessionID=\(sessionID) mode=\(result.acceptedMode?.rawValue ?? "unset")"
        )
    }

    func sessionMutationHandleModeFailure(
        _ error: Error,
        sessionID: String,
        rollbackState: ModeRollbackState?
    ) -> Bool {
        restoreOptimisticModeSnapshot(
            rollbackState?.snapshot,
            previousDetail: rollbackState?.detail,
            sessionID: sessionID
        )
        connectionState = connectionState(for: error)
        clearConnectionRouteStateIfNeeded(for: connectionState)
        errorMessage = error.localizedDescription
        Haptics.error()
        return false
    }

    func sessionMutationSetPromptMutating(_ isMutating: Bool, sessionID: String) {
        setSessionMutation(isMutating, sessionID: sessionID)
    }

    func sessionMutationApplyPromptResult(
        _ result: CompanionPromptSendResult,
        sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) async {
        await applyPromptSendResult(
            result,
            sessionID: sessionID,
            assistantSurface: assistantSurface
        )
    }

    func sessionMutationHandlePromptFailure(_ error: Error, sessionID: String) -> Bool {
        connectionState = connectionState(for: error)
        clearConnectionRouteStateIfNeeded(for: connectionState)
        errorMessage = error.localizedDescription
        Haptics.error()
        return false
    }
}

extension CompanionAppModel: CompanionConnectionCoordinatorDelegate {
    var connectionCoordinatorCurrentState: ConnectivityState {
        connectionState
    }

    var connectionCoordinatorErrorMessage: String? {
        errorMessage
    }

    var connectionCoordinatorConfiguredBaseURL: String {
        configuredBaseURL
    }

    func connectionCoordinatorReloadConnection() async {
        await reloadConnection()
    }
}

extension CompanionAppModel: CompanionNotificationCoordinatorDelegate {
    var notificationService: any CompanionService {
        service
    }

    var notificationCanSendLocalNotifications: Bool {
        canSendLocalNotifications
    }

    var notificationAreLocalNotificationsDenied: Bool {
        areLocalNotificationsDenied
    }

    var notificationRemotePushRegistration: RemotePushRegistrationResponse? {
        remotePushRegistration
    }

    func notificationApplyLocalAuthorizationStatus(_ status: UNAuthorizationStatus) {
        localNotificationStatus = status
    }

    func notificationSetRemotePushRegistration(_ registration: RemotePushRegistrationResponse?) {
        remotePushRegistration = registration
    }

    func notificationSetRemotePushRegistrationInFlight(_ isRegistering: Bool) {
        isRegisteringRemotePush = isRegistering
    }

    func notificationSetRemotePushFailureMessage(_ message: String?) {
        remotePushFailureMessage = message
    }
}

extension CompanionAppModel: CompanionNotificationReplyCoordinatorDelegate {
    var notificationReplyService: any CompanionService {
        service
    }

    var notificationReplySelectedAssistantSurface: CompanionAssistantSurface {
        selectedAssistantSurface
    }

    func notificationReplyMakeClientMutationID() -> String {
        makeClientMutationID()
    }

    func notificationReplyAssistantSurface(for sessionID: String) -> CompanionAssistantSurface? {
        sessionIndex.assistantSurface(containingSessionID: sessionID)
    }

    func notificationReplyReject(_ message: String) {
        errorMessage = message
        Haptics.warning()
    }

    func notificationReplyApplyAccepted(
        _ response: LooperRealtimeNotificationReplyResponse,
        sessionID: String,
        notificationID: String,
        targetSurface: CompanionAssistantSurface
    ) async {
        connectionState = .connected
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "notification-reply:accepted sessionID=\(sessionID) notificationID=\(notificationID) kind=\(response.dispatchKind)"
        )
        if detailBySessionID[sessionID] != nil {
            await refreshSessionDetail(id: sessionID, assistantSurface: targetSurface)
        }
    }

    func notificationReplyApplyFailure(
        _ error: Error,
        sessionID: String,
        notificationID: String
    ) {
        connectionState = connectionState(for: error)
        clearConnectionRouteStateIfNeeded(for: connectionState)
        errorMessage = error.localizedDescription
        Haptics.error()
        CompanionDiagnostics.record(
            "notification-reply:send-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
        )
    }
}

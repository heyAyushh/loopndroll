import AppIntents
import Foundation
import LooperClientCore
import LooperCompanionCore
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
    var serverHealth: CompanionServerHealth?
    var reachedBaseURL: URL?
    var snapshotState = CompanionSnapshotStateStore()
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
    var isLoading = false
    var lastUpdatedAt: Date?
    var localNotificationStatus: UNAuthorizationStatus = .notDetermined
    var remotePushRegistration: RemotePushRegistrationResponse?
    var remotePushFailureMessage: String?
    var isRegisteringRemotePush = false
    private(set) var isSavingDefaultPrompt = false
    var pendingOpenSessionID: String?
    var pendingSettingsTarget: SettingsSearchTarget?

    @ObservationIgnored private var service: any CompanionService
    @ObservationIgnored private var sessionCommands: any CompanionSessionCommanding
    @ObservationIgnored private let reloadsServiceFromStoredConnection: Bool
    @ObservationIgnored private let notificationManager: LocalNotificationManager
    @ObservationIgnored private let spotlightCoordinator: CompanionSpotlightCoordinator
    @ObservationIgnored private let sessionDetailCoordinator = CompanionSessionDetailCoordinator()
    @ObservationIgnored private let sessionMiniController: CompanionSessionMiniController
    @ObservationIgnored private var connectionCoordinator: CompanionConnectionCoordinator?
    @ObservationIgnored private var notificationCoordinator: CompanionNotificationCoordinator?
    @ObservationIgnored private var snapshotLoadCoordinator: CompanionSnapshotLoadCoordinator?
    @ObservationIgnored private var connectionRevision = 0
    @ObservationIgnored private var activeServiceConnectionFingerprint = ""
    @ObservationIgnored private var donatedOpenedSiriSessionIDs: Set<String> = []

    init(
        environment: CompanionEnvironment,
        notificationManager: LocalNotificationManager = LocalNotificationManager(),
        remotePushRegistrar: RemotePushRegistrar = .shared,
        spotlightIndexer: SessionSpotlightIndexer = .shared,
        sessionRuntime providedSessionRuntime: CompanionSessionRuntime? = nil
    ) {
        reloadsServiceFromStoredConnection = environment.reloadsServiceFromStoredConnection
        self.notificationManager = notificationManager
        self.spotlightCoordinator = CompanionSpotlightCoordinator(indexer: spotlightIndexer)
        let explicitSessionRuntime = environment.sessionRuntime ?? providedSessionRuntime
        let sessionRuntime = explicitSessionRuntime
            ?? CompanionSessionRuntime.liveDefault()
        self.sessionMiniController = CompanionSessionMiniController(sessionRuntime: sessionRuntime)

        let didActivateBundledConnection = reloadsServiceFromStoredConnection &&
            CompanionConfiguration.activateBundledConnectionIfNeeded()
        let activeEnvironment = didActivateBundledConnection
            ? CompanionEnvironment.live(
                sessionRuntime: sessionRuntime
            )
            : environment
        service = activeEnvironment.service
        sessionCommands = activeEnvironment.sessionRuntime
            ?? explicitSessionRuntime
            ?? activeEnvironment.sessionCommands
        connectionCoordinator = CompanionConnectionCoordinator(delegate: self)
        notificationCoordinator = CompanionNotificationCoordinator(
            notificationManager: notificationManager,
            remotePushRegistrar: remotePushRegistrar,
            delegate: self
        )
        snapshotLoadCoordinator = CompanionSnapshotLoadCoordinator(delegate: self)
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
        SessionQuickActionCenter.shared.configureSessionRuntime(sessionMiniController.sessionRuntime)
        registerSessionQuickActionHandler()
        CompanionDiagnostics.lifecycle.info(
            "Model initialized baseURL=\(self.configuredBaseURL, privacy: .public) cachedSnapshotRestoreScheduled=\(didScheduleCachedSnapshotRestore, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "model:init baseURL=\(configuredBaseURL) cachedSnapshotRestoreScheduled=\(didScheduleCachedSnapshotRestore)"
        )
    }

    var viewState: CompanionAppViewState {
        CompanionAppViewState(model: self)
    }

    var snapshot: MobileSnapshot? {
        get {
            snapshotState.snapshot
        }
        set {
            if let newValue {
                snapshotState.applySnapshot(
                    newValue,
                    preferredSurface: snapshotState.selectedAssistantSurface
                )
            } else {
                snapshotState.reset()
            }
        }
    }

    var detailBySessionID: [String: SessionDetail] {
        get {
            snapshotState.detailBySessionID
        }
        set {
            snapshotState.detailBySessionID = newValue
        }
    }

    var sessionSections: SessionSections {
        snapshotState.sessionSections
    }

    var selectedAssistantSurface: CompanionAssistantSurface {
        get {
            snapshotState.selectedAssistantSurface
        }
        set {
            snapshotState.applyVisibleAssistantSurface(newValue)
        }
    }

    var sessionIndex: SessionIndex {
        snapshotState.sessionIndex
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

    private var snapshotLoads: CompanionSnapshotLoadCoordinator {
        guard let snapshotLoadCoordinator else {
            preconditionFailure("Snapshot load coordinator used before initialization")
        }
        return snapshotLoadCoordinator
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
            prepareSessionRuntimeInBackground()
        }
        startSessionRuntimeSyncIfNeeded()
        startNotificationReplyOutboxDrainIfNeeded()

        CompanionDiagnostics.record("snapshot:load-skip-state-mini-prepare")
        notifications.registerForRemoteNotificationsInBackground()
    }

    private func shouldReloadServiceFromStoredConnection() -> Bool {
        CompanionConfiguration.resolvedConnectionFingerprint() != activeServiceConnectionFingerprint
    }

    func stopSessionRuntimeSync() {
        sessionMiniController.stopSync()
    }

    func startSessionRuntimeSyncIfNeeded() {
        sessionMiniController.startSyncIfNeeded(
            connectionRevision: connectionRevision
        ) { [weak self] update, connectionRevision in
            self?.applySessionMiniSyncUpdate(
                update,
                connectionRevision: connectionRevision
            )
        }
    }

    private func applySessionMiniSyncUpdate(
        _ update: CompanionSessionMiniSyncUpdate,
        connectionRevision: Int
    ) {
        guard connectionRevision == self.connectionRevision else {
            CompanionDiagnostics.record("session-mini:sync-stale-skip")
            return
        }

        applyCachedSnapshot(update.snapshot, reason: "session-mini-sync-\(update.reason)")
        connectionState = .connected
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "session-mini:sync-applied reason=\(update.reason) seq=\(update.latestSeq)"
        )
    }

    private func prepareSessionRuntimeInBackground() {
        let sessionRuntime = sessionMiniController.sessionRuntime
        Task.detached(priority: .userInitiated) {
            await sessionRuntime?.prepareSessionRuntime()
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
        let shouldRestartSessionRuntimeSync = await resetConnectionStateForStoredConnection(
            clearsSnapshotCache: true,
            cachedSnapshotRestoreReason: nil
        )
        if shouldRestartSessionRuntimeSync {
            startSessionRuntimeSyncIfNeeded()
        }
        await loadSnapshot(allowsConcurrentConnectionReload: true)
    }

    @discardableResult
    private func resetConnectionStateForStoredConnection(
        clearsSnapshotCache: Bool,
        cachedSnapshotRestoreReason: String?
    ) async -> Bool {
        let shouldRestartSessionRuntimeSync = sessionMiniController.isSyncing
        connectionRevision += 1
        snapshotLoads.cancelCachedSnapshotRestore()
        snapshotLoads.cancelSnapshotLoad()
        stopSessionRuntimeSync()
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        stopNotificationReplyOutboxDrain()
        serverHealth = nil
        reachedBaseURL = nil
        snapshotState.clearDetails()
        pendingOpenSessionID = nil
        errorMessage = nil

        if clearsSnapshotCache {
            CompanionSnapshotCache.clear()
        }

        if reloadsServiceFromStoredConnection {
            configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
            applyLiveEnvironmentFromSessionCore()
            activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
            resetSnapshotState(cachedSnapshotRestoreReason: cachedSnapshotRestoreReason)
        } else {
            resetSnapshotState(cachedSnapshotRestoreReason: nil)
        }
        prepareSessionRuntimeInBackground()
        return shouldRestartSessionRuntimeSync
    }

    private func resetSnapshotState(cachedSnapshotRestoreReason: String?) {
        serverHealth = nil
        reachedBaseURL = nil
        snapshotState.clearDetails()
        errorMessage = nil
        if let cachedSnapshotRestoreReason {
            snapshotLoads.scheduleCachedSnapshotRestoreIfAvailable(reason: cachedSnapshotRestoreReason)
        }

        snapshotState.reset()
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
            let failureProjection = reduceSnapshotLoadFailureOrCrash(
                mappedErrorState: connectionState(for: error),
                currentState: connectionState,
                hasUsableSnapshot: hasUsableSnapshot,
                hasServerHealth: serverHealth != nil,
                hasReachedBaseURL: reachedBaseURL != nil
            )
            let nextConnectionState = connectionState(rawValue: failureProjection.connectionState)
            if failureProjection.preservedConnectedState {
                CompanionDiagnostics.record(
                    "snapshot:load-failed-preserve-connected error=\(error.localizedDescription)"
                )
            }
            connectionState = nextConnectionState
            clearConnectionRouteStateIfNeeded(
                shouldClear: failureProjection.shouldClearRouteState
            )
            errorMessage = failureProjection.shouldSuppressError ? nil : error.localizedDescription
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
        applyLiveEnvironmentFromSessionCore()
        prepareSessionRuntimeInBackground()
        restartSessionRuntimeSyncIfActive()
        CompanionDiagnostics.record(
            "health:base-urls-adopted count=\(nextBaseURLs.count) primary=\(configuredBaseURL)"
        )
    }

    var activeConnectionRouteBaseURL: URL? {
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

    private func liveEnvironmentFromSessionCore() -> CompanionEnvironment {
        CompanionEnvironment.live(
            sessionRuntime: sessionMiniController.sessionRuntime
        )
    }

    private func applyLiveEnvironmentFromSessionCore() {
        let environment = liveEnvironmentFromSessionCore()
        service = environment.service
        sessionCommands = environment.sessionRuntime ?? environment.sessionCommands
    }

    private func clearConnectionRouteStateIfNeeded(for state: ConnectivityState) {
        clearConnectionRouteStateIfNeeded(shouldClear: !state.allowsConnectionRoutePresentation)
    }

    private func clearConnectionRouteStateIfNeeded(shouldClear: Bool) {
        guard shouldClear else {
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
        if snapshot == nil || !snapshotState.containsSession(sessionID) {
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
        if snapshot == nil || !snapshotState.containsSession(sessionID) {
            await loadSnapshot()
        }
        let sessionSurface = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)

        await refreshSessionDetail(id: sessionID, assistantSurface: sessionSurface)
    }

    private func requestedAssistantSurfaceIfAvailable(
        _ requestedSurface: CompanionAssistantSurface?,
        sessionID: String
    ) -> CompanionAssistantSurface? {
        guard let requestedSurface = snapshotState.activateRequestedAssistantSurfaceIfAvailable(
            requestedSurface,
            sessionID: sessionID
        )
        else {
            return nil
        }

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
        let shouldRestartSessionRuntimeSync = await resetConnectionStateForStoredConnection(
            clearsSnapshotCache: false,
            cachedSnapshotRestoreReason: CachedSnapshotRestoreReason.handoffConnectionChange
        )
        if shouldRestartSessionRuntimeSync {
            startSessionRuntimeSyncIfNeeded()
        }
        CompanionDiagnostics.lifecycle.info(
            "Handoff adopted baseURL=\(handoffBaseURL.absoluteString, privacy: .public)"
        )
        CompanionDiagnostics.record("handoff:base-url-adopted baseURL=\(handoffBaseURL.absoluteString)")
    }

    private func restartSessionRuntimeSyncIfActive() {
        guard sessionMiniController.isSyncing else {
            return
        }

        stopSessionRuntimeSync()
        startSessionRuntimeSyncIfNeeded()
    }

    func loadSessionDetail(id: String) async {
        let outcome = await sessionDetailCoordinator.loadIfNeeded(
            id: id,
            service: service,
            snapshotState: snapshotState,
            selectedAssistantSurface: selectedAssistantSurface,
            connectionRevision: connectionRevision,
            isCurrentConnectionRevision: { [weak self] revision in
                self?.connectionRevision == revision
            }
        )
        applySessionDetailLoadOutcome(outcome)
    }

    func refreshSessionDetail(
        id: String,
        assistantSurface: CompanionAssistantSurface? = nil
    ) async {
        let outcome = await sessionDetailCoordinator.refresh(
            id: id,
            assistantSurface: assistantSurface,
            service: service,
            snapshotState: snapshotState,
            selectedAssistantSurface: selectedAssistantSurface,
            connectionRevision: connectionRevision,
            isCurrentConnectionRevision: { [weak self] revision in
                self?.connectionRevision == revision
            }
        )
        applySessionDetailLoadOutcome(outcome)
    }

    private func applySessionDetailLoadOutcome(_ outcome: CompanionSessionDetailLoadOutcome) {
        if case .failed(let lastError) = outcome {
            errorMessage = lastError.localizedDescription
        }
    }

    func applyMode(_ preset: SessionMode?, to sessionID: String) async {
        _ = await applyModeIntent(preset, to: sessionID)
    }

    @discardableResult
    func beginApplyMode(_ preset: SessionMode?, to sessionID: String) -> Task<Bool, Never> {
        Task { @MainActor [weak self] in
            await self?.applyModeIntent(preset, to: sessionID) ?? false
        }
    }

    func setSessionArchived(_ archived: Bool, sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.setSessionArchived(id: sessionID, archived: archived)
        }

        if didMutate, snapshotState.hasDetail(for: sessionID) {
            await refreshSessionDetail(id: sessionID)
        }
    }

    func deleteSession(_ sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.deleteSession(id: sessionID)
        }

        if didMutate {
            snapshotState.removeDetail(for: sessionID)
        }
    }

    @discardableResult
    func sendSessionPrompt(_ prompt: String, to sessionID: String) async -> Bool {
        await sendPromptIntent(prompt, to: sessionID)
    }

    @discardableResult
    func beginSendSessionPrompt(_ prompt: String, to sessionID: String) -> Task<Bool, Never> {
        Task { @MainActor [weak self] in
            await self?.sendPromptIntent(prompt, to: sessionID) ?? false
        }
    }

    private func applyModeIntent(_ preset: SessionMode?, to sessionID: String) async -> Bool {
        let targetCommands = sessionCommands
        let targetRevision = connectionRevision

        do {
            let result = try await targetCommands.setSessionMode(
                id: sessionID,
                preset: preset
            )
            guard targetRevision == connectionRevision else {
                CompanionDiagnostics.record("mode:mutation-stale-skip sessionID=\(sessionID)")
                return false
            }
            await applyModeResult(result, sessionID: sessionID)
            return true
        } catch {
            guard targetRevision == connectionRevision else {
                CompanionDiagnostics.record(
                    "mode:mutation-stale-error-skip sessionID=\(sessionID) error=\(error.localizedDescription)"
                )
                return false
            }
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }
    }

    private func applyModeResult(
        _ result: CompanionSessionModeResult,
        sessionID: String
    ) async {
        connectionState = .connected
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "mode:accepted sessionID=\(sessionID) mode=\(result.acceptedMode?.rawValue ?? "unset")"
        )
    }

    private func sendPromptIntent(_ prompt: String, to sessionID: String) async -> Bool {
        let targetSurface = snapshotState.assistantSurface(for: sessionID)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPrompt.isEmpty else {
            errorMessage = "Prompt is required."
            Haptics.warning()
            return false
        }

        let targetCommands = sessionCommands
        let targetRevision = connectionRevision

        do {
            let result = try await targetCommands.sendSessionPrompt(
                id: sessionID,
                prompt: trimmedPrompt,
                assistantSurface: targetSurface
            )
            guard targetRevision == connectionRevision else {
                CompanionDiagnostics.record("prompt:mutation-stale-skip sessionID=\(sessionID)")
                return false
            }
            await applyPromptSendResult(
                result,
                sessionID: sessionID,
                assistantSurface: targetSurface
            )
            return true
        } catch {
            guard targetRevision == connectionRevision else {
                CompanionDiagnostics.record(
                    "prompt:mutation-stale-error-skip sessionID=\(sessionID) error=\(error.localizedDescription)"
                )
                return false
            }
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        prompt: String,
        to sessionID: String
    ) async -> Bool {
        let trimmedNotificationID = notificationID.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedNotificationID.isEmpty else {
            rejectNotificationReply("Notification reply is missing its delivery ID.")
            return false
        }
        guard !trimmedPrompt.isEmpty else {
            rejectNotificationReply("Prompt is required.")
            return false
        }

        let targetSurface = snapshotState.assistantSurface(containingSessionID: sessionID)
            ?? selectedAssistantSurface

        return await submitNotificationReplyCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt,
            targetSurface: targetSurface
        )
    }

    func drainPendingNotificationReplies() async {
        let drainTask = startNotificationReplyOutboxDrainIfNeeded()
        await drainTask?.value
    }

    @discardableResult
    private func startNotificationReplyOutboxDrainIfNeeded() -> Task<Void, Never>? {
        sessionMiniController.startNotificationReplyOutboxDrainIfNeeded(
            submit: { [weak self] in
                await self?.submitPendingNotificationReply() ?? false
            }
        )
    }

    private func stopNotificationReplyOutboxDrain() {
        sessionMiniController.stopNotificationReplyOutboxDrain()
    }

    @discardableResult
    private func submitNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        targetSurface: CompanionAssistantSurface
    ) async -> Bool {
        do {
            let response = try await sessionCommands.submitNotificationReply(
                notificationID: notificationID,
                sessionID: sessionID,
                prompt: prompt,
                assistantSurface: nil
            )
            await applyNotificationReplyAccepted(
                response,
                sessionID: sessionID,
                notificationID: notificationID,
                targetSurface: targetSurface
            )
            return true
        } catch {
            applyNotificationReplyFailure(
                error,
                sessionID: sessionID,
                notificationID: notificationID
            )
            startNotificationReplyOutboxDrainIfNeeded()
            return false
        }
    }

    @discardableResult
    private func submitPendingNotificationReply() async -> Bool {
        do {
            let response = try await sessionCommands.submitPendingNotificationReply()
            guard let acceptedSessionID = Self.nonEmptyText(response.entityId),
                  let acceptedNotificationID = Self.nonEmptyText(response.notificationId)
            else {
                CompanionDiagnostics.record(
                    "notification-reply:pending-drain-missing-ack-target"
                )
                return false
            }

            let acceptedSurface = snapshotState.assistantSurface(containingSessionID: acceptedSessionID)
                ?? selectedAssistantSurface
            await applyNotificationReplyAccepted(
                response,
                sessionID: acceptedSessionID,
                notificationID: acceptedNotificationID,
                targetSurface: acceptedSurface
            )
            return true
        } catch {
            CompanionDiagnostics.record(
                "notification-reply:pending-drain-failed error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func rejectNotificationReply(_ message: String) {
        errorMessage = message
        Haptics.warning()
    }

    private func applyNotificationReplyAccepted(
        _ response: ClientNotificationReplyIntentResult,
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
        if snapshotState.hasDetail(for: sessionID) {
            await refreshSessionDetail(id: sessionID, assistantSurface: targetSurface)
        }
    }

    private func applyNotificationReplyFailure(
        _ error: Error,
        sessionID: String,
        notificationID: String
    ) {
        applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
        Haptics.error()
        CompanionDiagnostics.record(
            "notification-reply:send-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
        )
    }

    func muteSession(_ sessionID: String) async {
        let didMutate = await mutateSessionSnapshot(sessionID: sessionID) {
            try await service.muteSession(id: sessionID)
        }

        if didMutate, snapshotState.hasDetail(for: sessionID) {
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
        notificationID: String? = nil
    ) async {
        switch action {
        case .openSession:
            if snapshot == nil || !snapshotState.containsSession(sessionID) {
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
                    to: sessionID
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
                notificationID: request.notificationID
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
        guard snapshotState.selectAssistantSurface(surface) else {
            return
        }

        startAssistantSurfaceSaveIfNeeded()
    }

    private func mutateSessionSnapshot(
        sessionID: String,
        _ operation: () async throws -> MobileSnapshot
    ) async -> Bool {
        return await mutateSnapshot(operation)
    }

    private func applyPromptSendResult(
        _ result: CompanionPromptSendResult,
        sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) async {
        connectionState = .connected
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "prompt:accepted sessionID=\(sessionID) kind=\(result.dispatchKind ?? "unknown")"
        )
        if snapshotState.hasDetail(for: sessionID) {
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

            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }
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
            case .invalidResponse, .localStoreUnavailable, .serverError:
                return .connected
            }
        }

        return .offline
    }

    private func applyConnectionFailure(
        _ error: Error,
        suppressErrorWhenSnapshotUsable: Bool
    ) {
        let projection = reduceConnectionFailureOrCrash(
            mappedErrorState: connectionState(for: error),
            hasUsableSnapshot: snapshot != nil,
            suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
        )
        connectionState = connectionState(rawValue: projection.connectionState)
        clearConnectionRouteStateIfNeeded(shouldClear: projection.shouldClearRouteState)
        errorMessage = projection.shouldSuppressError ? nil : error.localizedDescription
    }

    private func reduceSnapshotLoadFailureOrCrash(
        mappedErrorState: ConnectivityState,
        currentState: ConnectivityState,
        hasUsableSnapshot: Bool,
        hasServerHealth: Bool,
        hasReachedBaseURL: Bool
    ) -> ClientSnapshotLoadFailureProjection {
        do {
            return try reduceSnapshotLoadFailure(
                mappedErrorState: mappedErrorState.rawValue,
                currentConnectionState: currentState.rawValue,
                hasUsableSnapshot: hasUsableSnapshot,
                hasServerHealth: hasServerHealth,
                hasReachedBaseUrl: hasReachedBaseURL
            )
        } catch {
            fatalError("Connection state projection failed: \(error)")
        }
    }

    private func reduceConnectionFailureOrCrash(
        mappedErrorState: ConnectivityState,
        hasUsableSnapshot: Bool,
        suppressErrorWhenSnapshotUsable: Bool
    ) -> ClientConnectionFailureProjection {
        do {
            return try reduceConnectionFailure(
                mappedErrorState: mappedErrorState.rawValue,
                hasUsableSnapshot: hasUsableSnapshot,
                suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
            )
        } catch {
            fatalError("Connection failure projection failed: \(error)")
        }
    }

    private func connectionState(rawValue: String) -> ConnectivityState {
        guard let state = ConnectivityState(rawValue: rawValue) else {
            fatalError("Connection projection returned unknown state: \(rawValue)")
        }
        return state
    }

    private func isCancellationError(_ error: Error) -> Bool {
        if error is CancellationError {
            return true
        }

        let nsError = error as NSError
        return nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled
    }

    @discardableResult
    private func restoreCachedSessionMiniSnapshotIfAvailable(reason: String) -> Bool {
        sessionMiniController.restoreCachedSnapshotIfAvailable(reason: reason) { [weak self] cachedSnapshot, reason in
            self?.applyCachedSnapshot(cachedSnapshot, reason: reason)
        }
    }

    @discardableResult
    private func restoreCachedSnapshotIfAvailable(
        reason: String,
        onlyWhenSnapshotMissing: Bool,
        restoreRevision: Int
    ) async -> Bool {
        if snapshotState.shouldSkipCachedRestore(onlyWhenSnapshotMissing: onlyWhenSnapshotMissing) {
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

        if snapshotState.shouldSkipCachedRestore(onlyWhenSnapshotMissing: onlyWhenSnapshotMissing) {
            CompanionDiagnostics.record("snapshot:cache-restore-skip reason=\(reason) existingSnapshot=true")
            return false
        }

        applyCachedSnapshot(cachedSnapshot, reason: reason)
        return true
    }

    private func applyCachedSnapshot(_ cachedSnapshot: MobileSnapshot, reason: String) {
        let visibleSnapshot = snapshotState.applySnapshot(
            cachedSnapshot,
            preferredSurface: cachedSnapshot.globalSettings.assistantSurface
        )
        lastUpdatedAt = Date()
        spotlightCoordinator.clearForCachedSnapshotIfNeeded()
        CompanionDiagnostics.record(
            "snapshot:cache-restore reason=\(reason) sessions=\(visibleSnapshot.sessions.count)"
        )
    }

    private func applySnapshot(_ nextSnapshot: MobileSnapshot) async {
        let previousSnapshot = snapshot
        let visibleSnapshot = snapshotState.applySnapshot(nextSnapshot)
        connectionState = .connected
        lastUpdatedAt = Date()
        CompanionSnapshotCache.save(visibleSnapshot)
        spotlightCoordinator.sync(with: snapshotState.allSessions)
        scheduleLocalFallbackNotificationsIfNeeded(
            previousSnapshot: previousSnapshot,
            currentSnapshot: visibleSnapshot
        )
    }

    private func scheduleLocalFallbackNotificationsIfNeeded(
        previousSnapshot: MobileSnapshot?,
        currentSnapshot: MobileSnapshot
    ) {
        guard viewState.shouldUseLocalFallbackNotifications, let previousSnapshot else {
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
        snapshotState.applyVisibleAssistantSurface(surface)
    }

    private func startAssistantSurfaceSaveIfNeeded() {
        guard snapshotState.beginAssistantSurfaceSaveIfNeeded() else {
            return
        }

        Task { @MainActor in
            await persistPendingAssistantSurfaces()
        }
    }

    private func persistPendingAssistantSurfaces() async {
        while let nextAssistantSurface = snapshotState.dequeuePendingAssistantSurfaceSave() {
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

        snapshotState.finishAssistantSurfaceSave()
        if snapshotState.hasPendingAssistantSurfaceSave {
            startAssistantSurfaceSaveIfNeeded()
        }
    }

    private func handleAssistantSurfaceSaveFailure(_ error: Error) {
        applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
        CompanionDiagnostics.record(
            "assistant-surface:save-failed surface=\(selectedAssistantSurface.rawValue) state=\(connectionState.rawValue) error=\(error.localizedDescription)"
        )
    }

    private func selectAssistantSurfaceContainingSessionIfAvailable(_ sessionID: String) -> CompanionAssistantSurface? {
        guard let surface = snapshotState.selectAssistantSurfaceContainingSessionIfAvailable(sessionID) else {
            CompanionDiagnostics.record("continuation:surface-miss sessionID=\(sessionID)")
            return nil
        }

        CompanionDiagnostics.record(
            "continuation:surface-match sessionID=\(sessionID) surface=\(surface.rawValue)"
        )
        return surface
    }

    private func assistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        snapshotState.assistantSurface(for: sessionID)
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
        viewState.canSendLocalNotifications
    }

    var notificationAreLocalNotificationsDenied: Bool {
        viewState.areLocalNotificationsDenied
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

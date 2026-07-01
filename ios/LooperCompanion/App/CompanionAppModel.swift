import AppIntents
import Foundation
import LooperClientCore
import LooperCompanionCore
import Observation
import UserNotifications

private enum CachedSnapshotRestoreReason {
    static let appLaunch = "app-launch"
    static let loadFailure = "load-failure"
}

private enum SiriDonationEvent {
    static let openSession = "open-session"
    static let setDefaultSession = "set-default-session"
}

private enum PromptDispatchFailure {
    static let resumeFailedDetailPrefix = "prompt-resume-failed:"
}

private enum AssistantSurfaceETTraceMetric {
    static let startedNotification = Notification.Name(rawValue: "EmergeMetricStarted")
    static let endedNotification = Notification.Name(rawValue: "EmergeMetricEnded")
    static let metricUserInfoKey = "metric"
    static let namePrefix = "assistant_surface_selection"

    static func name(for surface: CompanionAssistantSurface) -> String {
        "\(namePrefix).\(surface.rawValue)"
    }

    static func postStarted(for surface: CompanionAssistantSurface) {
        post(startedNotification, surface: surface)
    }

    static func postEnded(for surface: CompanionAssistantSurface) {
        post(endedNotification, surface: surface)
    }

    private static func post(_ notification: Notification.Name, surface: CompanionAssistantSurface) {
        #if DEBUG
        NotificationCenter.default.post(
            name: notification,
            object: nil,
            userInfo: [metricUserInfoKey: name(for: surface)]
        )
        #endif
    }
}

private enum AssistantSurfaceSelectionLogEvent {
    static let requested = "Selection requested"
    static let applied = "Selection applied"
    static let cancelled = "Selection cancelled"
    static let failed = "Selection failed"
}

private enum AssistantSurfaceSelectionFailureReason {
    static let noChange = "no-change"
    static let projectionRejected = "projection-rejected"
}

enum StateMiniRecoveryResult: Equatable {
    case skipped
    case applied
    case empty
    case failed(String)

    var didApplySnapshot: Bool {
        if case .applied = self {
            return true
        }
        return false
    }
}

private enum StateMiniRecoveryError: LocalizedError {
    case emptySnapshot

    var errorDescription: String? {
        switch self {
        case .emptySnapshot:
            "Looper did not return a state-mini snapshot."
        }
    }
}

enum CompanionLocalSessionReconcileReason: String {
    case activeScene = "active-scene"
    case fallbackTimer = "fallback-timer"
    case continuationWithoutSession = "continuation-without-session"
    case sessionOpen = "session-open"
    case sessionsPullRefresh = "sessions-pull-refresh"
    case searchPullRefresh = "search-pull-refresh"
    case manualRefresh = "manual-refresh"
    case unlockRecovery = "unlock-recovery"

    var shouldReplayCachedSnapshotWhenLoaded: Bool {
        switch self {
        case .sessionsPullRefresh,
             .searchPullRefresh,
             .manualRefresh:
            return true
        case .activeScene,
             .fallbackTimer,
             .continuationWithoutSession,
             .sessionOpen,
             .unlockRecovery:
            return false
        }
    }

    var shouldRecoverStateMiniSnapshot: Bool {
        switch self {
        case .activeScene,
             .sessionOpen,
             .sessionsPullRefresh,
             .searchPullRefresh,
             .manualRefresh,
             .unlockRecovery:
            return true
        case .fallbackTimer,
             .continuationWithoutSession:
            return false
        }
    }

    var shouldRecoverStateMiniSnapshotBeforeCachedReplay: Bool {
        switch self {
        case .sessionsPullRefresh,
             .searchPullRefresh,
             .manualRefresh:
            return true
        case .activeScene,
             .fallbackTimer,
             .continuationWithoutSession,
             .sessionOpen,
             .unlockRecovery:
            return false
        }
    }

    var shouldSkipLocalReplayWhenStreamIsLive: Bool {
        self == .fallbackTimer
    }
}

@MainActor
@Observable
final class CompanionAppModel {
    var configuredBaseURL = ""
    var serverHealth: CompanionServerHealth?
    var reachedBaseURL: URL?
    var activeSessionRouteBaseURL: URL?
    var snapshotState = CompanionSnapshotStateStore()
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
    var isLoading = false
    var lastUpdatedAt: Date?
    var realtimeServerTime: String?
    var realtimeLatestSeq: Int64 = 0
    var realtimeStreamIsLive = false
    private(set) var isAwaitingRouteSessionProof = false
    var localNotificationStatus: UNAuthorizationStatus = .notDetermined
    var remotePushRegistration: RemotePushRegistrationResponse?
    var remotePushFailureMessage: String?
    var isRegisteringRemotePush = false
    private(set) var isSavingDefaultPrompt = false
    var pendingOpenSessionID: String?
    var pendingSettingsTarget: SettingsSearchTarget?

    @ObservationIgnored private var service: any CompanionService
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
    @ObservationIgnored private var didAttemptForegroundSessionMiniRecovery = false

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
        let sessionRuntime = environment.sessionRuntime
            ?? providedSessionRuntime
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

        _ = restoreCachedSessionMiniSnapshotIfAvailable(
            reason: CachedSnapshotRestoreReason.appLaunch
        )

        configureStopQuickActions()
        SessionQuickActionCenter.shared.configureSessionRuntime(sessionMiniController.sessionRuntime)
        registerSessionQuickActionHandler()
        CompanionDiagnostics.lifecycle.info(
            "Model initialized baseURL=\(self.configuredBaseURL, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "model:init baseURL=\(configuredBaseURL)"
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

    var sessionSections: SessionSections {
        snapshotState.sessionSections
    }

    var selectedAssistantSurface: CompanionAssistantSurface {
        get {
            snapshotState.selectedAssistantSurface
        }
        set {
            _ = selectAssistantSurface(newValue)
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
        let didActivateBundledConnection = reloadsServiceFromStoredConnection &&
            CompanionConfiguration.activateBundledConnectionIfNeeded()
        if reloadsServiceFromStoredConnection,
           didActivateBundledConnection || shouldReloadServiceFromStoredConnection() {
            CompanionDiagnostics.lifecycle.info("Stored connection changed during active-state preparation")
            await resetConnectionStateForStoredConnection(clearsSnapshotCache: false)
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
        markSessionStreamStopped()
    }

    private func stopSessionRuntimeSyncForRestart() {
        sessionMiniController.stopSync()
        markSessionStreamStopped()
    }

    func startSessionRuntimeSyncIfNeeded() {
        sessionMiniController.startSyncIfNeeded(
            connectionRevision: connectionRevision
        ) { [weak self] update, connectionRevision in
            self?.applySessionMiniSyncUpdate(
                update,
                connectionRevision: connectionRevision
            )
        } onLiveness: { [weak self] liveness, connectionRevision in
            self?.applySessionMiniLivenessUpdate(
                liveness,
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

        guard applyCachedSessionMiniSnapshot(
            update.snapshot,
            reason: "session-mini-sync-\(update.reason)",
            latestSeq: update.latestSeq
        ) else {
            // The stream is still delivering data even though this
            // particular session-list snapshot was rejected as stale, so
            // liveness genuinely advances here (it has its own seq guard).
            applyRealtimeStreamLiveness(
                serverTime: update.snapshot.host.lastSyncedAt,
                latestSeq: update.latestSeq,
                isLive: true,
                endpointURL: update.endpointURL
            )
            if update.reason == CompanionSessionMiniSyncReason.textChunk {
                // A rejected text-chunk snapshot doesn't change the session
                // list, but the assistant reply text itself still needs a
                // fresh render: SessionDetailScreen re-reads the latest
                // reply straight from client-core on each render. Bumping
                // lastUpdatedAt is what triggers that re-render; it does not
                // claim the snapshot was applied.
                lastUpdatedAt = Date()
                CompanionDiagnostics.record(
                    "session-detail:text-chunk-invalidated seq=\(update.latestSeq)"
                )
                return
            }
            CompanionDiagnostics.record(
                "session-mini:sync-snapshot-skip reason=\(update.reason) seq=\(update.latestSeq)"
            )
            return
        }

        applyRealtimeStreamLiveness(
            serverTime: update.snapshot.host.lastSyncedAt,
            latestSeq: update.latestSeq,
            isLive: true,
            endpointURL: update.endpointURL
        )
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "session-mini:sync-applied reason=\(update.reason) seq=\(update.latestSeq)"
        )
    }

    private func applySessionMiniLivenessUpdate(
        _ update: CompanionSessionMiniLivenessUpdate,
        connectionRevision: Int
    ) {
        guard connectionRevision == self.connectionRevision else {
            CompanionDiagnostics.record("session-mini:liveness-stale-skip")
            return
        }

        let didChange = applyRealtimeStreamLiveness(
            serverTime: update.serverTime,
            latestSeq: update.latestSeq,
            isLive: update.isLive,
            endpointURL: update.endpointURL
        )
        guard didChange else {
            return
        }
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "session-mini:liveness-applied reason=\(update.reason) seq=\(update.latestSeq)"
        )
    }

    @discardableResult
    func applyRealtimeStreamLiveness(
        serverTime: String,
        latestSeq: Int64,
        isLive: Bool,
        endpointURL: URL?
    ) -> Bool {
        guard latestSeq >= realtimeLatestSeq || Self.isStreamRestartLiveness(
            latestSeq: latestSeq,
            isLive: isLive
        ) else {
            CompanionDiagnostics.record(
                "session-mini:liveness-stale-skip latestSeq=\(latestSeq) realtimeSeq=\(realtimeLatestSeq)"
            )
            return false
        }

        var didChange = false
        if !serverTime.isEmpty {
            if realtimeServerTime != serverTime {
                realtimeServerTime = serverTime
                didChange = true
            }
            didChange = snapshotState.applyHostSyncTime(serverTime) || didChange
        }
        let nextLatestSeq = max(realtimeLatestSeq, latestSeq)
        if realtimeLatestSeq != nextLatestSeq {
            realtimeLatestSeq = nextLatestSeq
            didChange = true
        }
        if realtimeStreamIsLive != isLive {
            realtimeStreamIsLive = isLive
            didChange = true
        }
        let nextRouteBaseURL = isLive ? endpointURL : nil
        if activeSessionRouteBaseURL != nextRouteBaseURL {
            activeSessionRouteBaseURL = nextRouteBaseURL
            didChange = true
        }
        if isLive, isAwaitingRouteSessionProof {
            isAwaitingRouteSessionProof = false
            didChange = true
        }
        let nextConnectionState = Self.connectionStateForSessionLiveness(
            isLive: isLive,
            currentState: connectionState
        )
        if connectionState != nextConnectionState {
            connectionState = nextConnectionState
            didChange = true
        }
        if isLive, errorMessage != nil {
            errorMessage = nil
            didChange = true
        }
        return didChange
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
        await resetConnectionStateForStoredConnection(clearsSnapshotCache: true)
        startSessionRuntimeSyncIfNeeded()
        await loadSnapshot(allowsConcurrentConnectionReload: true)
    }

    private func resetConnectionStateForStoredConnection(clearsSnapshotCache: Bool) async {
        connectionRevision += 1
        snapshotLoads.cancelSnapshotLoad()
        stopSessionRuntimeSyncForRestart()
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        stopNotificationReplyOutboxDrain()
        serverHealth = nil
        reachedBaseURL = nil
        activeSessionRouteBaseURL = nil
        realtimeServerTime = nil
        realtimeLatestSeq = 0
        realtimeStreamIsLive = false
        connectionState = .connecting
        pendingOpenSessionID = nil
        errorMessage = nil
        didAttemptForegroundSessionMiniRecovery = false

        if clearsSnapshotCache {
            CompanionSnapshotCache.clear()
        }

        if reloadsServiceFromStoredConnection {
            configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
            applyLiveEnvironmentFromSessionCore()
            activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
        }
        resetSnapshotState()
        prepareSessionRuntimeInBackground()
    }

    private func applyStoredConnectionRoutePreference() async {
        connectionRevision += 1
        isAwaitingRouteSessionProof = true
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        applyLiveEnvironmentFromSessionCore()
        activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
        errorMessage = nil
        serverHealth = nil
        reachedBaseURL = nil
        markSessionStreamStopped()
        didAttemptForegroundSessionMiniRecovery = false

        restartSessionRuntimeSyncForRouteChange()
    }

    private func restartSessionRuntimeSyncForRouteChange() {
        if sessionMiniController.isSyncing {
            stopSessionRuntimeSyncForRestart()
        }
        startSessionRuntimeSyncIfNeeded()
    }

    private func resetSnapshotState() {
        serverHealth = nil
        reachedBaseURL = nil
        activeSessionRouteBaseURL = nil
        errorMessage = nil

        guard !snapshotState.hasSnapshot else {
            CompanionDiagnostics.record("snapshot:reset-preserve-local-visible")
            return
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
            if hasKnownSessionMiniCursor() {
                if !snapshotState.hasSnapshot {
                    _ = restoreCachedSessionMiniSnapshotIfAvailable(
                        reason: "network-snapshot-local-session-mini-cursor"
                    )
                }
                markCachedSnapshotReadyIfNeeded(reason: "network-snapshot-local-session-mini-cursor")
                CompanionDiagnostics.record(
                    "snapshot:load-session-mini-cursor-skip realtimeSeq=\(realtimeLatestSeq)"
                )
                return
            }
            if sessionMiniController.hasLocalStateMiniEvidence(reason: "network-snapshot-local-session-mini-store") {
                markCachedSnapshotReadyIfNeeded(reason: "network-snapshot-local-session-mini-store")
                CompanionDiagnostics.record("snapshot:load-session-mini-store-skip")
                return
            }
            let nextSnapshot = try await service.loadSnapshot()
            guard loadRevision == connectionRevision else {
                CompanionDiagnostics.record("snapshot:load-stale-skip")
                return
            }
            guard shouldApplyNetworkSnapshot(nextSnapshot) else {
                CompanionDiagnostics.record(
                    "snapshot:load-live-session-wins realtimeSeq=\(realtimeLatestSeq)"
                )
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

            let didRestoreSessionMiniSnapshot: Bool
            if snapshot == nil {
                didRestoreSessionMiniSnapshot = restoreCachedSessionMiniSnapshotIfAvailable(
                    reason: CachedSnapshotRestoreReason.loadFailure
                )
            } else {
                didRestoreSessionMiniSnapshot = false
            }
            let hasUsableSnapshot = snapshot != nil
            let failureProjection = reduceSnapshotLoadFailureOrCrash(
                mappedErrorState: connectionState(for: error),
                currentState: connectionState,
                hasUsableSnapshot: hasUsableSnapshot,
                hasServerHealth: serverHealth != nil,
                hasReachedBaseURL: reachedBaseURL != nil
            )
            let nextConnectionState = sessionAuthoritativeConnectionState(
                connectionState(rawValue: failureProjection.connectionState)
            )
            if failureProjection.preservedConnectedState {
                CompanionDiagnostics.record(
                    "snapshot:load-failed-local-state-preserved nextState=\(nextConnectionState.rawValue) error=\(error.localizedDescription)"
                )
            }
            connectionState = nextConnectionState
            clearConnectionRouteStateIfNeeded(for: nextConnectionState)
            errorMessage = sessionAuthoritativeErrorMessage(
                shouldSuppressProjectionError: failureProjection.shouldSuppressError,
                error: error
            )
            CompanionDiagnostics.lifecycle.error(
                "Snapshot load failed state=\(nextConnectionState.rawValue, privacy: .public) restoredCache=\(didRestoreSessionMiniSnapshot, privacy: .public) error=\(error.localizedDescription, privacy: .public)"
            )
            CompanionDiagnostics.record(
                "snapshot:load-failed state=\(nextConnectionState.rawValue) restoredCache=\(didRestoreSessionMiniSnapshot) error=\(error.localizedDescription)"
            )
        }
    }

    func refresh() async {
        await reconcileLocalSessionState(reason: .manualRefresh)
    }

    /// Single linear reconciliation flow. Order of precedence:
    /// 1. Skip entirely if the realtime stream is already live and this
    ///    reason doesn't need a local replay on top of it.
    /// 2. Reasons that must recover the state-mini snapshot before trusting
    ///    any cached replay (pull-to-refresh style reasons) do that first.
    /// 3. Otherwise, prefer an already-loaded snapshot; fall back to
    ///    restoring one from the local cache; finally attempt state-mini
    ///    recovery as the last resort.
    ///
    /// A cached snapshot, once present, always ends the flow via
    /// `finishWithExistingSnapshot` (mirroring `recoverStateMiniSnapshotIfNeeded`
    /// in the background) rather than being checked for a second time.
    func reconcileLocalSessionState(reason: CompanionLocalSessionReconcileReason) async {
        startSessionRuntimeSyncIfNeeded()

        if reason.shouldSkipLocalReplayWhenStreamIsLive, realtimeStreamIsLive {
            CompanionDiagnostics.record(
                "session-mini:local-reconcile-live-skip reason=\(reason.rawValue)"
            )
            return
        }

        if reason.shouldRecoverStateMiniSnapshotBeforeCachedReplay {
            let recoveryResult = await recoverStateMiniSnapshotIfNeeded(reason: reason)
            if recoveryResult.didApplySnapshot {
                return
            }
            if snapshotState.hasSnapshot {
                if reason.shouldReplayCachedSnapshotWhenLoaded,
                   restoreCachedSessionMiniSnapshotIfAvailable(reason: reason.rawValue) {
                    CompanionDiagnostics.record(
                        "session-mini:local-reconcile-applied-after-recovery reason=\(reason.rawValue) result=\(recoveryResult)"
                    )
                    return
                }
                markCachedSnapshotReadyIfNeeded(reason: reason.rawValue)
                CompanionDiagnostics.record(
                    "session-mini:local-reconcile-existing-after-recovery reason=\(reason.rawValue) result=\(recoveryResult)"
                )
                return
            }
        }

        if snapshotState.hasSnapshot, !reason.shouldReplayCachedSnapshotWhenLoaded {
            await finishWithExistingSnapshot(reason: reason)
            return
        }

        if restoreCachedSessionMiniSnapshotIfAvailable(reason: reason.rawValue) {
            CompanionDiagnostics.record(
                "session-mini:local-reconcile-applied reason=\(reason.rawValue)"
            )
            _ = await recoverStateMiniSnapshotIfNeeded(reason: reason)
            return
        }

        if (await recoverStateMiniSnapshotIfNeeded(reason: reason)).didApplySnapshot {
            return
        }

        CompanionDiagnostics.record(
            "session-mini:local-reconcile-wait reason=\(reason.rawValue)"
        )
    }

    private func finishWithExistingSnapshot(reason: CompanionLocalSessionReconcileReason) async {
        markCachedSnapshotReadyIfNeeded(reason: reason.rawValue)
        CompanionDiagnostics.record(
            "session-mini:local-reconcile-existing reason=\(reason.rawValue)"
        )
        _ = await recoverStateMiniSnapshotIfNeeded(reason: reason)
    }

    private func recoverStateMiniSnapshotIfNeeded(
        reason: CompanionLocalSessionReconcileReason
    ) async -> StateMiniRecoveryResult {
        guard reason.shouldRecoverStateMiniSnapshot else {
            return .skipped
        }
        if shouldSkipStateMiniRecoveryBecauseLocalStateIsReady(reason: reason) {
            CompanionDiagnostics.record(
                "session-mini:recovery-skip reason=\(reason.rawValue) local-ready"
            )
            return .skipped
        }
        if reason == .activeScene {
            guard !didAttemptForegroundSessionMiniRecovery else {
                return .skipped
            }
        }
        guard let sessionRuntime = sessionMiniController.sessionRuntime else {
            let error = HTTPCompanionServiceError.localStoreUnavailable
            applyStateMiniRecoveryFailure(error, reason: reason)
            return .failed(error.localizedDescription)
        }

        if reason == .activeScene {
            didAttemptForegroundSessionMiniRecovery = true
        }

        do {
            guard let recoveredSnapshot = try await sessionRuntime.recoverStateMiniSnapshot() else {
                CompanionDiagnostics.record(
                    "session-mini:recovery-empty reason=\(reason.rawValue)"
                )
                applyStateMiniRecoveryFailure(StateMiniRecoveryError.emptySnapshot, reason: reason)
                return .empty
            }
            let didApplySnapshot = applyCachedSessionMiniSnapshot(
                recoveredSnapshot.snapshot,
                reason: "session-mini-recovery-\(reason.rawValue)",
                latestSeq: recoveredSnapshot.latestSeq
            )
            guard didApplySnapshot else {
                CompanionDiagnostics.record(
                    "session-mini:recovery-stale-skip reason=\(reason.rawValue) seq=\(recoveredSnapshot.latestSeq)"
                )
                return .skipped
            }
            CompanionDiagnostics.record(
                "session-mini:recovery-applied reason=\(reason.rawValue) sessions=\(recoveredSnapshot.snapshot.sessions.count)"
            )
            return .applied
        } catch {
            CompanionDiagnostics.record(
                "session-mini:recovery-failed reason=\(reason.rawValue) error=\(error.localizedDescription)"
            )
            applyStateMiniRecoveryFailure(error, reason: reason)
            return .failed(error.localizedDescription)
        }
    }

    private func applyStateMiniRecoveryFailure(
        _ error: Error,
        reason: CompanionLocalSessionReconcileReason
    ) {
        restoreCachedSessionMiniSnapshotIfAvailable(
            reason: "session-mini-recovery-failure-\(reason.rawValue)"
        )
        applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
        CompanionDiagnostics.record(
            "session-mini:recovery-truth-failed reason=\(reason.rawValue) error=\(error.localizedDescription)"
        )
    }

    private func shouldSkipStateMiniRecoveryBecauseLocalStateIsReady(
        reason: CompanionLocalSessionReconcileReason
    ) -> Bool {
        guard snapshotState.hasSnapshot else {
            return false
        }

        switch reason {
        case .activeScene:
            return realtimeStreamIsLive
        case .sessionsPullRefresh,
             .searchPullRefresh,
             .manualRefresh:
            return false
        case .sessionOpen,
             .unlockRecovery:
            return false
        case .fallbackTimer,
             .continuationWithoutSession:
            return false
        }
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
        await restartSessionRuntimeSyncIfActive()
        CompanionDiagnostics.record(
            "health:base-urls-adopted count=\(nextBaseURLs.count) primary=\(configuredBaseURL)"
        )
    }

    var activeConnectionRouteBaseURL: URL? {
        guard connectionState.allowsConnectionRoutePresentation else {
            return nil
        }

        return activeSessionRouteBaseURL
    }

    private func liveEnvironmentFromSessionCore() -> CompanionEnvironment {
        CompanionEnvironment.live(
            sessionRuntime: sessionMiniController.sessionRuntime
        )
    }

    private func applyLiveEnvironmentFromSessionCore() {
        let environment = liveEnvironmentFromSessionCore()
        service = environment.service
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
            CompanionDiagnostics.lifecycle.info("Continuation activity had no session id; reconciling local state")
            CompanionDiagnostics.record("continuation:model-local-reconcile-no-session")
            await reconcileLocalSessionState(reason: .continuationWithoutSession)
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
            await reconcileLocalSessionState(reason: .sessionOpen)
        }

        if requestedAssistantSurfaceIfAvailable(
            request.assistantSurface,
            sessionID: sessionID
        ) == nil {
            _ = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
        }
        _ = openSessionFromLocalTruth(sessionID, diagnosticPrefix: "siri-open")
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
        if snapshot == nil || !snapshotState.containsSession(sessionID) {
            await reconcileLocalSessionState(reason: .sessionOpen)
        }
        _ = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
        _ = openSessionFromLocalTruth(sessionID, diagnosticPrefix: "continuation")
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
        await resetConnectionStateForStoredConnection(clearsSnapshotCache: false)
        startSessionRuntimeSyncIfNeeded()
        CompanionDiagnostics.lifecycle.info(
            "Handoff adopted baseURL=\(handoffBaseURL.absoluteString, privacy: .public)"
        )
        CompanionDiagnostics.record("handoff:base-url-adopted baseURL=\(handoffBaseURL.absoluteString)")
    }

    private func restartSessionRuntimeSyncIfActive() async {
        guard sessionMiniController.isSyncing else {
            return
        }

        stopSessionRuntimeSyncForRestart()
        startSessionRuntimeSyncIfNeeded()
    }

    @discardableResult
    func refreshSessionDetail(
        id: String,
        assistantSurface: CompanionAssistantSurface? = nil
    ) -> Bool {
        sessionDetailCoordinator.refresh(
            id: id,
            assistantSurface: assistantSurface,
            snapshotState: snapshotState
        )
    }

    func sessionDetail(
        for sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> SessionDetail? {
        guard var detail = snapshotState.detail(
            for: sessionID,
            assistantSurface: assistantSurface
        ) else {
            return nil
        }

        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            return detail
        }

        do {
            let projection = try targetRuntime.sessionDetail(sessionID: sessionID)
            detail.applyLatestReplyProjection(projection)
        } catch {
            CompanionDiagnostics.record(
                "session-detail:client-core-read-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
        }
        return detail
    }

    @discardableResult
    private func openSessionFromLocalTruth(
        _ sessionID: String,
        diagnosticPrefix: String
    ) -> Bool {
        guard refreshSessionDetail(id: sessionID) else {
            pendingOpenSessionID = nil
            CompanionDiagnostics.record("\(diagnosticPrefix):local-detail-missing id=\(sessionID)")
            return false
        }

        pendingOpenSessionID = sessionID
        return true
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
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        do {
            try await targetRuntime.setSessionArchived(
                threadID: sessionID,
                archived: archived
            )
            applyAcceptedClientCoreLocalSnapshot(reason: "archive")
            errorMessage = nil
            lastUpdatedAt = Date()
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        CompanionDiagnostics.record("archive:client-core-owned sessionID=\(sessionID) archived=\(archived)")
    }

    func deleteSession(_ sessionID: String) async {
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        do {
            try await targetRuntime.deleteSession(threadID: sessionID)
            applyAcceptedClientCoreLocalSnapshot(reason: "delete")
            errorMessage = nil
            lastUpdatedAt = Date()
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        CompanionDiagnostics.record("delete:client-core-owned sessionID=\(sessionID)")
    }

    @discardableResult
    func sendSessionPrompt(
        _ prompt: String,
        intent: CompanionPromptIntent = .steer,
        to sessionID: String
    ) async -> Bool {
        await sendPromptIntent(prompt, intent: intent, to: sessionID)
    }

    @discardableResult
    func beginSendSessionPrompt(
        _ prompt: String,
        intent: CompanionPromptIntent = .steer,
        to sessionID: String
    ) -> Task<Bool, Never> {
        Task { @MainActor [weak self] in
            await self?.sendPromptIntent(prompt, intent: intent, to: sessionID) ?? false
        }
    }

    private func applyModeIntent(_ preset: SessionMode?, to sessionID: String) async -> Bool {
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }

        do {
            let result = try await targetRuntime.setMode(
                threadID: sessionID,
                preset: preset
            )
            if !applyAcceptedModeProjection(preset, sessionID: sessionID) {
                applyAcceptedClientCoreLocalSnapshot(reason: "mode")
            }
            recordModeAccepted(result, sessionID: sessionID)
            return true
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }
    }

    @discardableResult
    private func applyAcceptedModeProjection(_ preset: SessionMode?, sessionID: String) -> Bool {
        guard let sourceSnapshot = snapshotState.sourceSnapshot else {
            return false
        }
        do {
            let snapshotJSON = try encodeMobileSnapshot(sourceSnapshot)
            let projection = try reduceMobileSnapshotOptimisticMode(
                snapshotJson: snapshotJSON,
                detailJson: "",
                sessionId: sessionID,
                preset: preset?.rawValue ?? "",
                selectedAssistantSurface: snapshotState.selectedAssistantSurface.rawValue
            )
            guard projection.didUpdate,
                  let visibleSnapshot = decodeMobileSnapshot(projection.visibleSnapshotJson)
            else {
                return false
            }
            snapshotState.applyOptimisticVisibleSnapshot(
                visibleSnapshot,
                selectedSurface: snapshotState.selectedAssistantSurface
            )
            lastUpdatedAt = Date()
            return true
        } catch {
            CompanionDiagnostics.record(
                "mode:optimistic-projection-failed id=\(sessionID) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func recordModeAccepted(
        _ result: ClientSessionModeIntentResult,
        sessionID: String
    ) {
        errorMessage = nil
        lastUpdatedAt = Date()
        let acceptedMode = result.preset.trimmingCharacters(in: .whitespacesAndNewlines)
        CompanionDiagnostics.record(
            "mode:accepted sessionID=\(sessionID) mode=\(acceptedMode.isEmpty ? "unset" : acceptedMode)"
        )
    }

    private func sendPromptIntent(
        _ prompt: String,
        intent: CompanionPromptIntent,
        to sessionID: String
    ) async -> Bool {
        let targetSurface = snapshotState.assistantSurface(for: sessionID)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPrompt.isEmpty else {
            errorMessage = "Prompt is required."
            Haptics.warning()
            return false
        }

        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }

        do {
            let result = try await targetRuntime.sendPrompt(
                threadID: sessionID,
                prompt: trimmedPrompt,
                assistantSurface: targetSurface,
                promptIntent: intent
            )
            applyAcceptedClientCoreLocalSnapshot(reason: "prompt")
            recordPromptAccepted(
                result,
                sessionID: sessionID
            )
            return true
        } catch {
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

        return await submitNotificationReplyCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt
        )
    }

    @discardableResult
    func drainPendingNotificationReplies() async -> Bool {
        let drainTask = startNotificationReplyOutboxDrainIfNeeded()
        return await drainTask?.value ?? false
    }

    @discardableResult
    private func startNotificationReplyOutboxDrainIfNeeded() -> Task<Bool, Never>? {
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
        prompt: String
    ) async -> Bool {
        guard let sessionRuntime = sessionMiniController.sessionRuntime else {
            applyNotificationReplyFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                sessionID: sessionID,
                notificationID: notificationID
            )
            startNotificationReplyOutboxDrainIfNeeded()
            return false
        }

        do {
            let response = try await sessionRuntime.submitNotificationReply(
                notificationID: notificationID,
                threadID: sessionID,
                prompt: prompt,
                assistantSurface: nil
            )
            applyAcceptedClientCoreLocalSnapshot(reason: "notification-reply")
            recordNotificationReplyAccepted(
                response,
                sessionID: sessionID,
                notificationID: notificationID
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
        guard let sessionRuntime = sessionMiniController.sessionRuntime else {
            CompanionDiagnostics.record("notification-reply:pending-drain-missing-session-runtime")
            return false
        }

        do {
            let response = try await sessionRuntime.submitPendingNotificationReply()
            applyAcceptedClientCoreLocalSnapshot(reason: "notification-reply-pending")
            recordNotificationReplyAccepted(
                response,
                sessionID: Self.nonEmptyText(response.entityId) ?? "unknown",
                notificationID: Self.nonEmptyText(response.notificationId) ?? "unknown"
            )
            return true
        } catch ClientCoreError.NoPendingNotificationReply {
            CompanionDiagnostics.record("notification-reply:pending-drain-empty")
            return false
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

    private func recordNotificationReplyAccepted(
        _ response: ClientNotificationReplyIntentResult,
        sessionID: String,
        notificationID: String
    ) {
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "notification-reply:accepted sessionID=\(sessionID) notificationID=\(notificationID) kind=\(response.dispatchKind)"
        )
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
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        do {
            try await targetRuntime.muteSession(threadID: sessionID)
            applyAcceptedClientCoreLocalSnapshot(reason: "mute")
            errorMessage = nil
            lastUpdatedAt = Date()
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }
    }

    @discardableResult
    func setSiriDefaultSession(_ session: SessionSummary) async -> Bool {
        await setSiriDefaultSession(session.id)
    }

    @discardableResult
    func setSiriDefaultSession(
        _ sessionID: String,
        assistantSurface requestedSurface: CompanionAssistantSurface? = nil
    ) async -> Bool {
        let targetSurface = requestedSurface ?? assistantSurface(for: sessionID)
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return false
        }

        do {
            try await targetRuntime.setSiriDefaultSession(
                threadID: sessionID,
                assistantSurface: targetSurface
            )
            snapshotState.applyAcceptedSiriDefaultSession(
                sessionID: sessionID,
                assistantSurface: targetSurface
            )
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return false
        }

        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record("siri-default:client-core-owned sessionID=\(sessionID)")
        Haptics.success()
        if let session = snapshotState.session(withID: sessionID, assistantSurface: targetSurface) {
            await donateSetDefaultSiriSession(session)
        }
        return true
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

    @discardableResult
    func markCurrentSiriSession(_ session: SessionSummary) async -> Bool {
        await markCurrentSiriSession(session.id)
    }

    @discardableResult
    func markCurrentSiriSession(
        _ sessionID: String,
        assistantSurface requestedSurface: CompanionAssistantSurface? = nil
    ) async -> Bool {
        let targetSurface = requestedSurface ?? assistantSurface(for: sessionID)
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: true)
            return false
        }

        do {
            try await targetRuntime.setSiriCurrentSession(
                threadID: sessionID,
                assistantSurface: targetSurface
            )
            snapshotState.applyAcceptedSiriCurrentSession(
                sessionID: sessionID,
                assistantSurface: targetSurface
            )
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            return false
        }

        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record("siri-current:client-core-owned sessionID=\(sessionID)")
        return true
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
                await reconcileLocalSessionState(reason: .sessionOpen)
            }
            _ = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
            _ = openSessionFromLocalTruth(sessionID, diagnosticPrefix: "quick-action-open")
        case .continueChat:
            if snapshot == nil {
                await reconcileLocalSessionState(reason: .sessionOpen)
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

    @discardableResult
    func saveDefaultPrompt(_ defaultPrompt: String) async -> Bool {
        guard !isSavingDefaultPrompt else {
            return false
        }
        guard let targetRuntime = sessionMiniController.sessionRuntime else {
            applyConnectionFailure(HTTPCompanionServiceError.localStoreUnavailable, suppressErrorWhenSnapshotUsable: true)
            return false
        }

        isSavingDefaultPrompt = true
        defer {
            isSavingDefaultPrompt = false
        }

        do {
            try await targetRuntime.saveDefaultPrompt(defaultPrompt)
            snapshotState.applyAcceptedDefaultPrompt(defaultPrompt)
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            return false
        }

        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record("default-prompt:client-core-owned")
        return true
    }

    /// Selecting an assistant surface is a purely local, synchronous
    /// projection over already-loaded state (no FFI call, no network).
    /// Returns whether the selection was applied.
    @discardableResult
    func selectAssistantSurface(_ surface: CompanionAssistantSurface) -> Bool {
        logAssistantSurfaceSelection(AssistantSurfaceSelectionLogEvent.requested, surface: surface)

        guard snapshotState.selectedAssistantSurface != surface else {
            logAssistantSurfaceSelection(
                AssistantSurfaceSelectionLogEvent.cancelled,
                surface: surface,
                reason: AssistantSurfaceSelectionFailureReason.noChange
            )
            CompanionDiagnostics.record("assistant-surface:ignored surface=\(surface.rawValue) reason=no-change")
            return false
        }

        AssistantSurfaceETTraceMetric.postStarted(for: surface)
        guard snapshotState.selectAssistantSurface(surface) else {
            logAssistantSurfaceSelection(
                AssistantSurfaceSelectionLogEvent.failed,
                surface: surface,
                reason: AssistantSurfaceSelectionFailureReason.projectionRejected
            )
            AssistantSurfaceETTraceMetric.postEnded(for: surface)
            return false
        }
        logAssistantSurfaceSelection(AssistantSurfaceSelectionLogEvent.applied, surface: surface)
        CompanionDiagnostics.record("assistant-surface:selected-local surface=\(surface.rawValue)")
        errorMessage = nil
        lastUpdatedAt = Date()
        AssistantSurfaceETTraceMetric.postEnded(for: surface)

        return true
    }

    private func recordPromptAccepted(
        _ result: ClientSessionPromptIntentResult,
        sessionID: String
    ) {
        errorMessage = nil
        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "prompt:accepted sessionID=\(sessionID) kind=\(Self.nonEmptyText(result.dispatchKind) ?? "unknown")"
        )
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
                return .offline
            }
        }

        return .offline
    }

    private func applyConnectionFailure(
        _ error: Error,
        suppressErrorWhenSnapshotUsable: Bool
    ) {
        let mappedErrorState = connectionState(for: error)
        if suppressErrorWhenSnapshotUsable {
            let projection = reduceSnapshotLoadFailureOrCrash(
                mappedErrorState: mappedErrorState,
                currentState: connectionState,
                hasUsableSnapshot: snapshot != nil,
                hasServerHealth: serverHealth != nil,
                hasReachedBaseURL: reachedBaseURL != nil
            )
            if projection.preservedConnectedState {
                CompanionDiagnostics.record(
                    "connection:local-state-preserved error=\(error.localizedDescription)"
                )
            }
            let nextConnectionState = sessionAuthoritativeConnectionState(
                connectionState(rawValue: projection.connectionState)
            )
            connectionState = nextConnectionState
            clearConnectionRouteStateIfNeeded(for: nextConnectionState)
            errorMessage = sessionAuthoritativeErrorMessage(
                shouldSuppressProjectionError: projection.shouldSuppressError,
                error: error
            )
            return
        }

        let projection = reduceConnectionFailureOrCrash(
            mappedErrorState: mappedErrorState,
            hasUsableSnapshot: snapshot != nil,
            suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
        )
        let nextConnectionState = sessionAuthoritativeConnectionState(
            connectionState(rawValue: projection.connectionState)
        )
        connectionState = nextConnectionState
        clearConnectionRouteStateIfNeeded(for: nextConnectionState)
        errorMessage = sessionAuthoritativeErrorMessage(
            shouldSuppressProjectionError: projection.shouldSuppressError,
            error: error
        )
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
            CompanionDiagnostics.record(
                "connection:snapshot-load-projection-failed error=\(error.localizedDescription)"
            )
            return ClientSnapshotLoadFailureProjection(
                connectionState: mappedErrorState.rawValue,
                preservedConnectedState: false,
                shouldClearRouteState: mappedErrorState != .connected,
                shouldSuppressError: hasUsableSnapshot
            )
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
            CompanionDiagnostics.record(
                "connection:failure-projection-failed error=\(error.localizedDescription)"
            )
            return ClientConnectionFailureProjection(
                connectionState: mappedErrorState.rawValue,
                shouldClearRouteState: mappedErrorState != .connected,
                shouldSuppressError: hasUsableSnapshot && suppressErrorWhenSnapshotUsable
            )
        }
    }

    private func connectionState(rawValue: String) -> ConnectivityState {
        guard let state = ConnectivityState(rawValue: rawValue) else {
            CompanionDiagnostics.record("connection:projection-unknown-state state=\(rawValue)")
            return .offline
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
    private func restoreCachedSessionMiniSnapshotIfAvailable(
        reason: String,
        bypassesSeqGating: Bool = false
    ) -> Bool {
        sessionMiniController.restoreCachedSnapshotIfAvailable(reason: reason) { [weak self] cachedSnapshot, reason, latestSeq in
            self?.applyCachedSessionMiniSnapshot(
                cachedSnapshot,
                reason: reason,
                latestSeq: latestSeq,
                bypassesSeqGating: bypassesSeqGating
            ) ?? false
        }
    }

    /// Client-core-accepted commands (archive, delete, mode, prompt, etc.)
    /// already mutated the local store synchronously, so the resulting
    /// cached snapshot is authoritative regardless of its sequence number.
    /// This flag is threaded explicitly instead of inferring intent from a
    /// magic string prefix on `reason`.
    @discardableResult
    private func applyAcceptedClientCoreLocalSnapshot(reason: String) -> Bool {
        restoreCachedSessionMiniSnapshotIfAvailable(reason: reason, bypassesSeqGating: true)
    }

    private func applyCachedSnapshot(_ cachedSnapshot: MobileSnapshot, reason: String) {
        let result = snapshotState.applySnapshotResult(
            cachedSnapshot
        )
        let visibleSnapshot = result.visibleSnapshot
        markCachedSnapshotReadyIfNeeded(reason: reason)
        guard result.didChangeVisibleSnapshot else {
            CompanionDiagnostics.record(
                "snapshot:cache-restore-noop reason=\(reason) sessions=\(visibleSnapshot.sessions.count)"
            )
            return
        }

        lastUpdatedAt = Date()
        spotlightCoordinator.clearForCachedSnapshotIfNeeded()
        CompanionDiagnostics.record(
            "snapshot:cache-restore reason=\(reason) sessions=\(visibleSnapshot.sessions.count)"
        )
    }

    @discardableResult
    private func applyCachedSessionMiniSnapshot(
        _ cachedSnapshot: MobileSnapshot,
        reason: String,
        latestSeq: Int64,
        bypassesSeqGating: Bool = false
    ) -> Bool {
        guard bypassesSeqGating || shouldApplyStateMiniSnapshot(latestSeq: latestSeq) else {
            guard applyBroaderCachedSessionMiniSnapshot(
                cachedSnapshot,
                reason: reason,
                latestSeq: latestSeq
            ) else {
                CompanionDiagnostics.record(
                    "session-mini:cache-skip reason=\(reason) latestSeq=\(latestSeq) realtimeSeq=\(realtimeLatestSeq)"
                )
                return false
            }
            return true
        }

        realtimeLatestSeq = max(realtimeLatestSeq, latestSeq)
        applyCachedSnapshot(cachedSnapshot, reason: reason)
        return true
    }

    @discardableResult
    private func applyBroaderCachedSessionMiniSnapshot(
        _ cachedSnapshot: MobileSnapshot,
        reason: String,
        latestSeq: Int64
    ) -> Bool {
        guard let result = snapshotState.applyBroaderSnapshotPreservingCurrentSessions(
            cachedSnapshot,
            preferredSurface: snapshotState.selectedAssistantSurface
        ) else {
            CompanionDiagnostics.record(
                "session-mini:cache-skip reason=\(reason) latestSeq=\(latestSeq) realtimeSeq=\(realtimeLatestSeq)"
            )
            return false
        }

        realtimeLatestSeq = max(realtimeLatestSeq, latestSeq)
        markCachedSnapshotReadyIfNeeded(reason: reason)
        guard result.didChangeVisibleSnapshot else {
            CompanionDiagnostics.record(
                "session-mini:cache-merge-broader-noop reason=\(reason) sessions=\(result.visibleSnapshot.sessionsAcrossSurfaces.count)"
            )
            return true
        }

        lastUpdatedAt = Date()
        CompanionDiagnostics.record(
            "session-mini:cache-merge-broader reason=\(reason) sessions=\(result.visibleSnapshot.sessionsAcrossSurfaces.count)"
        )
        return true
    }

    private func applySnapshot(_ nextSnapshot: MobileSnapshot) async {
        let previousSnapshot = snapshot
        let visibleSnapshot = snapshotState.applySnapshot(nextSnapshot)
        if realtimeStreamIsLive {
            connectionState = .connected
        } else {
            serverHealth = nil
            reachedBaseURL = nil
        }
        lastUpdatedAt = Date()
        spotlightCoordinator.sync(with: snapshotState.allSessions)
        scheduleLocalFallbackNotificationsIfNeeded(
            previousSnapshot: previousSnapshot,
            currentSnapshot: visibleSnapshot
        )
    }

    private func shouldApplyNetworkSnapshot(_: MobileSnapshot) -> Bool {
        if hasKnownSessionMiniCursor() {
            return false
        }
        return snapshot == nil || !snapshotState.hasSnapshot
    }

    private func encodeMobileSnapshot(_ snapshot: MobileSnapshot) throws -> String {
        let data = try JSONEncoder().encode(snapshot)
        guard let json = String(data: data, encoding: .utf8) else {
            throw HTTPCompanionServiceError.invalidResponse
        }
        return json
    }

    private func decodeMobileSnapshot(_ json: String) -> MobileSnapshot? {
        try? JSONDecoder().decode(MobileSnapshot.self, from: Data(json.utf8))
    }

    private func hasKnownSessionMiniCursor() -> Bool {
        if realtimeLatestSeq > 0 {
            return true
        }
        guard let sessionRuntime = sessionMiniController.sessionRuntime else {
            return false
        }
        do {
            let localSnapshot = try sessionRuntime.currentStateMiniSnapshot()
            guard localSnapshot.latestSeq > 0 else {
                return false
            }
            if localSnapshot.sessions.isEmpty {
                return true
            }
            return try sessionRuntime.cachedSnapshot() != nil
        } catch {
            CompanionDiagnostics.record(
                "session-mini:cursor-read-failed error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func shouldApplyStateMiniSnapshot(latestSeq: Int64) -> Bool {
        if realtimeLatestSeq > 0, latestSeq < realtimeLatestSeq {
            return false
        }
        if snapshot == nil || !snapshotState.hasSnapshot {
            return true
        }
        if realtimeLatestSeq <= 0 {
            return true
        }
        if snapshotState.allSessions.isEmpty {
            return true
        }
        return latestSeq > realtimeLatestSeq
    }

    private func markCachedSnapshotReadyIfNeeded(reason: String) {
        guard connectionState == .connecting, snapshotState.hasSnapshot else {
            return
        }

        errorMessage = nil
        CompanionDiagnostics.record("connection:local-cache-ready reason=\(reason)")
    }

    private func markSessionStreamStopped() {
        realtimeStreamIsLive = false
        activeSessionRouteBaseURL = nil
        if connectionState == .connected {
            connectionState = .connecting
        }
    }

    private func sessionAuthoritativeConnectionState(
        _ projectedState: ConnectivityState
    ) -> ConnectivityState {
        if isAwaitingRouteSessionProof {
            switch projectedState {
            case .unauthorized, .locked, .unpaired, .offline:
                isAwaitingRouteSessionProof = false
                return projectedState
            case .connecting, .connected:
                return .connecting
            }
        }

        if realtimeStreamIsLive {
            return .connected
        }

        if projectedState == .connected, !realtimeStreamIsLive {
            return .connecting
        }

        return projectedState
    }

    private func sessionAuthoritativeErrorMessage(
        shouldSuppressProjectionError: Bool,
        error: Error
    ) -> String? {
        if realtimeStreamIsLive || shouldSuppressProjectionError {
            return nil
        }

        return error.localizedDescription
    }

    private static func connectionStateForSessionLiveness(
        isLive: Bool,
        currentState: ConnectivityState
    ) -> ConnectivityState {
        if isLive {
            return .connected
        }

        switch currentState {
        case .unauthorized, .locked, .unpaired:
            return currentState
        case .connecting, .connected, .offline:
            return .connecting
        }
    }

    private static func isStreamRestartLiveness(latestSeq: Int64, isLive: Bool) -> Bool {
        !isLive && latestSeq == CompanionSessionMiniController.restartLivenessUpdate().latestSeq
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

    private func logAssistantSurfaceSelection(
        _ event: String,
        surface: CompanionAssistantSurface,
        selectedSurface: CompanionAssistantSurface? = nil,
        reason: String? = nil
    ) {
        #if DEBUG
        let selectedSurfaceValue = selectedSurface?.rawValue ?? ""
        let reasonValue = reason ?? ""
        CompanionDiagnostics.assistantSurface.info(
            "\(event, privacy: .public) surface=\(surface.rawValue, privacy: .public) selectedSurface=\(selectedSurfaceValue, privacy: .public) reason=\(reasonValue, privacy: .public) source=local command=false streamRestart=false"
        )
        #endif
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

    func connectionCoordinatorApplyRoutePreference() async {
        await applyStoredConnectionRoutePreference()
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

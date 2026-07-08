import AppIntents
import Foundation
import LooperClientCore
import LooperCompanionCore
import Observation
import UserNotifications

private enum CachedSnapshotRestoreReason {
    static let appLaunch = "app-launch"
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
    var lastUpdatedAt: Date?
    var realtimeServerTime: String?
    var realtimeLatestSeq: Int64 = 0
    var realtimeStreamIsLive = false
    @ObservationIgnored var lastRealtimeDataAt: Date?
    var realtimeReconnectInProgress = false
    private(set) var sessionDetailRevision = 0
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
    @ObservationIgnored let connection: CompanionConnectionRuntime

    @ObservationIgnored private var commandDispatcher: CompanionCommandDispatcher?
    @ObservationIgnored private var connectionCoordinator: CompanionConnectionCoordinator?
    @ObservationIgnored private var pushCoordinator: CompanionPushCoordinator?
    @ObservationIgnored private var bootstrapLoader: CompanionBootstrapLoader?
    @ObservationIgnored private var activeServiceConnectionFingerprint = ""
    @ObservationIgnored private var donatedOpenedSiriSessionIDs: Set<String> = []
    /// Single funnel every snapshot apply (live stream, cache replay,
    /// recovery, post-command replay) routes through. Owns the ordering
    /// state (build generation, seq acceptance) that used to be split across
    /// three separate guard mechanisms on this model.
    @ObservationIgnored private let snapshotIngest: CompanionSnapshotIngest
    /// Per-session live-reply buffers, keyed by thread/session id. Kept off
    /// the model's own `@Observable` tracking so a chunk landing for one
    /// session only invalidates views reading that session's buffer, not
    /// every view reading `CompanionAppModel`.
    @ObservationIgnored private var streamingReplyBuffers: [String: StreamingReplyBuffer] = [:]

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
        // Built as a local first (rather than read back via `self.snapshotState`)
        // so constructing `snapshotIngest` around it below doesn't require
        // every other stored property to already be initialized.
        let snapshotState = CompanionSnapshotStateStore()
        self.snapshotState = snapshotState
        let sessionRuntime = environment.sessionRuntime
            ?? providedSessionRuntime
            ?? CompanionSessionRuntime.liveDefault()
        self.connection = CompanionConnectionRuntime(sessionRuntime: sessionRuntime)

        let didActivateBundledConnection = reloadsServiceFromStoredConnection &&
            CompanionConfiguration.activateBundledConnectionIfNeeded()
        let activeEnvironment = didActivateBundledConnection
            ? CompanionEnvironment.live(
                sessionRuntime: sessionRuntime
            )
            : environment
        service = activeEnvironment.service
        snapshotIngest = CompanionSnapshotIngest(snapshotState: snapshotState)
        commandDispatcher = CompanionCommandDispatcher(delegate: self)
        connectionCoordinator = CompanionConnectionCoordinator(delegate: self)
        pushCoordinator = CompanionPushCoordinator(
            notificationManager: notificationManager,
            remotePushRegistrar: remotePushRegistrar,
            connection: connection,
            delegate: self
        )
        bootstrapLoader = CompanionBootstrapLoader(delegate: self)
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
        if didActivateBundledConnection {
            CompanionDiagnostics.record("model:bundled-connection-activated")
        }

        snapshotIngest.configure(
            currentStreamGeneration: { [weak self] in self?.connection.currentStreamGeneration ?? 0 },
            currentRealtimeLatestSeq: { [weak self] in self?.realtimeLatestSeq ?? 0 },
            onLiveStreamResult: { [weak self] result in
                self?.handleLiveStreamIngestResult(result)
            }
        )

        connection.configure(callbacks: CompanionConnectionRuntimeCallbacks(
            applySnapshotUpdate: { [weak self] update, streamGeneration in
                self?.applySessionMiniSyncUpdate(update, streamGeneration: streamGeneration)
            },
            applyLiveness: { [weak self] liveness in
                self?.applySessionMiniLivenessUpdate(liveness)
            },
            applyTextChunk: { [weak self] chunk in
                self?.applyStreamingTextChunk(chunk)
            },
            refreshPendingPromptDeliveryState: { [weak self] in
                self?.refreshPendingPromptDeliveryState()
            },
            recoverySnapshotArrived: { [weak self] recovered in
                _ = self?.ingestReplaySnapshot(
                    recovered.snapshot,
                    reason: "refresh-recovery",
                    latestSeq: recovered.latestSeq
                )
            },
            replayLocalStore: { [weak self] reason in
                self?.restoreCachedSessionMiniSnapshotIfAvailable(reason: reason) ?? false
            }
        ))

        _ = restoreCachedSessionMiniSnapshotIfAvailable(
            reason: CachedSnapshotRestoreReason.appLaunch
        )

        configureStopQuickActions()
        SessionQuickActionCenter.shared.configureSessionRuntime(connection.sessionRuntime)
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
                snapshotIngest.invalidatePendingBuilds()
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

    /// Deleted the HTTP snapshot hot path's own load-state tracking in A7:
    /// there is nothing left to load but the local truth, so "loading" is
    /// just "no snapshot yet and the runtime hasn't proven liveness".
    var isLoading: Bool {
        !snapshotState.hasSnapshot && connection.machine.phase == .connecting
    }

    private var connectionActions: CompanionConnectionCoordinator {
        guard let connectionCoordinator else {
            preconditionFailure("Connection coordinator used before initialization")
        }
        return connectionCoordinator
    }

    private var commands: CompanionCommandDispatcher {
        guard let commandDispatcher else {
            preconditionFailure("Command dispatcher used before initialization")
        }
        return commandDispatcher
    }

    private var push: CompanionPushCoordinator {
        guard let pushCoordinator else {
            preconditionFailure("Push coordinator used before initialization")
        }
        return pushCoordinator
    }

    private var bootstrap: CompanionBootstrapLoader {
        guard let bootstrapLoader else {
            preconditionFailure("Bootstrap loader used before initialization")
        }
        return bootstrapLoader
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
        startNetworkPathMonitoringIfNeeded()
        startNotificationReplyOutboxDrainIfNeeded()

        CompanionDiagnostics.record("snapshot:load-skip-state-mini-prepare")
        push.registerForRemoteNotificationsInBackground()
    }

    private func shouldReloadServiceFromStoredConnection() -> Bool {
        CompanionConfiguration.resolvedConnectionFingerprint() != activeServiceConnectionFingerprint
    }

    func stopSessionRuntimeSync() {
        connection.stopSync()
        snapshotIngest.invalidatePendingBuilds()
        markSessionStreamStopped(reconnectInProgress: false)
    }

    private func startNetworkPathMonitoringIfNeeded() {
        connection.startNetworkPathMonitoringIfNeeded()
    }

    private func stopSessionRuntimeSyncForRestart() {
        connection.stopSync(reconnectInProgress: true)
        snapshotIngest.invalidatePendingBuilds()
        markSessionStreamStopped(reconnectInProgress: true)
    }

    func startSessionRuntimeSyncIfNeeded() {
        if !realtimeStreamIsLive {
            realtimeReconnectInProgress = true
        }
        connection.startSyncIfNeeded()
    }

    func applySessionMiniSyncUpdate(
        _ update: CompanionSessionMiniSyncUpdate,
        streamGeneration: UInt64
    ) {
        snapshotIngest.ingestLiveStreamUpdate(update, streamGeneration: streamGeneration)
    }

    /// Returns the live-reply buffer for a session, creating it on first
    /// access. Callers (SessionDetailScreen, SessionRow) hold the returned
    /// instance and read its `@Observable` properties directly, so a chunk
    /// only invalidates the views actually showing that session's reply.
    func streamingReplyBuffer(forSessionID sessionID: String) -> StreamingReplyBuffer {
        if let existing = streamingReplyBuffers[sessionID] {
            return existing
        }
        let buffer = StreamingReplyBuffer()
        streamingReplyBuffers[sessionID] = buffer
        return buffer
    }

    /// Fast path: feeds the chunk straight into its session's buffer, ahead
    /// of (and independent from) `applySessionMiniSyncUpdate`'s full snapshot
    /// projection, so the visible text updates the instant the chunk lands.
    private func applyStreamingTextChunk(_ chunk: ClientTextChunk) {
        streamingReplyBuffer(forSessionID: chunk.threadId).append(chunk)
    }

    private func applySessionMiniLivenessUpdate(_ update: CompanionSessionMiniLivenessUpdate) {
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

    /// Single writer for the realtime presentation fields. Moved verbatim from
    /// CompanionConnectionController when the runtime absorbed it; A8 replaces
    /// these fields with projections of `connection.machine`.
    @discardableResult
    func applyRealtimeStreamLiveness(
        serverTime: String,
        latestSeq: Int64,
        isLive: Bool,
        endpointURL: URL?,
        recordedAt: Date = Date()
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
        if isLive {
            lastRealtimeDataAt = recordedAt
        }
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
        let nextReconnectInProgress = isLive ? false : connection.isSyncing
        if realtimeReconnectInProgress != nextReconnectInProgress {
            realtimeReconnectInProgress = nextReconnectInProgress
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

    func sessionAuthoritativeConnectionState(
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

    func sessionAuthoritativeErrorMessage(
        shouldSuppressProjectionError: Bool,
        error: Error
    ) -> String? {
        if realtimeStreamIsLive || shouldSuppressProjectionError {
            return nil
        }

        return error.localizedDescription
    }

    private func markSessionStreamStopped(reconnectInProgress: Bool) {
        realtimeStreamIsLive = false
        activeSessionRouteBaseURL = nil
        realtimeReconnectInProgress = reconnectInProgress
        if connectionState == .connected {
            connectionState = .connecting
        }
    }

    private func resetRealtimeState() {
        activeSessionRouteBaseURL = nil
        realtimeServerTime = nil
        realtimeLatestSeq = 0
        realtimeStreamIsLive = false
        lastRealtimeDataAt = nil
        realtimeReconnectInProgress = false
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
        !isLive && latestSeq == 0
    }

    private func prepareSessionRuntimeInBackground() {
        let sessionRuntime = connection.sessionRuntime
        Task.detached(priority: .userInitiated) {
            await sessionRuntime?.prepareSessionRuntime()
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
        await bootstrapConnection()
    }

    /// Bounded pairing/bootstrap: resolve server health once to adopt any
    /// advertised base URLs (the only remaining `service.resolveServerHealth()`
    /// call site outside the endpoint plan cache), then wait for the realtime
    /// stream to prove liveness instead of blocking on a full HTTP snapshot
    /// fetch. A stream that never confirms in time maps through the same
    /// connection-failure reducer the deleted HTTP hot path used, so
    /// locked/unauthorized/offline handling and Face ID lock overlay are
    /// unaffected.
    private func bootstrapConnection() async {
        let healthError = await resolveBootstrapServerHealth()

        switch await bootstrap.awaitPhase(timeout: CompanionMetrics.bootstrapConnectTimeout) {
        case .connected:
            return
        case let .blocked(state):
            CompanionDiagnostics.record("bootstrap:blocked state=\(state.rawValue)")
        case .timedOut:
            guard let healthError else {
                CompanionDiagnostics.record("bootstrap:connect-timeout-no-health-error")
                return
            }
            applyConnectionFailure(healthError, suppressErrorWhenSnapshotUsable: true)
        }
    }

    @discardableResult
    private func resolveBootstrapServerHealth() async -> Error? {
        do {
            let resolvedHealth = try await service.resolveServerHealth()
            serverHealth = resolvedHealth.health
            reachedBaseURL = resolvedHealth.reachedBaseURL
            await adoptServerHealthBaseURLsIfNeeded(resolvedHealth)
            return nil
        } catch {
            serverHealth = nil
            reachedBaseURL = nil
            CompanionDiagnostics.record(
                "bootstrap:health-load-failed-clear-route error=\(error.localizedDescription)"
            )
            return error
        }
    }

    private func resetConnectionStateForStoredConnection(clearsSnapshotCache: Bool) async {
        stopSessionRuntimeSyncForRestart()
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        stopNotificationReplyOutboxDrain()
        serverHealth = nil
        reachedBaseURL = nil
        resetRealtimeState()
        snapshotIngest.resetOrdering()
        connectionState = .connecting
        pendingOpenSessionID = nil
        errorMessage = nil

        if clearsSnapshotCache {
            streamingReplyBuffers.removeAll()
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
        isAwaitingRouteSessionProof = true
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        applyLiveEnvironmentFromSessionCore()
        activeServiceConnectionFingerprint = CompanionConfiguration.resolvedConnectionFingerprint()
        errorMessage = nil
        serverHealth = nil
        reachedBaseURL = nil
        stopSessionRuntimeSyncForRestart()

        restartSessionRuntimeSyncForRouteChange()
    }

    private func restartSessionRuntimeSyncForRouteChange() {
        if connection.isSyncing {
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
        await push.refreshLocalNotificationStatus()
    }

    func enableLocalNotifications() async {
        await push.enableLocalNotifications()
    }

    func sendTestAlert() async {
        await push.sendTestAlert()
    }

    func sendLaunchVerificationAlertIfRequested() async {
        await push.sendLaunchVerificationAlertIfRequested()
    }

    func refresh() async {
        connection.requestRefresh(.manual)
    }

    /// Post-unlock recovery: restart the stream, replay local state, and
    /// backfill in the background. Never blocks the unlock interaction.
    func handleUnlock() {
        startSessionRuntimeSyncIfNeeded()
        _ = restoreCachedSessionMiniSnapshotIfAvailable(reason: "unlock-recovery")
        connection.requestRefresh(.foreground)
    }

    /// Local-first open support: replay whatever the local store has so the
    /// session can render immediately, then refresh in the background.
    private func ensureLocalSessionStateForOpen(reason: String) {
        startSessionRuntimeSyncIfNeeded()
        _ = restoreCachedSessionMiniSnapshotIfAvailable(reason: reason)
        connection.requestRefresh(.sessionOpen)
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
            sessionRuntime: connection.sessionRuntime
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
            ensureLocalSessionStateForOpen(reason: "continuation-without-session")
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
            ensureLocalSessionStateForOpen(reason: "siri-open")
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
            ensureLocalSessionStateForOpen(reason: "continuation")
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
        guard connection.isSyncing else {
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

        guard let targetRuntime = connection.sessionRuntime else {
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

    private func bumpSessionDetailRevision(reason: String, latestSeq: Int64) {
        sessionDetailRevision &+= 1
        CompanionDiagnostics.record(
            "session-detail:revision-bump reason=\(reason) seq=\(latestSeq) revision=\(sessionDetailRevision)"
        )
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
        await commands.applyMode(preset, to: sessionID)
    }

    @discardableResult
    func beginApplyMode(_ preset: SessionMode?, to sessionID: String) -> Task<Bool, Never> {
        commands.beginApplyMode(preset, to: sessionID)
    }

    func setSessionArchived(_ archived: Bool, sessionID: String) async {
        await commands.setSessionArchived(archived, sessionID: sessionID)
    }

    func deleteSession(_ sessionID: String) async {
        await commands.deleteSession(sessionID)
    }

    @discardableResult
    func sendSessionPrompt(
        _ prompt: String,
        intent: CompanionPromptIntent = .steer,
        to sessionID: String
    ) async -> Bool {
        await commands.sendSessionPrompt(prompt, intent: intent, to: sessionID)
    }

    @discardableResult
    func beginSendSessionPrompt(
        _ prompt: String,
        intent: CompanionPromptIntent = .steer,
        to sessionID: String
    ) -> Task<Bool, Never> {
        commands.beginSendSessionPrompt(prompt, intent: intent, to: sessionID)
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        prompt: String,
        to sessionID: String
    ) async -> Bool {
        await commands.submitNotificationReply(
            notificationID: notificationID,
            prompt: prompt,
            to: sessionID
        )
    }

    @discardableResult
    func drainPendingNotificationReplies() async -> Bool {
        await push.drainPendingNotificationReplies { [weak self] in
            await self?.submitPendingNotificationReply() ?? false
        }
    }

    @discardableResult
    private func startNotificationReplyOutboxDrainIfNeeded() -> Task<Bool, Never>? {
        push.startNotificationReplyOutboxDrainIfNeeded(
            submit: { [weak self] in
                await self?.submitPendingNotificationReply() ?? false
            }
        )
    }

    private func stopNotificationReplyOutboxDrain() {
        push.stopNotificationReplyOutboxDrain()
    }

    @discardableResult
    private func submitPendingNotificationReply() async -> Bool {
        await commands.submitPendingNotificationReply()
    }

    func muteSession(_ sessionID: String) async {
        await commands.muteSession(sessionID)
    }

    @discardableResult
    func setSiriDefaultSession(_ session: SessionSummary) async -> Bool {
        await commands.setSiriDefaultSession(session)
    }

    @discardableResult
    func setSiriDefaultSession(
        _ sessionID: String,
        assistantSurface requestedSurface: CompanionAssistantSurface? = nil
    ) async -> Bool {
        await commands.setSiriDefaultSession(
            sessionID,
            assistantSurface: requestedSurface
        )
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
        await commands.markCurrentSiriSession(session)
    }

    @discardableResult
    func markCurrentSiriSession(
        _ sessionID: String,
        assistantSurface requestedSurface: CompanionAssistantSurface? = nil
    ) async -> Bool {
        await commands.markCurrentSiriSession(
            sessionID,
            assistantSurface: requestedSurface
        )
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
                ensureLocalSessionStateForOpen(reason: "quick-action-open")
            }
            _ = selectAssistantSurfaceContainingSessionIfAvailable(sessionID)
            _ = openSessionFromLocalTruth(sessionID, diagnosticPrefix: "quick-action-open")
        case .continueChat:
            if snapshot == nil {
                ensureLocalSessionStateForOpen(reason: "quick-action-continue")
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
        push.configureStopQuickActions()
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

        isSavingDefaultPrompt = true
        defer {
            isSavingDefaultPrompt = false
        }

        return await commands.saveDefaultPrompt(defaultPrompt)
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
        snapshotIngest.invalidatePendingBuilds()
        logAssistantSurfaceSelection(AssistantSurfaceSelectionLogEvent.applied, surface: surface)
        CompanionDiagnostics.record("assistant-surface:selected-local surface=\(surface.rawValue)")
        errorMessage = nil
        lastUpdatedAt = Date()
        AssistantSurfaceETTraceMetric.postEnded(for: surface)

        return true
    }

    private func applyConnectionFailure(
        _ error: Error,
        suppressErrorWhenSnapshotUsable: Bool
    ) {
        commands.applyConnectionFailure(
            error,
            suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
        )
    }

    @discardableResult
    private func restoreCachedSessionMiniSnapshotIfAvailable(
        reason: String,
        bypassesSeqGating: Bool = false
    ) -> Bool {
        connection.restoreCachedSnapshotIfAvailable(reason: reason) { [weak self] cachedSnapshot, reason, latestSeq in
            self?.ingestReplaySnapshot(
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
        let didRestore = restoreCachedSessionMiniSnapshotIfAvailable(
            reason: reason,
            bypassesSeqGating: true
        )
        refreshPendingPromptDeliveryState()
        return didRestore
    }

    @discardableResult
    private func refreshPendingPromptDeliveryState(now: Date = Date()) -> Bool {
        let pendingCommands = connection.sessionRuntime?.pendingCommands() ?? []
        let didChange = snapshotState.applyPendingCommands(pendingCommands, now: now)
        if didChange {
            lastUpdatedAt = now
        }
        return didChange
    }

    /// Cache replay / recovery / post-command entry point into the ingest
    /// funnel. Shared by app-launch restore, unlock recovery, session-open
    /// replay, background refresh recovery, and
    /// `applyAcceptedClientCoreLocalSnapshot`'s seq-gating bypass.
    @discardableResult
    private func ingestReplaySnapshot(
        _ snapshot: MobileSnapshot,
        reason: String,
        latestSeq: Int64,
        bypassesSeqGating: Bool = false
    ) -> Bool {
        handleReplayIngestResult(
            snapshotIngest.ingestReplaySnapshot(
                snapshot,
                reason: reason,
                latestSeq: latestSeq,
                bypassesSeqGating: bypassesSeqGating
            )
        )
    }

    /// Live-stream completion handler: the only apply path that toggles
    /// realtime liveness, and the only one where a rejection still has a
    /// side effect (liveness always applies; a rejected text-chunk frame
    /// still invalidates the detail projection it would have carried).
    @discardableResult
    private func handleLiveStreamIngestResult(_ result: CompanionSnapshotIngest.IngestResult) -> Bool {
        switch result {
        case .dropped:
            return false
        case let .rejected(rejection):
            applyRealtimeStreamLiveness(
                serverTime: rejection.sourceSnapshot.host.lastSyncedAt,
                latestSeq: rejection.latestSeq,
                isLive: true,
                endpointURL: rejection.endpointURL
            )
            if rejection.reason == CompanionSessionMiniSyncReason.textChunk {
                bumpSessionDetailRevision(reason: "rejected-text-chunk", latestSeq: rejection.latestSeq)
            }
            return false
        case let .applied(outcome):
            settleAppliedIngestOutcome(outcome)
            return true
        }
    }

    /// Cache/recovery/post-command completion handler: never toggles
    /// liveness (these sources are local-only), and a total rejection (the
    /// broader-merge fallback also declined) has no side effect at all.
    @discardableResult
    private func handleReplayIngestResult(_ result: CompanionSnapshotIngest.IngestResult) -> Bool {
        switch result {
        case .dropped, .rejected:
            return false
        case let .applied(outcome):
            settleAppliedIngestOutcome(outcome)
            return true
        }
    }

    /// Bookkeeping shared by every accepted ingest outcome regardless of
    /// source: seq tracking, cache-ready/liveness signaling, and — only when
    /// the visible snapshot actually changed — the timestamp, Spotlight
    /// invalidation (direct-apply only; a broader merge preserves what's
    /// already indexed), and the text-chunk detail-revision bump.
    private func settleAppliedIngestOutcome(_ outcome: CompanionSnapshotIngest.Outcome) {
        realtimeLatestSeq = max(realtimeLatestSeq, outcome.latestSeq)
        if outcome.provesLiveness {
            applyRealtimeStreamLiveness(
                serverTime: outcome.sourceSnapshot.host.lastSyncedAt,
                latestSeq: outcome.latestSeq,
                isLive: true,
                endpointURL: outcome.endpointURL
            )
        } else {
            markCachedSnapshotReadyIfNeeded(reason: outcome.reason)
        }

        guard outcome.applyResult.didChangeVisibleSnapshot else {
            CompanionDiagnostics.record("snapshot:apply-noop reason=\(outcome.reason) seq=\(outcome.latestSeq)")
            return
        }

        lastUpdatedAt = Date()
        if outcome.strategy == .direct {
            spotlightCoordinator.clearForCachedSnapshotIfNeeded()
        }
        if outcome.reason == CompanionSessionMiniSyncReason.textChunk {
            bumpSessionDetailRevision(reason: outcome.reason, latestSeq: outcome.latestSeq)
        }
        CompanionDiagnostics.record("snapshot:apply reason=\(outcome.reason) seq=\(outcome.latestSeq)")
    }

    // MARK: - Testing seams

    func reserveSessionMiniProjectionBuildForTesting(latestSeq: Int64) -> UInt64? {
        snapshotIngest.reserveForTesting(latestSeq: latestSeq)
    }

    func waitForPendingSessionMiniProjectionForTesting() async {
        await snapshotIngest.waitForPendingBuildForTesting()
    }

    /// Testing seam mirroring the old reserve-then-apply two-step now that
    /// both halves live behind `CompanionSnapshotIngest`.
    @discardableResult
    func applyPreparedSessionMiniSyncSnapshot(
        _ preparedProjection: CompanionPreparedSnapshotProjection,
        reason: String,
        latestSeq: Int64,
        endpointURL: URL?,
        streamGeneration: UInt64,
        generation: UInt64,
        builtSurface: CompanionAssistantSurface
    ) -> Bool {
        handleLiveStreamIngestResult(
            snapshotIngest.completeForTesting(
                preparedProjection,
                reason: reason,
                latestSeq: latestSeq,
                endpointURL: endpointURL,
                streamGeneration: streamGeneration,
                reservedGeneration: generation,
                builtSurface: builtSurface
            )
        )
    }

    private func markCachedSnapshotReadyIfNeeded(reason: String) {
        guard connectionState == .connecting, snapshotState.hasSnapshot else {
            return
        }

        errorMessage = nil
        CompanionDiagnostics.record("connection:local-cache-ready reason=\(reason)")
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

extension CompanionAppModel: CompanionCommandDispatcherDelegate {
    var commandDispatcherConnection: CompanionConnectionRuntime {
        connection
    }

    var commandDispatcherSnapshotState: CompanionSnapshotStateStore {
        snapshotState
    }

    var commandDispatcherSnapshot: MobileSnapshot? {
        snapshot
    }

    var commandDispatcherServerHealth: CompanionServerHealth? {
        serverHealth
    }

    var commandDispatcherReachedBaseURL: URL? {
        reachedBaseURL
    }

    var commandDispatcherConnectionState: ConnectivityState {
        get { connectionState }
        set { connectionState = newValue }
    }

    var commandDispatcherErrorMessage: String? {
        get { errorMessage }
        set { errorMessage = newValue }
    }

    func commandDispatcherSetLastUpdatedAt(_ date: Date) {
        lastUpdatedAt = date
    }

    func commandDispatcherClearConnectionRouteStateIfNeeded(for state: ConnectivityState) {
        clearConnectionRouteStateIfNeeded(for: state)
    }

    func commandDispatcherSessionAuthoritativeConnectionState(
        _ projectedState: ConnectivityState
    ) -> ConnectivityState {
        sessionAuthoritativeConnectionState(projectedState)
    }

    func commandDispatcherSessionAuthoritativeErrorMessage(
        shouldSuppressProjectionError: Bool,
        error: Error
    ) -> String? {
        sessionAuthoritativeErrorMessage(
            shouldSuppressProjectionError: shouldSuppressProjectionError,
            error: error
        )
    }

    func commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: String) -> Bool {
        applyAcceptedClientCoreLocalSnapshot(reason: reason)
    }

    func commandDispatcherAssistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        assistantSurface(for: sessionID)
    }

    func commandDispatcherDonateSetDefaultSiriSession(_ session: SessionSummary) async {
        await donateSetDefaultSiriSession(session)
    }

    func commandDispatcherStartNotificationReplyOutboxDrainIfNeeded() {
        startNotificationReplyOutboxDrainIfNeeded()
    }
}

extension CompanionAppModel: CompanionBootstrapLoaderDelegate {
    var bootstrapLoaderConnectionState: ConnectivityState {
        connectionState
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

extension CompanionAppModel: CompanionPushCoordinatorDelegate {
    var pushCoordinatorNotificationService: any CompanionService {
        service
    }

    var pushCoordinatorCanSendLocalNotifications: Bool {
        viewState.canSendLocalNotifications
    }

    var pushCoordinatorAreLocalNotificationsDenied: Bool {
        viewState.areLocalNotificationsDenied
    }

    var pushCoordinatorRemotePushRegistration: RemotePushRegistrationResponse? {
        remotePushRegistration
    }

    func pushCoordinatorApplyLocalAuthorizationStatus(_ status: UNAuthorizationStatus) {
        localNotificationStatus = status
    }

    func pushCoordinatorSetRemotePushRegistration(_ registration: RemotePushRegistrationResponse?) {
        remotePushRegistration = registration
    }

    func pushCoordinatorSetRemotePushRegistrationInFlight(_ isRegistering: Bool) {
        isRegisteringRemotePush = isRegistering
    }

    func pushCoordinatorSetRemotePushFailureMessage(_ message: String?) {
        remotePushFailureMessage = message
    }
}

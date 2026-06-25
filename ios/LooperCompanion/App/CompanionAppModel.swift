import AppIntents
import Foundation
import LooperCompanionCore
import LooperRealtime
import Observation
import UIKit
import UserNotifications

private enum LaunchArgument {
    static let sendTestAlertOnLaunch = "--send-test-alert-on-launch"
}

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

private enum NotificationReplyOutboxRetry {
    static let initialDelayNanoseconds: UInt64 = 250_000_000
    static let maximumDelayNanoseconds: UInt64 = 30_000_000_000
    static let backoffMultiplier: UInt64 = 2
}

private enum LocalFirstMutationError: LocalizedError {
    case modeBarrierRejected

    var errorDescription: String? {
        switch self {
        case .modeBarrierRejected:
            return "Mode change was not accepted. Prompt stayed queued."
        }
    }
}

private enum PendingSessionModeSelection: Sendable {
    case globalDefault
    case preset(SessionMode)

    var mode: SessionMode? {
        switch self {
        case .globalDefault:
            return nil
        case let .preset(mode):
            return mode
        }
    }

    init(_ mode: SessionMode?) {
        if let mode {
            self = .preset(mode)
        } else {
            self = .globalDefault
        }
    }
}

private struct ModeRollbackState: Sendable {
    let snapshot: MobileSnapshot?
    let detail: SessionDetail?
}

private struct PendingSessionModeMutation: Sendable {
    let selection: PendingSessionModeSelection
    let clientMutationID: String
    let barrier: LocalFirstMutationBarrier

    var mode: SessionMode? {
        selection.mode
    }

    init(
        mode: SessionMode?,
        clientMutationID: String
    ) {
        selection = PendingSessionModeSelection(mode)
        self.clientMutationID = clientMutationID
        barrier = LocalFirstMutationBarrier()
    }
}

private struct ModeMutationEnvelope: Sendable {
    let sessionID: String
    let selection: PendingSessionModeSelection
    let clientMutationID: String
    let barrier: LocalFirstMutationBarrier
    let connectionRevision: Int
    let rollbackState: ModeRollbackState?
    let service: any CompanionService

    var mode: SessionMode? {
        selection.mode
    }
}

private struct PromptMutationEnvelope: Sendable {
    let sessionID: String
    let prompt: String
    let assistantSurface: CompanionAssistantSurface
    let clientMutationID: String
    let connectionRevision: Int
    let service: any CompanionService
    let pendingModeMutation: PendingSessionModeMutation?
    let modeBarrierTask: Task<Bool, Never>?
}

private actor LocalFirstMutationBarrier {
    private var result: Bool?
    private var continuations: [CheckedContinuation<Bool, Never>] = []

    func wait() async -> Bool {
        if let result {
            return result
        }

        return await withCheckedContinuation { continuation in
            continuations.append(continuation)
        }
    }

    func resolve(_ accepted: Bool) {
        guard result == nil else {
            return
        }

        result = accepted
        let continuations = continuations
        self.continuations.removeAll()
        for continuation in continuations {
            continuation.resume(returning: accepted)
        }
    }
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
    @ObservationIgnored private let remotePushRegistrar: RemotePushRegistrar
    @ObservationIgnored private let spotlightIndexer: SessionSpotlightIndexer
    @ObservationIgnored private let sessionMiniLocalStore: CompanionSessionMiniLocalStore?
    @ObservationIgnored private var notificationObservers: [NSObjectProtocol] = []
    @ObservationIgnored private var spotlightRecordsBySessionID: [String: SessionSpotlightRecord] = [:]
    @ObservationIgnored private var loadingSessionDetailIDs: Set<String> = []
    @ObservationIgnored private var hasRebuiltSpotlightIndexThisLaunch = false
    @ObservationIgnored private var didRequestRemotePushRegistrationThisLaunch = false
    @ObservationIgnored private var didSendLaunchVerificationAlertThisLaunch = false
    @ObservationIgnored private var sessionMiniSyncTask: Task<Void, Never>?
    @ObservationIgnored private var lastAppliedRealtimeRevision: String?
    @ObservationIgnored private var hasValidatedCurrentSnapshotWithHTTP = false
    @ObservationIgnored private var cachedSnapshotRestoreTask: Task<Void, Never>?
    @ObservationIgnored private var nextCachedSnapshotRestoreID = 0
    @ObservationIgnored private var activeCachedSnapshotRestoreID = 0
    @ObservationIgnored private var snapshotLoadTask: Task<Void, Never>?
    @ObservationIgnored private var nextSnapshotLoadID = 0
    @ObservationIgnored private var activeSnapshotLoadID = 0
    @ObservationIgnored private var isDrainingSnapshotLoads = false
    @ObservationIgnored private var hasPendingSnapshotLoad = false
    @ObservationIgnored private var hasUserSelectedAssistantSurface = false
    @ObservationIgnored private var pendingAssistantSurfaceSave: CompanionAssistantSurface?
    @ObservationIgnored private var isSavingAssistantSurface = false
    @ObservationIgnored private var connectionRevision = 0
    @ObservationIgnored private var activeServiceConnectionFingerprint = ""
    @ObservationIgnored private var modeMutationDrainTasksBySessionID: [String: Task<Bool, Never>] = [:]
    @ObservationIgnored private var modeMutationDrainIDBySessionID: [String: String] = [:]
    @ObservationIgnored private var pendingModeMutationsBySessionID: [String: [PendingSessionModeMutation]] = [:]
    @ObservationIgnored private var modeRollbackStateBySessionID: [String: ModeRollbackState] = [:]
    @ObservationIgnored private var latestModeMutationBySessionID: [String: PendingSessionModeMutation] = [:]
    @ObservationIgnored private var latestModeMutationIDBySessionID: [String: String] = [:]
    @ObservationIgnored private var latestModeMutationBarrierBySessionID: [String: LocalFirstMutationBarrier] = [:]
    #if DEBUG
    @ObservationIgnored private var modeDrainBeforeFinishHook: (() async -> Void)?
    #endif
    @ObservationIgnored private var notificationReplyOutboxDrainTask: Task<Void, Never>?
    @ObservationIgnored private var notificationReplyOutboxDrainID: String?
    @ObservationIgnored private var notificationReplyOutboxRetryTask: Task<Void, Never>?
    @ObservationIgnored private var notificationReplyOutboxRetryDelayNanoseconds =
        NotificationReplyOutboxRetry.initialDelayNanoseconds
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
        self.remotePushRegistrar = remotePushRegistrar
        self.spotlightIndexer = spotlightIndexer
        self.sessionMiniLocalStore = sessionMiniLocalStore

        let didActivateBundledConnection = reloadsServiceFromStoredConnection &&
            CompanionConfiguration.activateBundledConnectionIfNeeded()
        service = didActivateBundledConnection ? CompanionEnvironment.live().service : environment.service
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
            scheduleCachedSnapshotRestoreIfAvailable(reason: CachedSnapshotRestoreReason.appLaunch)
        }

        configureStopQuickActions()
        SessionQuickActionCenter.shared.configureLocalStore(sessionMiniLocalStore)
        registerSessionQuickActionHandler()
        registerNotificationObservers()
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
        registerForRemoteNotificationsInBackground()
    }

    private func shouldReloadServiceFromStoredConnection() -> Bool {
        CompanionConfiguration.resolvedConnectionFingerprint() != activeServiceConnectionFingerprint
    }

    func stopRealtimeSessionSync() {
        stopRealtimeSessionSync(disconnectCachedClients: true)
    }

    private func stopRealtimeSessionSync(disconnectCachedClients: Bool) {
        stopSessionMiniSync()
        if disconnectCachedClients {
            Task {
                await RealtimeCompanionClientFactory.disconnectCachedClients()
            }
        }
    }

    func startRealtimeSessionSyncIfNeeded() {
        startSessionMiniSyncIfNeeded()
    }

    private func startSessionMiniSyncIfNeeded() {
        guard sessionMiniSyncTask == nil,
              let sessionMiniLocalStore
        else {
            return
        }

        let service = service
        let connectionRevision = connectionRevision
        let synchronizer = LooperRealtimeStateMiniSynchronizer(
            store: sessionMiniLocalStore.realtimeLocalStore,
            transport: DeferredCompanionStateMiniSyncTransport(service: service)
        )

        sessionMiniSyncTask = Task { [weak self] in
            await synchronizer.runUntilCancelled { [weak self] update in
                await self?.applySessionMiniSyncUpdate(
                    update,
                    connectionRevision: connectionRevision
                )
            }
        }
    }

    #if DEBUG
    func runSessionMiniSyncCycleForSelfTest(
        transport: any LooperRealtimeStateMiniSyncTransport
    ) async -> LooperRealtimeStateMiniSyncCycleResult {
        guard let sessionMiniLocalStore else {
            return .retry(
                latestSeq: 0,
                errorDescription: "session mini local store unavailable"
            )
        }

        let connectionRevision = connectionRevision
        let synchronizer = LooperRealtimeStateMiniSynchronizer(
            store: sessionMiniLocalStore.realtimeLocalStore,
            transport: transport
        )
        return await synchronizer.runOneCycle { [weak self] update in
            await self?.applySessionMiniSyncUpdate(
                update,
                connectionRevision: connectionRevision
            )
        }
    }
    #endif

    private func stopSessionMiniSync() {
        sessionMiniSyncTask?.cancel()
        sessionMiniSyncTask = nil
    }

    private func applySessionMiniSyncUpdate(
        _ update: LooperRealtimeStateMiniSyncUpdate,
        connectionRevision: Int
    ) {
        guard connectionRevision == self.connectionRevision,
              let sessionMiniLocalStore
        else {
            CompanionDiagnostics.record("session-mini:sync-stale-skip")
            return
        }

        do {
            guard let cachedSnapshot = try sessionMiniLocalStore.cachedSnapshot() else {
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

    private func registerForRemoteNotificationsInBackground() {
        Task { @MainActor [weak self] in
            await self?.registerForRemoteNotificationsIfPossible()
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
        let shouldRestartEventStream = sessionMiniSyncTask != nil
        connectionRevision += 1
        cancelCachedSnapshotRestore()
        cancelSnapshotLoad()
        stopRealtimeSessionSync(disconnectCachedClients: false)
        configuredBaseURL = CompanionConfiguration.resolvedBaseURLString()
        selectedAssistantSurface = .defaultSurface
        hasUserSelectedAssistantSurface = false
        pendingAssistantSurfaceSave = nil
        isSavingAssistantSurface = false
        modeMutationDrainTasksBySessionID.values.forEach { task in
            task.cancel()
        }
        await resolveModeMutationBarriers(false)
        modeMutationDrainTasksBySessionID = [:]
        modeMutationDrainIDBySessionID = [:]
        pendingModeMutationsBySessionID = [:]
        modeRollbackStateBySessionID = [:]
        stopNotificationReplyOutboxDrain()
        latestModeMutationBySessionID = [:]
        latestModeMutationIDBySessionID = [:]
        latestModeMutationBarrierBySessionID = [:]
        lastAppliedRealtimeRevision = nil
        hasValidatedCurrentSnapshotWithHTTP = false
        hasPendingSnapshotLoad = false
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
            scheduleCachedSnapshotRestoreIfAvailable(reason: cachedSnapshotRestoreReason)
        }

        snapshot = nil
        sessionSections = .empty
        sessionIndex = .empty
    }

    private func scheduleCachedSnapshotRestoreIfAvailable(reason: String) {
        cachedSnapshotRestoreTask?.cancel()
        nextCachedSnapshotRestoreID += 1
        let restoreID = nextCachedSnapshotRestoreID
        activeCachedSnapshotRestoreID = restoreID
        let restoreRevision = connectionRevision

        cachedSnapshotRestoreTask = Task { @MainActor [weak self] in
            guard let self else {
                return
            }

            _ = await self.restoreCachedSnapshotIfAvailable(
                reason: reason,
                onlyWhenSnapshotMissing: true,
                restoreRevision: restoreRevision
            )
            self.finishCachedSnapshotRestore(id: restoreID)
        }
    }

    private func finishCachedSnapshotRestore(id: Int) {
        guard id == activeCachedSnapshotRestoreID else {
            return
        }

        cachedSnapshotRestoreTask = nil
    }

    private func cancelCachedSnapshotRestore() {
        cachedSnapshotRestoreTask?.cancel()
        cachedSnapshotRestoreTask = nil
        activeCachedSnapshotRestoreID = 0
    }

    private func cancelSnapshotLoad() {
        snapshotLoadTask?.cancel()
        snapshotLoadTask = nil
        isLoading = false
        isDrainingSnapshotLoads = false
        hasPendingSnapshotLoad = false
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
        #if DEBUG
        if UITestLaunchArguments.isMockModeEnabled {
            localNotificationStatus = .authorized
            Haptics.success()
            return
        }
        #endif

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
        if isDrainingSnapshotLoads, !allowsConcurrentConnectionReload {
            hasPendingSnapshotLoad = true
            CompanionDiagnostics.lifecycle.info("Snapshot load coalesced behind active load")
            CompanionDiagnostics.record("snapshot:load-coalesced")
            await snapshotLoadTask?.value
            return
        }

        if isDrainingSnapshotLoads, allowsConcurrentConnectionReload {
            snapshotLoadTask?.cancel()
            hasPendingSnapshotLoad = false
        }

        isDrainingSnapshotLoads = true
        defer {
            isDrainingSnapshotLoads = false
        }

        repeat {
            hasPendingSnapshotLoad = false
            await loadSnapshotOnce()
        } while shouldDrainPendingSnapshotLoad()
    }

    private func loadSnapshotOnce() async {
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

    private func shouldDrainPendingSnapshotLoad() -> Bool {
        guard hasPendingSnapshotLoad else {
            return false
        }

        guard !Task.isCancelled else {
            hasPendingSnapshotLoad = false
            return false
        }

        CompanionDiagnostics.record("snapshot:load-drain-pending")
        return true
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
        guard sessionMiniSyncTask == nil || snapshot == nil else {
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
        guard sessionMiniSyncTask != nil else {
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
        let drainTask = beginApplyMode(preset, to: sessionID)
        _ = await drainTask.value
    }

    @discardableResult
    func beginApplyMode(_ preset: SessionMode?, to sessionID: String) -> Task<Bool, Never> {
        if modeRollbackStateBySessionID[sessionID] == nil {
            modeRollbackStateBySessionID[sessionID] = ModeRollbackState(
                snapshot: snapshot,
                detail: detailBySessionID[sessionID]
            )
        }

        let clientMutationID = makeClientMutationID()
        let pendingMutation = PendingSessionModeMutation(
            mode: preset,
            clientMutationID: clientMutationID
        )
        applyOptimisticMode(preset, to: sessionID)
        enqueueLocalModeCommand(
            sessionID: sessionID,
            preset: preset,
            clientMutationID: clientMutationID
        )
        latestModeMutationBySessionID[sessionID] = pendingMutation
        latestModeMutationIDBySessionID[sessionID] = clientMutationID
        latestModeMutationBarrierBySessionID[sessionID] = pendingMutation.barrier

        if modeMutationDrainTasksBySessionID[sessionID] != nil {
            pendingModeMutationsBySessionID[sessionID, default: []].append(pendingMutation)
            return modeMutationBarrierTask(pendingMutation.barrier)
        }

        let envelope = makeModeMutationEnvelope(pendingMutation, sessionID: sessionID)
        let drainID = makeClientMutationID()
        let drainTask = makeModeMutationDrainTask(first: envelope, drainID: drainID)
        modeMutationDrainTasksBySessionID[sessionID] = drainTask
        modeMutationDrainIDBySessionID[sessionID] = drainID
        return modeMutationBarrierTask(pendingMutation.barrier)
    }

    private func modeMutationBarrierTask(
        _ barrier: LocalFirstMutationBarrier
    ) -> Task<Bool, Never> {
        Task.detached(priority: .userInitiated) {
            await barrier.wait()
        }
    }

    private func makeModeMutationDrainTask(
        first envelope: ModeMutationEnvelope,
        drainID: String
    ) -> Task<Bool, Never> {
        Task.detached(priority: .userInitiated) { [weak self] in
            var nextEnvelope: ModeMutationEnvelope? = envelope
            var didAcceptLatestMutation = true
            while !Task.isCancelled, let currentEnvelope = nextEnvelope {
                let didAcceptMutation = await Self.sendModeMutation(currentEnvelope, model: self)
                await currentEnvelope.barrier.resolve(didAcceptMutation)
                if !didAcceptMutation {
                    didAcceptLatestMutation = false
                    break
                }
                nextEnvelope = await self?.nextModeMutationEnvelope(for: currentEnvelope.sessionID)
            }
            if Task.isCancelled, let unresolvedEnvelope = nextEnvelope {
                await unresolvedEnvelope.barrier.resolve(false)
            }
            #if DEBUG
            await self?.runModeDrainBeforeFinishHookIfNeeded()
            #endif
            await self?.finishModeMutationDrain(for: envelope.sessionID, drainID: drainID)
            return didAcceptLatestMutation && !Task.isCancelled
        }
    }

    private func nextModeMutationEnvelope(for sessionID: String) -> ModeMutationEnvelope? {
        guard var pendingMutations = pendingModeMutationsBySessionID[sessionID],
              !pendingMutations.isEmpty
        else {
            return nil
        }

        let mutation = pendingMutations.removeFirst()
        pendingModeMutationsBySessionID[sessionID] = pendingMutations.isEmpty ? nil : pendingMutations
        return makeModeMutationEnvelope(mutation, sessionID: sessionID)
    }

    private func finishModeMutationDrain(for sessionID: String, drainID: String) {
        guard modeMutationDrainIDBySessionID[sessionID] == drainID else {
            CompanionDiagnostics.record("mode:stale-drain-finish-skip sessionID=\(sessionID)")
            return
        }

        modeMutationDrainTasksBySessionID[sessionID] = nil
        modeMutationDrainIDBySessionID[sessionID] = nil
        guard let nextEnvelope = nextModeMutationEnvelope(for: sessionID) else {
            modeRollbackStateBySessionID[sessionID] = nil
            latestModeMutationBySessionID[sessionID] = nil
            latestModeMutationIDBySessionID[sessionID] = nil
            latestModeMutationBarrierBySessionID[sessionID] = nil
            return
        }

        let nextDrainID = makeClientMutationID()
        let drainTask = makeModeMutationDrainTask(first: nextEnvelope, drainID: nextDrainID)
        modeMutationDrainTasksBySessionID[sessionID] = drainTask
        modeMutationDrainIDBySessionID[sessionID] = nextDrainID
    }

    private func resolveModeMutationBarriers(_ accepted: Bool) async {
        let barriers = Array(latestModeMutationBarrierBySessionID.values)
            + pendingModeMutationsBySessionID.values.flatMap { mutations in
                mutations.map(\.barrier)
            }
        for barrier in barriers {
            await barrier.resolve(accepted)
        }
    }

    #if DEBUG
    func setModeDrainBeforeFinishHookForSelfTest(_ hook: (() async -> Void)?) {
        modeDrainBeforeFinishHook = hook
    }

    private func runModeDrainBeforeFinishHookIfNeeded() async {
        guard let hook = modeDrainBeforeFinishHook else {
            return
        }

        modeDrainBeforeFinishHook = nil
        await hook()
    }
    #endif

    private func makeModeMutationEnvelope(
        _ mutation: PendingSessionModeMutation,
        sessionID: String
    ) -> ModeMutationEnvelope {
        ModeMutationEnvelope(
            sessionID: sessionID,
            selection: mutation.selection,
            clientMutationID: mutation.clientMutationID,
            barrier: mutation.barrier,
            connectionRevision: connectionRevision,
            rollbackState: modeRollbackStateBySessionID[sessionID],
            service: service
        )
    }

    private nonisolated static func sendModeMutation(
        _ envelope: ModeMutationEnvelope,
        model: CompanionAppModel?
    ) async -> Bool {
        guard !Task.isCancelled else {
            return false
        }

        do {
            let result = try await envelope.service.setSessionMode(
                id: envelope.sessionID,
                preset: envelope.mode,
                clientMutationID: envelope.clientMutationID
            )
            guard !Task.isCancelled else {
                return false
            }
            return await model?.handleModeMutationSuccess(result, envelope: envelope) ?? false
        } catch {
            guard !Task.isCancelled else {
                return false
            }
            return await model?.handleModeMutationFailure(error, envelope: envelope) ?? false
        }
    }

    private func handleModeMutationSuccess(
        _ result: CompanionSessionModeResult,
        envelope: ModeMutationEnvelope
    ) async -> Bool {
        markLocalCommandDelivered(result.clientMutationID ?? envelope.clientMutationID)
        guard envelope.connectionRevision == connectionRevision else {
            CompanionDiagnostics.record("mode:mutation-stale-skip sessionID=\(envelope.sessionID)")
            return false
        }

        guard latestModeMutationIDBySessionID[envelope.sessionID] == envelope.clientMutationID else {
            CompanionDiagnostics.record("mode:mutation-superseded-skip sessionID=\(envelope.sessionID)")
            return true
        }

        await applyModeMutationResult(result, sessionID: envelope.sessionID)
        return true
    }

    private func handleModeMutationFailure(
        _ error: Error,
        envelope: ModeMutationEnvelope
    ) -> Bool {
        guard envelope.connectionRevision == connectionRevision else {
            CompanionDiagnostics.record(
                "mode:mutation-stale-error-skip sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return false
        }

        guard latestModeMutationIDBySessionID[envelope.sessionID] == envelope.clientMutationID else {
            CompanionDiagnostics.record(
                "mode:mutation-superseded-error-skip sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return true
        }

        restoreOptimisticModeSnapshot(
            envelope.rollbackState?.snapshot,
            previousDetail: envelope.rollbackState?.detail,
            sessionID: envelope.sessionID
        )
        connectionState = connectionState(for: error)
        clearConnectionRouteStateIfNeeded(for: connectionState)
        errorMessage = error.localizedDescription
        Haptics.error()
        return false
    }

    private func applyModeMutationResult(
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
        let promptTask = beginSendSessionPrompt(prompt, to: sessionID)
        return await promptTask.value
    }

    @discardableResult
    func beginSendSessionPrompt(_ prompt: String, to sessionID: String) -> Task<Bool, Never> {
        let targetSurface = sessionIndex.assistantSurface(containingSessionID: sessionID)
            ?? selectedAssistantSurface

        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPrompt.isEmpty else {
            errorMessage = "Prompt is required."
            Haptics.warning()
            return Task.detached { false }
        }

        guard !mutatingSessionIDs.contains(sessionID) else {
            return Task.detached { false }
        }

        let clientMutationID = makeClientMutationID()
        let pendingModeMutation = service.supportsModePromptBatch
            ? latestModeMutationBySessionID[sessionID]
            : nil
        let modeBarrierTask = pendingModeMutation.map {
            modeMutationBarrierTask($0.barrier)
        }
        enqueueLocalPromptCommand(
            sessionID: sessionID,
            prompt: trimmedPrompt,
            assistantSurface: targetSurface,
            clientMutationID: clientMutationID
        )

        let mutationRevision = connectionRevision
        setSessionMutation(true, sessionID: sessionID)
        let envelope = PromptMutationEnvelope(
            sessionID: sessionID,
            prompt: trimmedPrompt,
            assistantSurface: targetSurface,
            clientMutationID: clientMutationID,
            connectionRevision: mutationRevision,
            service: service,
            pendingModeMutation: pendingModeMutation,
            modeBarrierTask: modeBarrierTask
        )

        return Task.detached(priority: .userInitiated) { [weak self] in
            await Self.sendPromptMutation(envelope, model: self)
        }
    }

    private nonisolated static func sendPromptMutation(
        _ envelope: PromptMutationEnvelope,
        model: CompanionAppModel?
    ) async -> Bool {
        if let pendingModeMutation = envelope.pendingModeMutation {
            do {
                let result = try await envelope.service.sendSessionPromptAfterMode(
                    id: envelope.sessionID,
                    modePreset: pendingModeMutation.mode,
                    modeClientMutationID: pendingModeMutation.clientMutationID,
                    prompt: envelope.prompt,
                    assistantSurface: envelope.assistantSurface,
                    promptClientMutationID: envelope.clientMutationID
                )
                return await model?.handleModePromptBatchSuccess(
                    result,
                    pendingModeMutation: pendingModeMutation,
                    envelope: envelope
                ) ?? false
            } catch {
                CompanionDiagnostics.record(
                    "prompt:mode-batch-failed sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
                )
                return await model?.handlePromptMutationFailure(error, envelope: envelope) ?? false
            }
        }

        if let modeBarrierTask = envelope.modeBarrierTask {
            let didAcceptMode = await modeBarrierTask.value
            guard didAcceptMode else {
                return await model?.handlePromptMutationFailure(
                    LocalFirstMutationError.modeBarrierRejected,
                    envelope: envelope
                ) ?? false
            }
        }

        do {
            let result = try await envelope.service.sendSessionPrompt(
                id: envelope.sessionID,
                prompt: envelope.prompt,
                assistantSurface: envelope.assistantSurface,
                clientMutationID: envelope.clientMutationID
            )
            return await model?.handlePromptMutationSuccess(result, envelope: envelope) ?? false
        } catch {
            return await model?.handlePromptMutationFailure(error, envelope: envelope) ?? false
        }
    }

    private func handleModePromptBatchSuccess(
        _ result: CompanionModePromptBatchResult,
        pendingModeMutation: PendingSessionModeMutation,
        envelope: PromptMutationEnvelope
    ) async -> Bool {
        markLocalCommandDelivered(result.mode.clientMutationID ?? pendingModeMutation.clientMutationID)
        await finishModeMutationDeliveredByBatch(
            pendingModeMutation,
            sessionID: envelope.sessionID
        )
        return await handlePromptMutationSuccess(result.prompt, envelope: envelope)
    }

    private func finishModeMutationDeliveredByBatch(
        _ mutation: PendingSessionModeMutation,
        sessionID: String
    ) async {
        await mutation.barrier.resolve(true)

        if var pendingMutations = pendingModeMutationsBySessionID[sessionID] {
            pendingMutations.removeAll { pendingMutation in
                pendingMutation.clientMutationID == mutation.clientMutationID
            }
            pendingModeMutationsBySessionID[sessionID] = pendingMutations.isEmpty
                ? nil
                : pendingMutations
        }

        let batchedMutationWasLatest =
            latestModeMutationIDBySessionID[sessionID] == mutation.clientMutationID
        if batchedMutationWasLatest {
            latestModeMutationBySessionID[sessionID] = nil
            latestModeMutationIDBySessionID[sessionID] = nil
            latestModeMutationBarrierBySessionID[sessionID] = nil
        }

        modeMutationDrainTasksBySessionID[sessionID]?.cancel()
        modeMutationDrainTasksBySessionID[sessionID] = nil
        modeMutationDrainIDBySessionID[sessionID] = nil

        guard let nextEnvelope = nextModeMutationEnvelope(for: sessionID) else {
            if batchedMutationWasLatest {
                modeRollbackStateBySessionID[sessionID] = nil
            }
            return
        }

        let nextDrainID = makeClientMutationID()
        modeMutationDrainTasksBySessionID[sessionID] = makeModeMutationDrainTask(
            first: nextEnvelope,
            drainID: nextDrainID
        )
        modeMutationDrainIDBySessionID[sessionID] = nextDrainID
    }

    private func handlePromptMutationSuccess(
        _ result: CompanionPromptSendResult,
        envelope: PromptMutationEnvelope
    ) async -> Bool {
        defer {
            setSessionMutation(false, sessionID: envelope.sessionID)
        }

        markLocalCommandDelivered(result.clientMutationID ?? envelope.clientMutationID)
        guard envelope.connectionRevision == connectionRevision else {
            CompanionDiagnostics.record("prompt:mutation-stale-skip sessionID=\(envelope.sessionID)")
            return false
        }

        await applyPromptSendResult(
            result,
            sessionID: envelope.sessionID,
            assistantSurface: envelope.assistantSurface
        )
        return true
    }

    private func handlePromptMutationFailure(
        _ error: Error,
        envelope: PromptMutationEnvelope
    ) -> Bool {
        defer {
            setSessionMutation(false, sessionID: envelope.sessionID)
        }

        guard envelope.connectionRevision == connectionRevision else {
            CompanionDiagnostics.record(
                "prompt:mutation-stale-error-skip sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return false
        }

        connectionState = connectionState(for: error)
        clearConnectionRouteStateIfNeeded(for: connectionState)
        errorMessage = error.localizedDescription
        Haptics.error()
        return false
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        prompt: String,
        to sessionID: String,
        clientMutationID providedClientMutationID: String? = nil
    ) async -> Bool {
        let trimmedNotificationID = notificationID.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedNotificationID.isEmpty else {
            errorMessage = "Notification reply is missing its delivery ID."
            Haptics.warning()
            return false
        }
        guard !trimmedPrompt.isEmpty else {
            errorMessage = "Prompt is required."
            Haptics.warning()
            return false
        }

        let targetSurface = sessionIndex.assistantSurface(containingSessionID: sessionID)
            ?? selectedAssistantSurface
        let providedMutationID = providedClientMutationID?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let clientMutationID = providedMutationID.isEmpty
            ? SessionQuickActionRequest.notificationReplyClientMutationID(
                notificationID: trimmedNotificationID
            )
            : providedMutationID
        enqueueLocalNotificationReplyCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt,
            clientMutationID: clientMutationID
        )

        return await sendNotificationReplyCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt,
            targetSurface: targetSurface,
            clientMutationID: clientMutationID
        )
    }

    @discardableResult
    private func sendNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        targetSurface: CompanionAssistantSurface,
        clientMutationID: String
    ) async -> Bool {
        do {
            let response = try await service.submitNotificationReply(
                notificationID: notificationID,
                sessionID: sessionID,
                prompt: prompt,
                assistantSurface: nil,
                clientMutationID: clientMutationID
            )
            markLocalCommandDelivered(response.clientMutationID)
            if nextPendingNotificationReplyCommand() == nil {
                resetNotificationReplyOutboxRetry()
            }
            connectionState = .connected
            errorMessage = nil
            lastUpdatedAt = Date()
            CompanionDiagnostics.record(
                "notification-reply:accepted sessionID=\(sessionID) notificationID=\(notificationID) kind=\(response.dispatchKind)"
            )
            if detailBySessionID[sessionID] != nil {
                await refreshSessionDetail(id: sessionID, assistantSurface: targetSurface)
            }
            return true
        } catch {
            connectionState = connectionState(for: error)
            clearConnectionRouteStateIfNeeded(for: connectionState)
            errorMessage = error.localizedDescription
            Haptics.error()
            CompanionDiagnostics.record(
                "notification-reply:send-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
            )
            scheduleNotificationReplyOutboxRetryIfNeeded()
            return false
        }
    }

    func drainPendingNotificationReplies() async {
        startNotificationReplyOutboxDrainIfNeeded()
        await notificationReplyOutboxDrainTask?.value
    }

    private func startNotificationReplyOutboxDrainIfNeeded() {
        guard notificationReplyOutboxDrainTask == nil,
              nextPendingNotificationReplyCommand() != nil
        else {
            return
        }

        let drainID = makeClientMutationID()
        cancelNotificationReplyOutboxRetry()
        let drainTask = Task { [weak self] in
            guard let self else {
                return
            }
            await self.drainNotificationReplyOutbox(drainID: drainID)
        }
        notificationReplyOutboxDrainTask = drainTask
        notificationReplyOutboxDrainID = drainID
    }

    private func drainNotificationReplyOutbox(drainID: String) async {
        while !Task.isCancelled {
            guard let command = nextPendingNotificationReplyCommand() else {
                break
            }
            let didSend = await submitPendingNotificationReplyCommand(command)
            if !didSend {
                break
            }
        }
        finishNotificationReplyOutboxDrain(drainID: drainID)
    }

    private func finishNotificationReplyOutboxDrain(drainID: String) {
        guard notificationReplyOutboxDrainID == drainID else {
            CompanionDiagnostics.record("notification-reply:stale-drain-finish-skip")
            return
        }

        notificationReplyOutboxDrainTask = nil
        notificationReplyOutboxDrainID = nil
        if nextPendingNotificationReplyCommand() == nil {
            resetNotificationReplyOutboxRetry()
        }
    }

    private func stopNotificationReplyOutboxDrain() {
        notificationReplyOutboxDrainTask?.cancel()
        notificationReplyOutboxDrainTask = nil
        notificationReplyOutboxDrainID = nil
        cancelNotificationReplyOutboxRetry()
    }

    private func nextPendingNotificationReplyCommand() -> CompanionSessionMiniPendingCommand? {
        sessionMiniLocalStore?.pendingCommands().first { command in
            command.kind == .submitNotificationReply
        }
    }

    @discardableResult
    private func submitPendingNotificationReplyCommand(
        _ command: CompanionSessionMiniPendingCommand
    ) async -> Bool {
        guard let notificationID = Self.nonEmptyText(command.notificationID),
              let prompt = Self.nonEmptyText(command.prompt),
              let sessionID = Self.nonEmptyText(command.threadID)
        else {
            CompanionDiagnostics.record(
                "notification-reply:drop-malformed-outbox-command id=\(command.clientMutationID)"
            )
            markLocalCommandDelivered(command.clientMutationID)
            return true
        }

        markLocalCommandAttempted(command.clientMutationID)
        let targetSurface = sessionIndex.assistantSurface(containingSessionID: sessionID)
            ?? selectedAssistantSurface
        return await sendNotificationReplyCommand(
            notificationID: notificationID,
            sessionID: sessionID,
            prompt: prompt,
            targetSurface: targetSurface,
            clientMutationID: command.clientMutationID
        )
    }

    private func scheduleNotificationReplyOutboxRetryIfNeeded() {
        guard notificationReplyOutboxRetryTask == nil,
              nextPendingNotificationReplyCommand() != nil
        else {
            return
        }

        let delayNanoseconds = notificationReplyOutboxRetryDelayNanoseconds
        notificationReplyOutboxRetryDelayNanoseconds = min(
            delayNanoseconds * NotificationReplyOutboxRetry.backoffMultiplier,
            NotificationReplyOutboxRetry.maximumDelayNanoseconds
        )
        CompanionDiagnostics.record(
            "notification-reply:retry-scheduled delayNanoseconds=\(delayNanoseconds)"
        )
        notificationReplyOutboxRetryTask = Task { [weak self] in
            do {
                try await Task.sleep(nanoseconds: delayNanoseconds)
            } catch {
                return
            }

            guard !Task.isCancelled else {
                return
            }

            self?.resumeNotificationReplyOutboxRetry()
        }
    }

    private func resumeNotificationReplyOutboxRetry() {
        notificationReplyOutboxRetryTask = nil
        startNotificationReplyOutboxDrainIfNeeded()
    }

    private func resetNotificationReplyOutboxRetry() {
        cancelNotificationReplyOutboxRetry()
        notificationReplyOutboxRetryDelayNanoseconds =
            NotificationReplyOutboxRetry.initialDelayNanoseconds
    }

    private func cancelNotificationReplyOutboxRetry() {
        notificationReplyOutboxRetryTask?.cancel()
        notificationReplyOutboxRetryTask = nil
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
        guard let sessionMiniLocalStore else {
            return false
        }

        do {
            guard let cachedSnapshot = try sessionMiniLocalStore.cachedSnapshot() else {
                return false
            }

            applyCachedSnapshot(cachedSnapshot, reason: "session-mini-\(reason)")
            CompanionDiagnostics.record(
                "session-mini:cache-restore reason=\(reason) sessions=\(cachedSnapshot.sessions.count)"
            )
            return true
        } catch {
            CompanionDiagnostics.record(
                "session-mini:cache-restore-failed reason=\(reason) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func makeClientMutationID() -> String {
        UUID().uuidString
    }

    private func enqueueLocalModeCommand(
        sessionID: String,
        preset: SessionMode?,
        clientMutationID: String
    ) {
        do {
            try sessionMiniLocalStore?.enqueueModeCommand(
                threadID: sessionID,
                preset: preset,
                clientMutationID: clientMutationID
            )
            markLocalCommandAttempted(clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:mode-outbox-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
        }
    }

    private func enqueueLocalPromptCommand(
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface,
        clientMutationID: String
    ) {
        do {
            try sessionMiniLocalStore?.enqueuePromptCommand(
                threadID: sessionID,
                prompt: prompt,
                assistantSurface: assistantSurface,
                clientMutationID: clientMutationID
            )
            markLocalCommandAttempted(clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:prompt-outbox-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
        }
    }

    private func enqueueLocalNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        clientMutationID: String
    ) {
        do {
            try sessionMiniLocalStore?.enqueueNotificationReplyCommand(
                notificationID: notificationID,
                threadID: sessionID,
                prompt: prompt,
                assistantSurface: nil,
                clientMutationID: clientMutationID
            )
            markLocalCommandAttempted(clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:notification-reply-outbox-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
            )
        }
    }

    private func markLocalCommandAttempted(_ clientMutationID: String) {
        do {
            try sessionMiniLocalStore?.markAttempted(clientMutationID: clientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:outbox-attempt-mark-failed id=\(clientMutationID) error=\(error.localizedDescription)"
            )
        }
    }

    private func markLocalCommandDelivered(_ clientMutationID: String?) {
        guard let trimmedClientMutationID = clientMutationID?
            .trimmingCharacters(in: .whitespacesAndNewlines),
            !trimmedClientMutationID.isEmpty
        else {
            return
        }

        do {
            try sessionMiniLocalStore?.markDelivered(clientMutationID: trimmedClientMutationID)
        } catch {
            CompanionDiagnostics.record(
                "session-mini:outbox-delivery-mark-failed id=\(trimmedClientMutationID) error=\(error.localizedDescription)"
            )
        }
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
    private static func normalizedRevision(_ revision: String?) -> String? {
        let trimmedRevision = revision?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedRevision, !trimmedRevision.isEmpty else {
            return nil
        }
        return trimmedRevision
    }
}

private actor SpotlightIndexSyncWorker {
    private var currentTask: Task<Void, Never>?

    deinit {
        currentTask?.cancel()
    }

    func clearSessions(indexer: SessionSpotlightIndexer) async {
        schedule {
            try await indexer.deleteAllSessions()
        }
    }

    func syncSessions(
        indexer: SessionSpotlightIndexer,
        rebuildsIndex: Bool,
        removedSearchableIDs: [String],
        changedSessions: [SessionSummary],
        indexableSessions: [SessionSummary]
    ) async {
        schedule {
            if rebuildsIndex {
                try await indexer.deleteAllSessions()
            } else if !removedSearchableIDs.isEmpty {
                try await indexer.deleteSessions(withIDs: removedSearchableIDs)
            }
            try Task.checkCancellation()

            let sessionsToIndex = rebuildsIndex ? indexableSessions : changedSessions
            if !sessionsToIndex.isEmpty {
                try await indexer.indexSessions(sessionsToIndex)
            }
        }
    }

    private func schedule(_ operation: @escaping @Sendable () async throws -> Void) {
        currentTask?.cancel()
        currentTask = Task.detached(priority: .utility) {
            do {
                try Task.checkCancellation()
                try await operation()
                try Task.checkCancellation()
            } catch is CancellationError {
            } catch {
                print("Failed to update Spotlight sessions: \(error)")
            }
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

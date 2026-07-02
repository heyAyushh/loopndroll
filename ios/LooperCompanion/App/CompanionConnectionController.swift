import Foundation
import LooperCompanionCore
import Network

@MainActor
protocol CompanionConnectionControllerDelegate: AnyObject {
    var connectionControllerSnapshotState: CompanionSnapshotStateStore { get }
    var connectionControllerRealtimeServerTime: String? { get set }
    var connectionControllerRealtimeLatestSeq: Int64 { get set }
    var connectionControllerRealtimeStreamIsLive: Bool { get set }
    var connectionControllerLastRealtimeDataAt: Date? { get set }
    var connectionControllerRealtimeReconnectInProgress: Bool { get set }
    var connectionControllerActiveSessionRouteBaseURL: URL? { get set }
    var connectionControllerConnectionState: ConnectivityState { get set }
    var connectionControllerErrorMessage: String? { get set }
    var connectionControllerIsAwaitingRouteSessionProof: Bool { get }

    func connectionControllerSetAwaitingRouteSessionProof(_ isAwaiting: Bool)
    func connectionControllerInvalidatePendingSessionMiniProjectionBuilds()
    func connectionControllerApplySessionMiniSyncUpdate(
        _ update: CompanionSessionMiniSyncUpdate,
        connectionRevision: Int
    )
    func connectionControllerRefreshPendingPromptDeliveryState()
    func connectionControllerSetLastUpdatedAt(_ date: Date)
}

@MainActor
final class CompanionConnectionController {
    private let sessionMiniController: CompanionSessionMiniController
    private weak var delegate: CompanionConnectionControllerDelegate?
    private var networkPathMonitor: NWPathMonitor?
    private var lastNetworkPathIdentity: String?

    private(set) var connectionRevision = 0

    init(
        sessionMiniController: CompanionSessionMiniController,
        delegate: CompanionConnectionControllerDelegate
    ) {
        self.sessionMiniController = sessionMiniController
        self.delegate = delegate
    }

    func advanceConnectionRevision() {
        connectionRevision += 1
    }

    func startNetworkPathMonitoringIfNeeded() {
        guard networkPathMonitor == nil else {
            return
        }

        let monitor = NWPathMonitor()
        networkPathMonitor = monitor
        monitor.pathUpdateHandler = { [weak self] path in
            let identity = Self.networkPathIdentity(path)
            Task { @MainActor [weak self] in
                self?.handleNetworkPathChange(identity: identity)
            }
        }
        monitor.start(queue: DispatchQueue(label: "companion.network-path-monitor"))
    }

    func stopSessionRuntimeSync() {
        sessionMiniController.stopSync()
        delegate?.connectionControllerInvalidatePendingSessionMiniProjectionBuilds()
        markSessionStreamStopped(reconnectInProgress: false)
    }

    func stopSessionRuntimeSyncForRestart() {
        sessionMiniController.stopSync()
        delegate?.connectionControllerInvalidatePendingSessionMiniProjectionBuilds()
        markSessionStreamStopped(reconnectInProgress: true)
    }

    func startSessionRuntimeSyncIfNeeded() {
        guard let delegate else {
            return
        }

        if !delegate.connectionControllerRealtimeStreamIsLive {
            delegate.connectionControllerRealtimeReconnectInProgress = true
        }
        sessionMiniController.startSyncIfNeeded(
            connectionRevision: connectionRevision
        ) { [weak self] update, connectionRevision in
            self?.delegate?.connectionControllerApplySessionMiniSyncUpdate(
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

    func restartSessionRuntimeSyncForRouteChange() {
        if sessionMiniController.isSyncing {
            stopSessionRuntimeSyncForRestart()
        }
        startSessionRuntimeSyncIfNeeded()
    }

    func resetRealtimeState() {
        guard let delegate else {
            return
        }

        delegate.connectionControllerActiveSessionRouteBaseURL = nil
        delegate.connectionControllerRealtimeServerTime = nil
        delegate.connectionControllerRealtimeLatestSeq = 0
        delegate.connectionControllerRealtimeStreamIsLive = false
        delegate.connectionControllerLastRealtimeDataAt = nil
        delegate.connectionControllerRealtimeReconnectInProgress = false
    }

    @discardableResult
    func applyRealtimeStreamLiveness(
        serverTime: String,
        latestSeq: Int64,
        isLive: Bool,
        endpointURL: URL?,
        recordedAt: Date = Date()
    ) -> Bool {
        guard let delegate else {
            return false
        }

        guard latestSeq >= delegate.connectionControllerRealtimeLatestSeq || Self.isStreamRestartLiveness(
            latestSeq: latestSeq,
            isLive: isLive
        ) else {
            CompanionDiagnostics.record(
                "session-mini:liveness-stale-skip latestSeq=\(latestSeq) realtimeSeq=\(delegate.connectionControllerRealtimeLatestSeq)"
            )
            return false
        }

        var didChange = false
        if isLive {
            delegate.connectionControllerLastRealtimeDataAt = recordedAt
        }
        if !serverTime.isEmpty {
            if delegate.connectionControllerRealtimeServerTime != serverTime {
                delegate.connectionControllerRealtimeServerTime = serverTime
                didChange = true
            }
            didChange = delegate.connectionControllerSnapshotState.applyHostSyncTime(serverTime) || didChange
        }
        let nextLatestSeq = max(delegate.connectionControllerRealtimeLatestSeq, latestSeq)
        if delegate.connectionControllerRealtimeLatestSeq != nextLatestSeq {
            delegate.connectionControllerRealtimeLatestSeq = nextLatestSeq
            didChange = true
        }
        if delegate.connectionControllerRealtimeStreamIsLive != isLive {
            delegate.connectionControllerRealtimeStreamIsLive = isLive
            didChange = true
        }
        let nextReconnectInProgress = isLive ? false : sessionMiniController.isSyncing
        if delegate.connectionControllerRealtimeReconnectInProgress != nextReconnectInProgress {
            delegate.connectionControllerRealtimeReconnectInProgress = nextReconnectInProgress
            didChange = true
        }
        let nextRouteBaseURL = isLive ? endpointURL : nil
        if delegate.connectionControllerActiveSessionRouteBaseURL != nextRouteBaseURL {
            delegate.connectionControllerActiveSessionRouteBaseURL = nextRouteBaseURL
            didChange = true
        }
        if isLive, delegate.connectionControllerIsAwaitingRouteSessionProof {
            delegate.connectionControllerSetAwaitingRouteSessionProof(false)
            didChange = true
        }
        let nextConnectionState = Self.connectionStateForSessionLiveness(
            isLive: isLive,
            currentState: delegate.connectionControllerConnectionState
        )
        if delegate.connectionControllerConnectionState != nextConnectionState {
            delegate.connectionControllerConnectionState = nextConnectionState
            didChange = true
        }
        if isLive, delegate.connectionControllerErrorMessage != nil {
            delegate.connectionControllerErrorMessage = nil
            didChange = true
        }
        return didChange
    }

    func sessionAuthoritativeConnectionState(
        _ projectedState: ConnectivityState
    ) -> ConnectivityState {
        guard let delegate else {
            return projectedState
        }

        if delegate.connectionControllerIsAwaitingRouteSessionProof {
            switch projectedState {
            case .unauthorized, .locked, .unpaired, .offline:
                delegate.connectionControllerSetAwaitingRouteSessionProof(false)
                return projectedState
            case .connecting, .connected:
                return .connecting
            }
        }

        if delegate.connectionControllerRealtimeStreamIsLive {
            return .connected
        }

        if projectedState == .connected, !delegate.connectionControllerRealtimeStreamIsLive {
            return .connecting
        }

        return projectedState
    }

    func sessionAuthoritativeErrorMessage(
        shouldSuppressProjectionError: Bool,
        error: Error
    ) -> String? {
        guard let delegate else {
            return error.localizedDescription
        }

        if delegate.connectionControllerRealtimeStreamIsLive || shouldSuppressProjectionError {
            return nil
        }

        return error.localizedDescription
    }

    private func handleNetworkPathChange(identity: String) {
        guard let previousIdentity = lastNetworkPathIdentity else {
            lastNetworkPathIdentity = identity
            return
        }
        guard identity != previousIdentity else {
            return
        }

        lastNetworkPathIdentity = identity
        CompanionDiagnostics.record("network:path-changed identity=\(identity)")
        stopSessionRuntimeSyncForRestart()
        startSessionRuntimeSyncIfNeeded()
    }

    private func applySessionMiniLivenessUpdate(
        _ update: CompanionSessionMiniLivenessUpdate,
        connectionRevision: Int
    ) {
        guard connectionRevision == self.connectionRevision else {
            CompanionDiagnostics.record("session-mini:liveness-stale-skip")
            return
        }
        delegate?.connectionControllerRefreshPendingPromptDeliveryState()

        let didChange = applyRealtimeStreamLiveness(
            serverTime: update.serverTime,
            latestSeq: update.latestSeq,
            isLive: update.isLive,
            endpointURL: update.endpointURL
        )
        guard didChange else {
            return
        }
        delegate?.connectionControllerSetLastUpdatedAt(Date())
        CompanionDiagnostics.record(
            "session-mini:liveness-applied reason=\(update.reason) seq=\(update.latestSeq)"
        )
    }

    private func markSessionStreamStopped(reconnectInProgress: Bool) {
        guard let delegate else {
            return
        }

        delegate.connectionControllerRealtimeStreamIsLive = false
        delegate.connectionControllerActiveSessionRouteBaseURL = nil
        delegate.connectionControllerRealtimeReconnectInProgress = reconnectInProgress
        if delegate.connectionControllerConnectionState == .connected {
            delegate.connectionControllerConnectionState = .connecting
        }
    }

    private nonisolated static func networkPathIdentity(_ path: NWPath) -> String {
        let interfaces = path.availableInterfaces
            .map { "\($0.type)" }
            .sorted()
            .joined(separator: ",")
        return "\(path.status):\(interfaces)"
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
}

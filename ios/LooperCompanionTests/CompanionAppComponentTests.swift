import Foundation
import Testing
@testable import Looper

@Suite("CompanionAppComponentTests")
struct CompanionAppComponentTests {
    private enum Constants {
        static let serverTime = "2026-06-24T00:00:15Z"
        static let routeURL = "http://192.168.2.10:8766"
        static let recordedAt = Date(timeIntervalSinceReferenceDate: 1_000)
    }

    @MainActor
    @Test
    func connectionControllerAppliesFreshnessAndRestartTransitions() throws {
        let harness = ConnectionControllerHarness()
        let controller = CompanionConnectionController(
            sessionMiniController: harness.sessionMiniController,
            delegate: harness
        )
        let route = try #require(URL(string: Constants.routeURL))

        #expect(controller.applyRealtimeStreamLiveness(
            serverTime: Constants.serverTime,
            latestSeq: 10,
            isLive: true,
            endpointURL: route,
            recordedAt: Constants.recordedAt
        ))
        #expect(harness.realtimeServerTime == Constants.serverTime)
        #expect(harness.realtimeLatestSeq == 10)
        #expect(harness.realtimeStreamIsLive)
        #expect(harness.lastRealtimeDataAt == Constants.recordedAt)
        #expect(!harness.realtimeReconnectInProgress)
        #expect(harness.activeSessionRouteBaseURL == route)
        #expect(harness.connectionState == .connected)

        #expect(!controller.applyRealtimeStreamLiveness(
            serverTime: Constants.serverTime,
            latestSeq: 9,
            isLive: true,
            endpointURL: route,
            recordedAt: Constants.recordedAt.addingTimeInterval(1)
        ))
        #expect(harness.realtimeLatestSeq == 10)
        #expect(harness.lastRealtimeDataAt == Constants.recordedAt)

        controller.stopSessionRuntimeSyncForRestart()
        #expect(!harness.realtimeStreamIsLive)
        #expect(harness.realtimeReconnectInProgress)
        #expect(harness.activeSessionRouteBaseURL == nil)
        #expect(harness.connectionState == .connecting)

        let restart = CompanionSessionMiniController.restartLivenessUpdate()
        #expect(controller.applyRealtimeStreamLiveness(
            serverTime: restart.serverTime,
            latestSeq: restart.latestSeq,
            isLive: restart.isLive,
            endpointURL: restart.endpointURL
        ))
        #expect(!harness.realtimeStreamIsLive)
        #expect(!harness.realtimeReconnectInProgress)
        #expect(harness.connectionState == .connecting)
    }

    @MainActor
    @Test
    func commandDispatcherMapsConnectionErrors() {
        let cases: [(Error, ConnectivityState)] = [
            (CompanionConfigurationError.invalidConnectionCode, .unpaired),
            (HTTPCompanionServiceError.unauthorized, .unauthorized),
            (HTTPCompanionServiceError.passkeySessionRequired("Unlock looper."), .locked),
            (HTTPCompanionServiceError.invalidResponse, .offline),
            (HTTPCompanionServiceError.localStoreUnavailable, .offline),
            (HTTPCompanionServiceError.serverError("No route."), .offline),
            (NSError(domain: "CompanionAppComponentTests", code: 1), .offline),
        ]

        for (error, expectedState) in cases {
            #expect(CompanionCommandDispatcher.connectionState(for: error) == expectedState)
        }
    }
}

@MainActor
private final class ConnectionControllerHarness: CompanionConnectionControllerDelegate {
    let sessionMiniController = CompanionSessionMiniController(sessionRuntime: nil)
    let snapshotState = CompanionSnapshotStateStore()

    var realtimeServerTime: String?
    var realtimeLatestSeq: Int64 = 0
    var realtimeStreamIsLive = false
    var lastRealtimeDataAt: Date?
    var realtimeReconnectInProgress = false
    var activeSessionRouteBaseURL: URL?
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
    var isAwaitingRouteSessionProof = false
    var invalidatedProjectionBuilds = false
    var refreshedPendingPromptDelivery = false
    var lastUpdatedAt: Date?

    var connectionControllerSnapshotState: CompanionSnapshotStateStore {
        snapshotState
    }

    var connectionControllerRealtimeServerTime: String? {
        get { realtimeServerTime }
        set { realtimeServerTime = newValue }
    }

    var connectionControllerRealtimeLatestSeq: Int64 {
        get { realtimeLatestSeq }
        set { realtimeLatestSeq = newValue }
    }

    var connectionControllerRealtimeStreamIsLive: Bool {
        get { realtimeStreamIsLive }
        set { realtimeStreamIsLive = newValue }
    }

    var connectionControllerLastRealtimeDataAt: Date? {
        get { lastRealtimeDataAt }
        set { lastRealtimeDataAt = newValue }
    }

    var connectionControllerRealtimeReconnectInProgress: Bool {
        get { realtimeReconnectInProgress }
        set { realtimeReconnectInProgress = newValue }
    }

    var connectionControllerActiveSessionRouteBaseURL: URL? {
        get { activeSessionRouteBaseURL }
        set { activeSessionRouteBaseURL = newValue }
    }

    var connectionControllerConnectionState: ConnectivityState {
        get { connectionState }
        set { connectionState = newValue }
    }

    var connectionControllerErrorMessage: String? {
        get { errorMessage }
        set { errorMessage = newValue }
    }

    var connectionControllerIsAwaitingRouteSessionProof: Bool {
        isAwaitingRouteSessionProof
    }

    func connectionControllerSetAwaitingRouteSessionProof(_ isAwaiting: Bool) {
        isAwaitingRouteSessionProof = isAwaiting
    }

    func connectionControllerInvalidatePendingSessionMiniProjectionBuilds() {
        invalidatedProjectionBuilds = true
    }

    func connectionControllerApplySessionMiniSyncUpdate(
        _: CompanionSessionMiniSyncUpdate,
        connectionRevision _: Int
    ) {}

    func connectionControllerRefreshPendingPromptDeliveryState() {
        refreshedPendingPromptDelivery = true
    }

    func connectionControllerSetLastUpdatedAt(_ date: Date) {
        lastUpdatedAt = date
    }
}

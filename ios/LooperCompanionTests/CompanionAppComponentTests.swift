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
    func modelAppliesFreshnessAndRestartTransitions() throws {
        let storeDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("looper-component-tests", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: storeDirectory, withIntermediateDirectories: true)
        let runtime = try CompanionSessionRuntime(
            fileURL: storeDirectory.appendingPathComponent(CompanionSessionRuntime.defaultFileName)
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: MockCompanionService()),
            sessionRuntime: runtime
        )
        let route = try #require(URL(string: Constants.routeURL))

        #expect(model.applyRealtimeStreamLiveness(
            serverTime: Constants.serverTime,
            latestSeq: 10,
            isLive: true,
            endpointURL: route,
            recordedAt: Constants.recordedAt
        ))
        #expect(model.realtimeServerTime == Constants.serverTime)
        #expect(model.realtimeLatestSeq == 10)
        #expect(model.realtimeStreamIsLive)
        #expect(model.lastRealtimeDataAt == Constants.recordedAt)
        #expect(!model.realtimeReconnectInProgress)
        #expect(model.activeSessionRouteBaseURL == route)
        #expect(model.connectionState == .connected)

        #expect(!model.applyRealtimeStreamLiveness(
            serverTime: Constants.serverTime,
            latestSeq: 9,
            isLive: true,
            endpointURL: route,
            recordedAt: Constants.recordedAt.addingTimeInterval(1)
        ))
        #expect(model.realtimeLatestSeq == 10)
        #expect(model.lastRealtimeDataAt == Constants.recordedAt)

        model.stopSessionRuntimeSync()
        #expect(!model.realtimeStreamIsLive)
        #expect(model.activeSessionRouteBaseURL == nil)
        #expect(model.connectionState == .connecting)

        // Restart-shaped liveness (seq 0, not live) passes the stale-seq
        // guard; with no state delta it reports no change.
        #expect(!model.applyRealtimeStreamLiveness(
            serverTime: "",
            latestSeq: 0,
            isLive: false,
            endpointURL: nil
        ))
        #expect(!model.realtimeStreamIsLive)
        #expect(model.connectionState == .connecting)
    }

    @MainActor
    @Test
    func foregroundReactivationClampsStaleFreshnessToLive() throws {
        let model = try Self.makeModel()
        let now = Date(timeIntervalSinceReferenceDate: 10_000)

        // Data last arrived long before the background/offline windows —
        // naively this would read as a dead stream — but the app just came
        // back to the foreground, so the clock should restart at wake.
        model.lastRealtimeDataAt = now.addingTimeInterval(-200)
        model.connection.setLastBecameActiveAtForTesting(now.addingTimeInterval(-2))

        let status = model.viewState.connectionStatusPresentation(now: now).status
        #expect(status == .live)
    }

    @MainActor
    @Test
    func staleActivationStillDegradesToOffline() throws {
        let model = try Self.makeModel()
        let now = Date(timeIntervalSinceReferenceDate: 10_000)

        // No recent activation recorded (matches a session that never left
        // the foreground) — behavior must match the pre-clamp legacy path.
        model.lastRealtimeDataAt = now.addingTimeInterval(-200)

        let status = model.viewState.connectionStatusPresentation(now: now).status
        #expect(status == .offline)
    }

    @MainActor
    @Test
    func longForegroundOutageIsNotMaskedByActivation() throws {
        let model = try Self.makeModel()
        let now = Date(timeIntervalSinceReferenceDate: 10_000)

        // The app has been continuously active (no background gap) for
        // longer than the offline window, and no data has ever arrived
        // since. Activation must not grant an unbounded grace period.
        model.lastRealtimeDataAt = now.addingTimeInterval(-300)
        model.connection.setLastBecameActiveAtForTesting(now.addingTimeInterval(-50))

        let status = model.viewState.connectionStatusPresentation(now: now).status
        #expect(status == .offline)
    }

    @MainActor
    private static func makeModel() throws -> CompanionAppModel {
        let storeDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent("looper-component-tests", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: storeDirectory, withIntermediateDirectories: true)
        let runtime = try CompanionSessionRuntime(
            fileURL: storeDirectory.appendingPathComponent(CompanionSessionRuntime.defaultFileName)
        )
        return CompanionAppModel(
            environment: CompanionEnvironment(service: MockCompanionService()),
            sessionRuntime: runtime
        )
    }

    @MainActor
    @Test
    func provenTransportMigrationSkipsPathChangeRestart() async throws {
        let model = try Self.makeModel()
        let connection = model.connection
        connection.rebindTransportForTesting = { true }
        connection.migrationGraceWindowForTesting = .milliseconds(50)
        connection.startSyncIfNeeded()

        connection.simulateNetworkPathChangeForTesting()
        connection.setLastLiveActivityAtForTesting(Date())
        await connection.waitForMigrationGraceForTesting()

        #expect(connection.routeRestartCountForTesting == 0)
    }

    @MainActor
    @Test
    func unprovenTransportMigrationFallsBackToRestart() async throws {
        let model = try Self.makeModel()
        let connection = model.connection
        connection.rebindTransportForTesting = { true }
        connection.migrationGraceWindowForTesting = .milliseconds(50)
        connection.startSyncIfNeeded()

        connection.simulateNetworkPathChangeForTesting()
        connection.setLastLiveActivityAtForTesting(nil)
        await connection.waitForMigrationGraceForTesting()

        #expect(connection.routeRestartCountForTesting == 1)
    }

    @MainActor
    @Test
    func failedRebindRestartsImmediately() throws {
        let model = try Self.makeModel()
        let connection = model.connection
        connection.rebindTransportForTesting = { false }
        connection.startSyncIfNeeded()

        connection.simulateNetworkPathChangeForTesting()

        #expect(connection.routeRestartCountForTesting == 1)
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

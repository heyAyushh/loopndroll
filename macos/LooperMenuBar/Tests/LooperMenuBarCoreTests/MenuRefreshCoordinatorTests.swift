import Foundation
import Testing
@testable import LooperMenuBarCore

@Suite("Menu refresh coordinator")
struct MenuRefreshCoordinatorTests {
    @Test("concurrent refreshes share the in-flight request")
    func concurrentRefreshesShareInFlightRequest() async {
        let client = MenuRefreshRecordingClient(snapshotDelay: .milliseconds(100))
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .seconds(5))

        async let first = coordinator.refresh()
        async let second = coordinator.refresh()
        let results = await [first, second]

        #expect(results.map(\.succeeded) == [true, true])
        #expect(client.snapshotCalls == 1)
        #expect(client.connectionCalls == 1)
        #expect(client.acpHostCalls == 1)
        #expect(client.mobileStateCalls == 1)
        #expect(client.pushDeviceCalls == 1)
        #expect(client.healthCalls == 1)
    }

    @Test("fresh successful refresh is reused until force refresh")
    func freshResultIsReusedUntilForceRefresh() async {
        let client = MenuRefreshRecordingClient()
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .seconds(5))

        _ = await coordinator.refresh()
        _ = await coordinator.refresh()

        #expect(client.snapshotCalls == 1)
        #expect(client.connectionCalls == 1)
        #expect(client.acpHostCalls == 1)
        #expect(client.mobileStateCalls == 1)
        #expect(client.pushDeviceCalls == 1)
        #expect(client.healthCalls == 1)

        _ = await coordinator.refresh(force: true)

        #expect(client.snapshotCalls == 2)
        #expect(client.connectionCalls == 2)
        #expect(client.acpHostCalls == 2)
        #expect(client.mobileStateCalls == 2)
        #expect(client.pushDeviceCalls == 2)
        #expect(client.healthCalls == 2)
    }

    @Test("optional refresh details fetch concurrently after snapshot succeeds")
    func optionalRefreshDetailsFetchConcurrentlyAfterSnapshotSucceeds() async {
        let client = MenuRefreshRecordingClient(
            connectionDelay: .milliseconds(200),
            acpHostDelay: .milliseconds(200),
            healthDelay: .milliseconds(200)
        )
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .zero)
        let clock = ContinuousClock()
        let start = clock.now

        let result = await coordinator.refresh(force: true)

        #expect(result.succeeded)
        #expect(start.duration(to: clock.now) < .milliseconds(350))
        #expect(client.snapshotCalls == 1)
        #expect(client.connectionCalls == 1)
        #expect(client.acpHostCalls == 1)
        #expect(client.mobileStateCalls == 1)
        #expect(client.pushDeviceCalls == 1)
        #expect(client.healthCalls == 1)
    }

    @Test("mobile health failure keeps successful snapshot")
    func mobileHealthFailureKeepsSuccessfulSnapshot() async {
        let client = MenuRefreshRecordingClient(
            healthResult: .failure(ControlPlaneClientError.invalidResponse)
        )
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .zero)

        let result = await coordinator.refresh(force: true)

        #expect(result.succeeded)
        #expect(result.snapshot != nil)
        #expect(result.mobileHealth == nil)
        #expect(result.error == nil)
    }

    @Test("ACP host failure keeps successful snapshot")
    func acpHostFailureKeepsSuccessfulSnapshot() async {
        let client = MenuRefreshRecordingClient(
            acpHostResult: .failure(ControlPlaneClientError.invalidResponse)
        )
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .zero)

        let result = await coordinator.refresh(force: true)

        #expect(result.succeeded)
        #expect(result.snapshot != nil)
        #expect(result.acpClientHosts == nil)
        #expect(result.error == nil)
    }

    @Test("connection failure keeps successful snapshot")
    func connectionFailureKeepsSuccessfulSnapshot() async {
        let client = MenuRefreshRecordingClient(
            connectionResult: .failure(ControlPlaneClientError.invalidResponse)
        )
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .zero)

        let result = await coordinator.refresh(force: true)

        #expect(result.succeeded)
        #expect(result.snapshot != nil)
        #expect(result.connections == nil)
        #expect(result.acpClientHosts != nil)
        #expect(result.error == nil)
    }

    @Test("snapshot failure keeps enrichment fetches independent")
    func snapshotFailureKeepsEnrichmentFetchesIndependent() async {
        let client = MenuRefreshRecordingClient(
            snapshotResult: .failure(ControlPlaneClientError.timeout)
        )
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .zero)

        let result = await coordinator.refresh(force: true)

        #expect(!result.succeeded)
        #expect(result.snapshot == nil)
        #expect(result.mobileHealth != nil)
        #expect(result.mobileState != nil)
        #expect(result.pushDevices != nil)
        #expect(result.connections != nil)
        #expect(result.acpClientHosts != nil)
        #expect(result.error?.message.contains("timeout") == true)
        #expect(client.connectionCalls == 1)
        #expect(client.acpHostCalls == 1)
        #expect(client.mobileStateCalls == 1)
        #expect(client.pushDeviceCalls == 1)
        #expect(client.healthCalls == 1)
    }

    @Test("failed refresh replaces prior success cache during reuse window")
    func failedRefreshReplacesPriorSuccessCacheDuringReuseWindow() async {
        let client = MenuRefreshRecordingClient()
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .seconds(5))

        let success = await coordinator.refresh(force: true)
        client.snapshotResultOverride = .failure(ControlPlaneClientError.timeout)
        let failure = await coordinator.refresh(force: true)
        let cachedFailure = await coordinator.refresh()

        #expect(success.snapshot?.threadCount == 1)
        #expect(!failure.succeeded)
        #expect(!cachedFailure.succeeded)
        #expect(client.snapshotCalls == 2)
    }

    @Test("force refresh waits for stale normal in-flight request then fetches fresh")
    func forceRefreshWaitsForStaleNormalInFlightRequestThenFetchesFresh() async {
        let client = MenuRefreshRecordingClient(snapshotDelay: .milliseconds(100))
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .seconds(5))

        async let normal = coordinator.refresh()
        try? await Task.sleep(for: .milliseconds(10))
        let forced = await coordinator.refresh(force: true)
        let normalResult = await normal

        #expect(normalResult.snapshot?.threadCount == 1)
        #expect(forced.snapshot?.threadCount == 2)
        #expect(client.snapshotCalls == 2)
    }

    @Test("concurrent force refreshes share one forced request")
    func concurrentForceRefreshesShareOneForcedRequest() async {
        let client = MenuRefreshRecordingClient(snapshotDelay: .milliseconds(100))
        let coordinator = MenuRefreshCoordinator(client: client, freshReuseDuration: .seconds(5))

        async let first = coordinator.refresh(force: true)
        async let second = coordinator.refresh(force: true)
        let results = await [first, second]

        #expect(results.map { $0.snapshot?.threadCount } == [1, 1])
        #expect(client.snapshotCalls == 1)
    }
}

private final class MenuRefreshRecordingClient: ControlPlaneClient, @unchecked Sendable {
    private let lock = NSLock()
    private let snapshotDelay: Duration
    private let connectionDelay: Duration
    private let acpHostDelay: Duration
    private let healthDelay: Duration
    private let connectionResult: Result<DesktopConnectionsResponse, Error>
    private let acpHostResult: Result<AcpClientHostsResponse, Error>
    private let healthResult: Result<MobileHealthResponse, Error>
    private var recordedSnapshotCalls = 0
    private var recordedConnectionCalls = 0
    private var recordedAcpHostCalls = 0
    private var recordedMobileStateCalls = 0
    private var recordedPushDeviceCalls = 0
    private var recordedHealthCalls = 0
    private var recordedSnapshotResultOverride: Result<DesktopSnapshotResponse, Error>?

    init(
        snapshotDelay: Duration = .zero,
        connectionDelay: Duration = .zero,
        acpHostDelay: Duration = .zero,
        healthDelay: Duration = .zero,
        snapshotResult: Result<DesktopSnapshotResponse, Error>? = nil,
        connectionResult: Result<DesktopConnectionsResponse, Error>? = nil,
        acpHostResult: Result<AcpClientHostsResponse, Error>? = nil,
        healthResult: Result<MobileHealthResponse, Error>? = nil
    ) {
        self.snapshotDelay = snapshotDelay
        self.connectionDelay = connectionDelay
        self.acpHostDelay = acpHostDelay
        self.healthDelay = healthDelay
        self.connectionResult = connectionResult ?? .success(Self.desktopConnections())
        self.acpHostResult = acpHostResult ?? .success(Self.acpClientHosts())
        self.healthResult = healthResult ?? .success(Self.mobileHealth())
        self.recordedSnapshotResultOverride = snapshotResult
    }

    var snapshotCalls: Int {
        lock.withLock { recordedSnapshotCalls }
    }

    var connectionCalls: Int {
        lock.withLock { recordedConnectionCalls }
    }

    var acpHostCalls: Int {
        lock.withLock { recordedAcpHostCalls }
    }

    var mobileStateCalls: Int {
        lock.withLock { recordedMobileStateCalls }
    }

    var pushDeviceCalls: Int {
        lock.withLock { recordedPushDeviceCalls }
    }

    var healthCalls: Int {
        lock.withLock { recordedHealthCalls }
    }

    var snapshotResultOverride: Result<DesktopSnapshotResponse, Error>? {
        get {
            lock.withLock { recordedSnapshotResultOverride }
        }
        set {
            lock.withLock {
                recordedSnapshotResultOverride = newValue
            }
        }
    }

    func registerHooks() async throws {}
    func unregisterLiveHooks(timeout: TimeInterval) throws {}
    func shutdownServer(timeout: TimeInterval) throws {}

    func fetchControlPlaneStatus() async throws -> ControlPlaneStatusResponse {
        Self.controlPlaneStatus()
    }

    func fetchDesktopSnapshot() async throws -> DesktopSnapshotResponse {
        let callNumber = lock.withLock {
            recordedSnapshotCalls += 1
            return recordedSnapshotCalls
        }
        try? await Task.sleep(for: snapshotDelay)
        if let snapshotResultOverride {
            return try snapshotResultOverride.get()
        }
        return Self.snapshot(threadCount: callNumber)
    }

    func fetchDesktopConnections() async throws -> DesktopConnectionsResponse {
        lock.withLock {
            recordedConnectionCalls += 1
        }
        try? await Task.sleep(for: connectionDelay)
        return try connectionResult.get()
    }

    func fetchAcpClientHosts() async throws -> AcpClientHostsResponse {
        lock.withLock {
            recordedAcpHostCalls += 1
        }
        try? await Task.sleep(for: acpHostDelay)
        return try acpHostResult.get()
    }

    func fetchDesktopMobileState() async throws -> DesktopMobileStateResponse {
        lock.withLock {
            recordedMobileStateCalls += 1
        }
        return DesktopMobileStateResponse()
    }

    func fetchDesktopPushDevices() async throws -> DesktopPushDevicesResponse {
        lock.withLock {
            recordedPushDeviceCalls += 1
        }
        return DesktopPushDevicesResponse()
    }

    func setDefaultNotificationTargets(_ targetIDs: [String]) async throws -> DesktopMobileStateResponse {
        DesktopMobileStateResponse(defaultNotificationTargetIDs: targetIDs)
    }

    func fetchMobileHealth() async throws -> MobileHealthResponse {
        lock.withLock {
            recordedHealthCalls += 1
        }
        try? await Task.sleep(for: healthDelay)
        return try healthResult.get()
    }

    func probeDevinAcpBridge(agentId: String?) async throws -> DevinAcpBridgeProbeResponse {
        throw ControlPlaneClientError.invalidResponse
    }

    static func snapshot(threadCount: Int = 0) -> DesktopSnapshotResponse {
        DesktopSnapshotResponse(
            controlPlane: controlPlaneStatus(),
            devinDesktop: DevinDesktopStatus(acpBridge: devinBridge()),
            threadCount: threadCount,
            activeThreadCount: 0,
            archivedThreadCount: 0,
            threads: [],
            automations: [],
            goals: [],
            compactions: [],
            assistantAdapters: []
        )
    }

    static func desktopConnections() -> DesktopConnectionsResponse {
        DesktopConnectionsResponse(
            connections: [
                DesktopConnectionSummary(
                    id: "codex-hooks",
                    kind: "codex",
                    label: "Codex hooks",
                    status: "healthy",
                    subtitle: nil,
                    detail: "managed hooks"
                )
            ]
        )
    }

    static func acpClientHosts() -> AcpClientHostsResponse {
        AcpClientHostsResponse(
            hosts: [
                AcpClientHost(
                    id: "zed",
                    label: "Zed",
                    running: true,
                    installed: true,
                    registry: AcpClientHostRegistry(
                        path: "/Users/test/.zed/settings.json",
                        exists: true,
                        version: nil,
                        agentCount: 1
                    ),
                    agents: [],
                    sessions: [],
                    actions: [],
                    limitations: [
                        "Read-only: Zed manages External Agent install, auth, and runtime inside Zed.",
                    ],
                    runtime: nil
                ),
            ]
        )
    }

    static func mobileHealth() -> MobileHealthResponse {
        MobileHealthResponse(
            ok: true,
            baseURL: "http://127.0.0.1:8765",
            baseURLs: ["http://192.168.1.4:8765", "http://127.0.0.1:8765"],
            requiresAuthentication: true
        )
    }

    static func controlPlaneStatus() -> ControlPlaneStatusResponse {
        ControlPlaneStatusResponse(
            hooks: HookStatusSummary(
                enabled: true,
                registeredEvents: [],
                activeCommand: nil,
                owner: "looper-rust",
                health: "healthy",
                issues: [],
                recentFailuresCount: 0
            ),
            codexServers: [],
            source: SourceStatusSummary(
                codexHome: "/tmp/codex",
                stateDb: nil,
                logsDb: nil,
                sessionsRoot: "/tmp/codex/sessions",
                health: "healthy",
                degradedReason: nil
            )
        )
    }

    static func devinBridge() -> DevinAcpBridgeStatus {
        DevinAcpBridgeStatus(
            available: false,
            controlLevel: "unavailable",
            summary: "Unavailable",
            actions: [],
            agents: []
        )
    }
}

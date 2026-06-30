import Foundation

public struct MenuRefreshError: Error, Equatable, Sendable {
    public let message: String

    public init(_ error: Error) {
        self.message = String(describing: error)
    }
}

public struct MenuRefreshResult: Equatable, Sendable {
    public let didFetchHTTP: Bool
    public let sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?
    public let snapshot: DesktopSnapshotResponse?
    public let connections: DesktopConnectionsResponse?
    public let acpClientHosts: AcpClientHostsResponse?
    public let mobileState: DesktopMobileStateResponse?
    public let pushDevices: DesktopPushDevicesResponse?
    public let mobileHealth: MobileHealthResponse?
    public let error: MenuRefreshError?

    public var succeeded: Bool {
        sessionMiniSnapshot != nil || snapshot != nil
    }

    public var hasReusableEnrichment: Bool {
        snapshot != nil
            || connections != nil
            || acpClientHosts != nil
            || mobileState != nil
            || pushDevices != nil
    }

    public func mergingReusableEnrichment(from cached: MenuRefreshResult?) -> MenuRefreshResult {
        guard let cached else {
            return self
        }
        return MenuRefreshResult(
            didFetchHTTP: didFetchHTTP || cached.didFetchHTTP,
            sessionMiniSnapshot: sessionMiniSnapshot ?? cached.sessionMiniSnapshot,
            snapshot: snapshot ?? cached.snapshot,
            connections: connections ?? cached.connections,
            acpClientHosts: acpClientHosts ?? cached.acpClientHosts,
            mobileState: mobileState ?? cached.mobileState,
            pushDevices: pushDevices ?? cached.pushDevices,
            mobileHealth: mobileHealth,
            error: error
        )
    }

    public func replacingSessionMiniSnapshot(
        _ nextSessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?
    ) -> MenuRefreshResult {
        MenuRefreshResult(
            didFetchHTTP: didFetchHTTP,
            sessionMiniSnapshot: nextSessionMiniSnapshot ?? sessionMiniSnapshot,
            snapshot: snapshot,
            connections: connections,
            acpClientHosts: acpClientHosts,
            mobileState: mobileState,
            pushDevices: pushDevices,
            mobileHealth: mobileHealth,
            error: error
        )
    }

    public func replacingSessionMiniSnapshotDroppingHTTPEnrichment(
        _ nextSessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?
    ) -> MenuRefreshResult {
        MenuRefreshResult(
            didFetchHTTP: false,
            sessionMiniSnapshot: nextSessionMiniSnapshot ?? sessionMiniSnapshot,
            snapshot: nil,
            connections: nil,
            acpClientHosts: nil,
            mobileState: nil,
            pushDevices: nil,
            mobileHealth: nil,
            error: error
        )
    }
}

protocol MenuBarSessionMiniSnapshotProviding: Sendable {
    func cachedSnapshot() throws -> MenuBarSessionMiniLocalSnapshot
}

extension MenuBarSessionRuntime: MenuBarSessionMiniSnapshotProviding {}

public actor MenuRefreshCoordinator {
    private struct CachedRefresh: Sendable {
        let result: MenuRefreshResult
        let recordedAt: ContinuousClock.Instant
    }

    private struct InFlightRefresh: Sendable {
        let id: UInt64
        let task: Task<MenuRefreshResult, Never>
        let bypassesCache: Bool
    }

    private let client: any ControlPlaneClient
    private let sessionRuntime: (any MenuBarSessionMiniSnapshotProviding)?
    private let clock = ContinuousClock()
    private let freshReuseDuration: Duration
    private var inFlight: InFlightRefresh?
    private var cachedRefresh: CachedRefresh?
    private var nextRefreshID: UInt64 = 1

    public init(
        client: any ControlPlaneClient,
        sessionRuntime: MenuBarSessionRuntime? = nil,
        freshReuseDuration: Duration = .milliseconds(750)
    ) {
        self.client = client
        self.sessionRuntime = sessionRuntime
        self.freshReuseDuration = freshReuseDuration
    }

    init(
        client: any ControlPlaneClient,
        sessionMiniSnapshotProvider: (any MenuBarSessionMiniSnapshotProviding)?,
        freshReuseDuration: Duration = .milliseconds(750)
    ) {
        self.client = client
        self.sessionRuntime = sessionMiniSnapshotProvider
        self.freshReuseDuration = freshReuseDuration
    }

    public func refresh(force: Bool = false) async -> MenuRefreshResult {
        if force {
            return await performRefresh(bypassingCache: true)
        }

        if let inFlight {
            return await inFlight.task.value
        }

        let now = clock.now
        if !force,
           let cachedRefresh,
           shouldReuse(cachedRefresh: cachedRefresh, at: now)
        {
            return refreshLocalSessionMiniSnapshotDroppingHTTPEnrichment(in: cachedRefresh.result)
        }

        return await performRefresh(bypassingCache: false)
    }

    public func clearCache() {
        cachedRefresh = nil
    }

    private func shouldReuse(cachedRefresh: CachedRefresh, at now: ContinuousClock.Instant) -> Bool {
        if cachedRefresh.recordedAt.duration(to: now) <= freshReuseDuration {
            return true
        }

        return cachedRefresh.result.sessionMiniSnapshot != nil
            && Self.fetchSessionMiniSnapshot(sessionRuntime) != nil
    }

    private func performRefresh(bypassingCache: Bool) async -> MenuRefreshResult {
        while let inFlight {
            if !bypassingCache || inFlight.bypassesCache {
                return refreshLocalSessionMiniSnapshot(in: await inFlight.task.value)
            }
            let staleRefreshID = inFlight.id
            _ = await inFlight.task.value
            if self.inFlight?.id == staleRefreshID {
                self.inFlight = nil
            }
        }

        let client = self.client
        let sessionRuntime = self.sessionRuntime
        let shouldFetchDesktopSnapshot = bypassingCache
        let refreshID = nextRefreshID
        nextRefreshID += 1
        let task = Task {
            await Self.fetch(
                client: client,
                sessionRuntime: sessionRuntime,
                shouldFetchDesktopSnapshot: shouldFetchDesktopSnapshot
            )
        }
        inFlight = InFlightRefresh(id: refreshID, task: task, bypassesCache: bypassingCache)
        let result = await task.value
        if inFlight?.id == refreshID {
            inFlight = nil
            cachedRefresh = CachedRefresh(result: result, recordedAt: clock.now)
        }
        return result
    }

    private func refreshLocalSessionMiniSnapshot(in result: MenuRefreshResult) -> MenuRefreshResult {
        result.replacingSessionMiniSnapshot(Self.fetchSessionMiniSnapshot(sessionRuntime))
    }

    private func refreshLocalSessionMiniSnapshotDroppingHTTPEnrichment(
        in result: MenuRefreshResult
    ) -> MenuRefreshResult {
        result.replacingSessionMiniSnapshotDroppingHTTPEnrichment(
            Self.fetchSessionMiniSnapshot(sessionRuntime)
        )
    }

    private static func fetch(
        client: any ControlPlaneClient,
        sessionRuntime: (any MenuBarSessionMiniSnapshotProviding)?,
        shouldFetchDesktopSnapshot: Bool
    ) async -> MenuRefreshResult {
        let sessionMiniSnapshot = fetchSessionMiniSnapshot(sessionRuntime)
        if sessionMiniSnapshot != nil, !shouldFetchDesktopSnapshot {
            async let mobileState = fetchDesktopMobileState(client: client)
            async let pushDevices = fetchDesktopPushDevices(client: client)
            async let health = fetchMobileHealth(client: client)
            return MenuRefreshResult(
                didFetchHTTP: true,
                sessionMiniSnapshot: sessionMiniSnapshot,
                snapshot: nil,
                connections: nil,
                acpClientHosts: nil,
                mobileState: await mobileState,
                pushDevices: await pushDevices,
                mobileHealth: await health,
                error: nil
            )
        }

        async let snapshotResult = fetchSnapshot(client: client)
        async let connections = fetchDesktopConnections(client: client)
        async let acpClientHosts = fetchAcpClientHosts(client: client)
        async let mobileState = fetchDesktopMobileState(client: client)
        async let pushDevices = fetchDesktopPushDevices(client: client)
        async let health = fetchMobileHealth(client: client)

        switch await snapshotResult {
        case let .success(snapshot):
            return MenuRefreshResult(
                didFetchHTTP: true,
                sessionMiniSnapshot: sessionMiniSnapshot,
                snapshot: snapshot,
                connections: await connections,
                acpClientHosts: await acpClientHosts,
                mobileState: await mobileState,
                pushDevices: await pushDevices,
                mobileHealth: await health,
                error: nil
            )
        case let .failure(error):
            return MenuRefreshResult(
                didFetchHTTP: true,
                sessionMiniSnapshot: sessionMiniSnapshot,
                snapshot: nil,
                connections: await connections,
                acpClientHosts: await acpClientHosts,
                mobileState: await mobileState,
                pushDevices: await pushDevices,
                mobileHealth: await health,
                error: error
            )
        }
    }

    private static func fetchSessionMiniSnapshot(
        _ sessionRuntime: (any MenuBarSessionMiniSnapshotProviding)?
    ) -> MenuBarSessionMiniLocalSnapshot? {
        try? sessionRuntime?.cachedSnapshot()
    }

    private static func fetchSnapshot(
        client: any ControlPlaneClient
    ) async -> Result<DesktopSnapshotResponse, MenuRefreshError> {
        do {
            return .success(try await client.fetchDesktopSnapshot())
        } catch {
            return .failure(MenuRefreshError(error))
        }
    }

    private static func fetchMobileHealth(client: any ControlPlaneClient) async -> MobileHealthResponse? {
        try? await client.fetchMobileHealth()
    }

    private static func fetchDesktopMobileState(client: any ControlPlaneClient) async -> DesktopMobileStateResponse? {
        try? await client.fetchDesktopMobileState()
    }

    private static func fetchDesktopPushDevices(client: any ControlPlaneClient) async -> DesktopPushDevicesResponse? {
        try? await client.fetchDesktopPushDevices()
    }

    private static func fetchDesktopConnections(client: any ControlPlaneClient) async -> DesktopConnectionsResponse? {
        try? await client.fetchDesktopConnections()
    }

    private static func fetchAcpClientHosts(client: any ControlPlaneClient) async -> AcpClientHostsResponse? {
        try? await client.fetchAcpClientHosts()
    }
}

import Foundation

public struct MenuRefreshError: Error, Equatable, Sendable {
    public let message: String

    public init(_ error: Error) {
        self.message = String(describing: error)
    }
}

public struct MenuRefreshResult: Equatable, Sendable {
    public let snapshot: DesktopSnapshotResponse?
    public let connections: DesktopConnectionsResponse?
    public let acpClientHosts: AcpClientHostsResponse?
    public let mobileState: DesktopMobileStateResponse?
    public let pushDevices: DesktopPushDevicesResponse?
    public let mobileHealth: MobileHealthResponse?
    public let error: MenuRefreshError?

    public var succeeded: Bool {
        snapshot != nil
    }
}

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
    private let clock = ContinuousClock()
    private let freshReuseDuration: Duration
    private var inFlight: InFlightRefresh?
    private var cachedRefresh: CachedRefresh?
    private var nextRefreshID: UInt64 = 1

    public init(
        client: any ControlPlaneClient,
        freshReuseDuration: Duration = .milliseconds(750)
    ) {
        self.client = client
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
           cachedRefresh.recordedAt.duration(to: now) <= freshReuseDuration
        {
            return cachedRefresh.result
        }

        return await performRefresh(bypassingCache: false)
    }

    public func clearCache() {
        cachedRefresh = nil
    }

    private func performRefresh(bypassingCache: Bool) async -> MenuRefreshResult {
        while let inFlight {
            if !bypassingCache || inFlight.bypassesCache {
                return await inFlight.task.value
            }
            let staleRefreshID = inFlight.id
            _ = await inFlight.task.value
            if self.inFlight?.id == staleRefreshID {
                self.inFlight = nil
            }
        }

        let client = self.client
        let refreshID = nextRefreshID
        nextRefreshID += 1
        let task = Task {
            await Self.fetch(client: client)
        }
        inFlight = InFlightRefresh(id: refreshID, task: task, bypassesCache: bypassingCache)
        let result = await task.value
        if inFlight?.id == refreshID {
            inFlight = nil
            cachedRefresh = CachedRefresh(result: result, recordedAt: clock.now)
        }
        return result
    }

    private static func fetch(client: any ControlPlaneClient) async -> MenuRefreshResult {
        switch await fetchSnapshot(client: client) {
        case let .success(snapshot):
            async let connections = fetchDesktopConnections(client: client)
            async let acpClientHosts = fetchAcpClientHosts(client: client)
            async let mobileState = fetchDesktopMobileState(client: client)
            async let pushDevices = fetchDesktopPushDevices(client: client)
            async let health = fetchMobileHealth(client: client)
            return MenuRefreshResult(
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

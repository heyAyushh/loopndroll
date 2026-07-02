import Foundation

public struct MenuBarStorePrimaryMenuContent: Equatable, Sendable {
    public let sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?
    public let snapshot: DesktopSnapshotResponse?
    public let connections: DesktopConnectionsResponse?
    public let acpClientHosts: AcpClientHostsResponse?
}

public struct MenuBarStorePrimaryMenuState: Sendable {
    public private(set) var sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?

    private let cachedMenuEnrichmentMaxAge: TimeInterval
    private var cachedMenuEnrichment: MenuRefreshResult?
    private var cachedMenuEnrichmentRecordedAt: Date?

    public init(
        sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot? = nil,
        cachedMenuEnrichmentMaxAge: TimeInterval
    ) {
        self.sessionMiniSnapshot = sessionMiniSnapshot
        self.cachedMenuEnrichmentMaxAge = cachedMenuEnrichmentMaxAge
    }

    public mutating func applySessionMiniSnapshot(_ snapshot: MenuBarSessionMiniLocalSnapshot?) {
        guard let snapshot else {
            return
        }
        sessionMiniSnapshot = snapshot
    }

    public mutating func applyHTTPRefreshResult(
        _ result: MenuRefreshResult,
        recordedAt: Date = Date()
    ) {
        cacheMenuEnrichmentIfAvailable(result, recordedAt: recordedAt)
        applySessionMiniSnapshot(result.sessionMiniSnapshot)
    }

    public mutating func latestSessionMiniSnapshot(
        fallback: MenuBarSessionMiniLocalSnapshot?
    ) -> MenuBarSessionMiniLocalSnapshot? {
        sessionMiniSnapshot ?? fallback
    }

    public mutating func menuContent(at now: Date = Date()) -> MenuBarStorePrimaryMenuContent {
        let enrichment = currentMenuEnrichmentIfFresh(at: now)
        return MenuBarStorePrimaryMenuContent(
            sessionMiniSnapshot: sessionMiniSnapshot,
            snapshot: enrichment?.snapshot,
            connections: enrichment?.connections,
            acpClientHosts: enrichment?.acpClientHosts
        )
    }

    public mutating func menuContentForMenuWillOpen(
        at now: Date = Date()
    ) -> MenuBarStorePrimaryMenuContent {
        menuContent(at: now)
    }

    public mutating func cacheMenuEnrichmentIfAvailable(
        _ result: MenuRefreshResult,
        recordedAt: Date = Date()
    ) {
        guard result.hasReusableEnrichment else {
            return
        }
        cachedMenuEnrichment = result.mergingReusableEnrichment(
            from: currentMenuEnrichmentIfFresh(at: recordedAt)
        )
        cachedMenuEnrichmentRecordedAt = recordedAt
    }

    public mutating func currentMenuEnrichmentIfFresh(at now: Date = Date()) -> MenuRefreshResult? {
        guard let cachedMenuEnrichment else {
            return nil
        }
        guard let cachedMenuEnrichmentRecordedAt else {
            return cachedMenuEnrichment
        }
        guard now.timeIntervalSince(cachedMenuEnrichmentRecordedAt) <= cachedMenuEnrichmentMaxAge else {
            self.cachedMenuEnrichment = nil
            self.cachedMenuEnrichmentRecordedAt = nil
            return nil
        }
        return cachedMenuEnrichment
    }
}

@MainActor
public final class MenuBarSessionMiniMenuRebuildDebouncer {
    public nonisolated static let defaultDelay: Duration = .milliseconds(500)

    private let delay: Duration
    private let rebuild: @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void
    private var task: Task<Void, Never>?

    public init(
        delay: Duration = MenuBarSessionMiniMenuRebuildDebouncer.defaultDelay,
        rebuild: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void
    ) {
        self.delay = delay
        self.rebuild = rebuild
    }

    public func scheduleRebuild(from snapshot: MenuBarSessionMiniLocalSnapshot) {
        task?.cancel()
        task = Task { @MainActor [delay, rebuild, snapshot] in
            do {
                try await Task.sleep(for: delay)
            } catch {
                return
            }
            guard !Task.isCancelled else {
                return
            }
            rebuild(snapshot)
        }
    }

    public func rebuildNow(from snapshot: MenuBarSessionMiniLocalSnapshot) {
        task?.cancel()
        task = nil
        rebuild(snapshot)
    }

    public func cancel() {
        task?.cancel()
        task = nil
    }

    deinit {
        task?.cancel()
    }
}

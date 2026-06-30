import Foundation

public enum MobileRouteSessionPhase: Equatable, Sendable {
    case disconnected
    case connecting
    case ready
    case reconnecting
}

public enum MobileRouteSessionSyncReason: String, Equatable, Sendable {
    case delta
    case heartbeat
    case recovery
    case reconnecting
    case unknown

    public init(_ rawValue: String?) {
        let normalized = rawValue?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
        self = normalized.flatMap(Self.init(rawValue:)) ?? .unknown
    }

    public var provesLiveSession: Bool {
        switch self {
        case .delta, .heartbeat:
            true
        case .recovery, .reconnecting, .unknown:
            false
        }
    }
}

public struct MobileRouteReadinessState: Equatable, Sendable {
    private enum Defaults {
        static let minimumElapsedSeconds = 0
        static let healthFreshnessWindow: TimeInterval = 30
    }

    private enum Titles {
        static let localSessionProofUnavailable = "Local cache: Session proof unavailable"
        static let localWaitingForSessionProof = "Local cache: waiting for Session proof"
        static let cachedWaitingForSessionProof = "Cached route: waiting for Session proof"
        static let sessionProofSuffix = "waiting for Session proof"
        static let freshHandoffRoute = "Fresh handoff route"
        static let sessionProven = "Session-proven"
        static let sessionProvenHTTPStale = "Session-proven; HTTP enrichment stale"
        static let notProven = "Not proven"
        static let notActive = "Not active"
    }

    public private(set) var health: MobileHealthResponse?
    public private(set) var healthRecordedAt: Date?
    public private(set) var generation: UInt64
    public private(set) var provenRealtimeEndpoint: URL?
    public private(set) var staleRealtimeEndpoint: URL?
    private var pendingLiveProofGeneration: UInt64?
    private var observedSessionTransitionGeneration: UInt64?

    public init(
        health: MobileHealthResponse? = nil,
        healthRecordedAt: Date? = nil,
        generation: UInt64 = 0,
        provenRealtimeEndpoint: URL? = nil,
        staleRealtimeEndpoint: URL? = nil
    ) {
        self.health = health
        self.healthRecordedAt = healthRecordedAt
        self.generation = generation
        self.provenRealtimeEndpoint = provenRealtimeEndpoint
        self.staleRealtimeEndpoint = staleRealtimeEndpoint
        self.pendingLiveProofGeneration = nil
        self.observedSessionTransitionGeneration = nil
    }

    public var requiresLiveProof: Bool {
        pendingLiveProofGeneration == generation
    }

    public var hasLiveRouteProof: Bool {
        provenRealtimeEndpoint != nil && !requiresLiveProof
    }

    public var supportsNativeHandoff: Bool {
        provenReachableHandoffBaseURL != nil
    }

    public var provenReachableHandoffBaseURL: URL? {
        provenReachableHandoffBaseURL(now: Date())
    }

    public func provenReachableHandoffBaseURL(now: Date) -> URL? {
        guard hasLiveRouteProof,
              let provenRealtimeEndpoint,
              !MobileRouteURLPolicy.isLoopbackURL(provenRealtimeEndpoint),
              health?.ok == true,
              health?.requiresAuthentication == true,
              isHTTPHealthFresh(now: now)
        else {
            return nil
        }

        return MobileRouteURLPolicy.canonicalHTTPAPIBaseURL(for: provenRealtimeEndpoint)
    }

    public var mobileStatusTitle: String {
        guard hasLiveRouteProof else {
            return pendingProofStatusTitle
        }

        if hasStaleHTTPHealth {
            return Titles.sessionProvenHTTPStale
        }

        return supportsNativeHandoff ? Titles.freshHandoffRoute : Titles.sessionProven
    }

    public var routeStatusTitle: String {
        guard let provenRealtimeEndpoint, hasLiveRouteProof else {
            guard let staleRealtimeEndpoint, requiresLiveProof else {
                return pendingProofStatusTitle
            }
            return "Cached route: \(routeSummaryTitle(for: staleRealtimeEndpoint)); \(Titles.sessionProofSuffix)"
        }

        return "\(Titles.sessionProven): \(routeSummaryTitle(for: provenRealtimeEndpoint))"
    }

    public var tailscaleStatusTitle: String {
        guard let provenRealtimeEndpoint, hasLiveRouteProof else {
            guard let staleRealtimeEndpoint, requiresLiveProof else {
                return requiresLiveProof ? Titles.localWaitingForSessionProof : Titles.notProven
            }
            guard MobileRouteURLPolicy.routeTitle(for: staleRealtimeEndpoint) == "Tailscale" else {
                return Titles.localWaitingForSessionProof
            }
            return "Cached route: \(hostTitle(for: staleRealtimeEndpoint)); \(Titles.sessionProofSuffix)"
        }

        let routeTitle = MobileRouteURLPolicy.routeTitle(for: provenRealtimeEndpoint)
        guard routeTitle == "Tailscale" else {
            return Titles.notActive
        }

        return "\(Titles.sessionProven): \(hostTitle(for: provenRealtimeEndpoint))"
    }

    public func httpEnrichmentStatusTitle(now: Date = Date()) -> String? {
        guard health != nil else {
            return nil
        }
        guard let healthRecordedAt else {
            return "HTTP enrichment: age unknown, not Session proof"
        }

        let elapsedSeconds = max(
            Defaults.minimumElapsedSeconds,
            Int(now.timeIntervalSince(healthRecordedAt))
        )
        guard isHTTPHealthFresh(now: now) else {
            return "HTTP enrichment: stale \(elapsedSeconds)s old, not Session proof"
        }
        return "HTTP enrichment: \(elapsedSeconds)s old, not Session proof"
    }

    @discardableResult
    public mutating func invalidateForRouteSwitch() -> UInt64 {
        generation += 1
        health = nil
        healthRecordedAt = nil
        staleRealtimeEndpoint = provenRealtimeEndpoint ?? staleRealtimeEndpoint
        provenRealtimeEndpoint = nil
        pendingLiveProofGeneration = generation
        observedSessionTransitionGeneration = nil
        return generation
    }

    public mutating func applyHTTPHealth(
        _ nextHealth: MobileHealthResponse?,
        refreshGeneration: UInt64,
        recordedAt: Date = Date()
    ) {
        guard refreshGeneration == generation else {
            return
        }

        health = nextHealth
        healthRecordedAt = nextHealth == nil ? nil : recordedAt
    }

    public mutating func applySessionState(
        phase: MobileRouteSessionPhase,
        endpointURL: URL?,
        refreshGeneration: UInt64,
        syncReason: MobileRouteSessionSyncReason = .unknown
    ) {
        guard refreshGeneration == generation else {
            return
        }

        guard syncReason.provesLiveSession else {
            if requiresLiveProof,
               syncReason == .reconnecting || phase != .ready
            {
                observedSessionTransitionGeneration = generation
            }
            return
        }

        guard phase == .ready,
              let endpointURL,
              !endpointURL.absoluteString.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            provenRealtimeEndpoint = nil
            if requiresLiveProof {
                observedSessionTransitionGeneration = generation
            }
            return
        }

        guard acceptsReadyEndpoint(endpointURL) else {
            provenRealtimeEndpoint = nil
            return
        }

        provenRealtimeEndpoint = endpointURL
        staleRealtimeEndpoint = nil
        pendingLiveProofGeneration = nil
        observedSessionTransitionGeneration = nil
    }

    private var pendingProofStatusTitle: String {
        guard requiresLiveProof else {
            return Titles.localSessionProofUnavailable
        }
        return staleRealtimeEndpoint == nil
            ? Titles.localWaitingForSessionProof
            : Titles.cachedWaitingForSessionProof
    }

    private var hasStaleHTTPHealth: Bool {
        guard health != nil else {
            return false
        }
        return !isHTTPHealthFresh(now: Date())
    }

    private func isHTTPHealthFresh(now: Date) -> Bool {
        guard health != nil,
              let healthRecordedAt
        else {
            return false
        }
        return now.timeIntervalSince(healthRecordedAt) <= Defaults.healthFreshnessWindow
    }

    private func acceptsReadyEndpoint(_ endpointURL: URL) -> Bool {
        guard requiresLiveProof,
              let staleRealtimeEndpoint,
              staleRealtimeEndpoint.absoluteString == endpointURL.absoluteString
        else {
            return true
        }

        return observedSessionTransitionGeneration == generation
    }

    private func routeSummaryTitle(for url: URL) -> String {
        "\(MobileRouteURLPolicy.routeTitle(for: url)): \(hostTitle(for: url))"
    }

    private func hostTitle(for url: URL) -> String {
        url.host ?? url.absoluteString
    }
}

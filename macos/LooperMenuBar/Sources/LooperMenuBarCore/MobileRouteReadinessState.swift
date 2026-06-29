import Foundation

public enum MobileRouteSessionPhase: Equatable, Sendable {
    case disconnected
    case connecting
    case ready
    case reconnecting
}

public struct MobileRouteReadinessState: Equatable, Sendable {
    private enum Defaults {
        static let minimumElapsedSeconds = 0
    }

    private enum Titles {
        static let unknown = "Unknown"
        static let waitingForSessionProof = "Waiting for Session proof"
        static let staleWaitingForSessionProof = "Stale: waiting for Session proof"
        static let staleSessionProofSuffix = "waiting for Session proof"
        static let handoffRouteProven = "Handoff route proven"
        static let sessionConnected = "Session connected"
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
        guard hasLiveRouteProof,
              let provenRealtimeEndpoint,
              !MobileRouteURLPolicy.isLoopbackURL(provenRealtimeEndpoint),
              health?.ok == true,
              health?.requiresAuthentication == true
        else {
            return nil
        }

        return MobileRouteURLPolicy.canonicalHTTPAPIBaseURL(for: provenRealtimeEndpoint)
    }

    public var mobileStatusTitle: String {
        guard hasLiveRouteProof else {
            return pendingProofStatusTitle
        }

        return supportsNativeHandoff ? Titles.handoffRouteProven : Titles.sessionConnected
    }

    public var routeStatusTitle: String {
        guard let provenRealtimeEndpoint, hasLiveRouteProof else {
            guard let staleRealtimeEndpoint, requiresLiveProof else {
                return pendingProofStatusTitle
            }
            return "Stale: \(routeSummaryTitle(for: staleRealtimeEndpoint)), \(Titles.staleSessionProofSuffix)"
        }

        return "Connected: \(routeSummaryTitle(for: provenRealtimeEndpoint))"
    }

    public var tailscaleStatusTitle: String {
        guard let provenRealtimeEndpoint, hasLiveRouteProof else {
            guard let staleRealtimeEndpoint, requiresLiveProof else {
                return requiresLiveProof ? Titles.waitingForSessionProof : Titles.notProven
            }
            guard MobileRouteURLPolicy.routeTitle(for: staleRealtimeEndpoint) == "Tailscale" else {
                return Titles.waitingForSessionProof
            }
            return "Stale: \(hostTitle(for: staleRealtimeEndpoint)), \(Titles.staleSessionProofSuffix)"
        }

        let routeTitle = MobileRouteURLPolicy.routeTitle(for: provenRealtimeEndpoint)
        guard routeTitle == "Tailscale" else {
            return Titles.notActive
        }

        return "Connected: \(hostTitle(for: provenRealtimeEndpoint))"
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
        refreshGeneration: UInt64
    ) {
        guard refreshGeneration == generation else {
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
            return Titles.unknown
        }
        return staleRealtimeEndpoint == nil
            ? Titles.waitingForSessionProof
            : Titles.staleWaitingForSessionProof
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

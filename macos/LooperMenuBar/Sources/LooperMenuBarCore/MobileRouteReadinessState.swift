import Foundation

public enum MobileRouteSessionPhase: Equatable, Sendable {
    case disconnected
    case connecting
    case ready
    case reconnecting
}

public struct MobileRouteReadinessState: Equatable, Sendable {
    public private(set) var health: MobileHealthResponse?
    public private(set) var generation: UInt64
    public private(set) var provenRealtimeEndpoint: URL?
    private var pendingLiveProofGeneration: UInt64?

    public init(
        health: MobileHealthResponse? = nil,
        generation: UInt64 = 0,
        provenRealtimeEndpoint: URL? = nil
    ) {
        self.health = health
        self.generation = generation
        self.provenRealtimeEndpoint = provenRealtimeEndpoint
        self.pendingLiveProofGeneration = nil
    }

    public var requiresLiveProof: Bool {
        pendingLiveProofGeneration == generation
    }

    public var hasLiveRouteProof: Bool {
        provenRealtimeEndpoint != nil && !requiresLiveProof
    }

    public var supportsNativeHandoff: Bool {
        provenReachableHandoffBaseURL != nil
            && (health?.requiresAuthentication ?? true)
    }

    public var provenReachableHandoffBaseURL: URL? {
        guard hasLiveRouteProof,
              let provenRealtimeEndpoint,
              !MobileRouteURLPolicy.isLoopbackURL(provenRealtimeEndpoint)
        else {
            return nil
        }

        return MobileRouteURLPolicy.canonicalHTTPAPIBaseURL(for: provenRealtimeEndpoint)
    }

    public var mobileStatusTitle: String {
        guard hasLiveRouteProof else {
            return requiresLiveProof ? "Waiting for Session proof" : "Unknown"
        }

        return supportsNativeHandoff ? "Handoff route proven" : "Session connected"
    }

    public var routeStatusTitle: String {
        guard let provenRealtimeEndpoint, hasLiveRouteProof else {
            return requiresLiveProof ? "Waiting for Session proof" : "Unknown"
        }

        return "Connected: \(routeSummaryTitle(for: provenRealtimeEndpoint))"
    }

    public var tailscaleStatusTitle: String {
        guard let provenRealtimeEndpoint, hasLiveRouteProof else {
            return requiresLiveProof ? "Waiting for Session proof" : "Not proven"
        }

        let routeTitle = MobileRouteURLPolicy.routeTitle(for: provenRealtimeEndpoint)
        guard routeTitle == "Tailscale" else {
            return "Not active"
        }

        return "Connected: \(provenRealtimeEndpoint.host ?? provenRealtimeEndpoint.absoluteString)"
    }

    @discardableResult
    public mutating func invalidateForRouteSwitch() -> UInt64 {
        generation += 1
        health = nil
        provenRealtimeEndpoint = nil
        pendingLiveProofGeneration = generation
        return generation
    }

    public mutating func applyHTTPHealth(
        _ nextHealth: MobileHealthResponse?,
        refreshGeneration: UInt64
    ) {
        guard refreshGeneration == generation else {
            return
        }

        health = nextHealth
    }

    public mutating func applySessionState(
        phase: MobileRouteSessionPhase,
        endpointURL: URL?
    ) {
        guard phase == .ready,
              let endpointURL,
              !endpointURL.absoluteString.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        else {
            provenRealtimeEndpoint = nil
            return
        }

        provenRealtimeEndpoint = endpointURL
        pendingLiveProofGeneration = nil
    }

    private func routeSummaryTitle(for url: URL) -> String {
        "\(MobileRouteURLPolicy.routeTitle(for: url)): \(url.host ?? url.absoluteString)"
    }
}

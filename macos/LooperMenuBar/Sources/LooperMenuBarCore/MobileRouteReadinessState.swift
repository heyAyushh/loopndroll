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

    public private(set) var health: MobileHealthResponse?
    public private(set) var healthRecordedAt: Date?
    public private(set) var generation: UInt64
    public private(set) var provenRealtimeEndpoint: URL?
    private var pendingLiveProofGeneration: UInt64?

    public init(
        health: MobileHealthResponse? = nil,
        healthRecordedAt: Date? = nil,
        generation: UInt64 = 0,
        provenRealtimeEndpoint: URL? = nil
    ) {
        self.health = health
        self.healthRecordedAt = healthRecordedAt
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
        provenRealtimeEndpoint = nil
        pendingLiveProofGeneration = generation
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

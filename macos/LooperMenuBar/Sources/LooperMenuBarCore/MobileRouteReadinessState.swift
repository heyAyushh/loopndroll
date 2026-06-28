import Foundation

public struct MobileRouteReadinessState: Equatable, Sendable {
    public private(set) var health: MobileHealthResponse?
    public private(set) var generation: UInt64
    private var pendingLiveProofGeneration: UInt64?

    public init(health: MobileHealthResponse? = nil, generation: UInt64 = 0) {
        self.health = health
        self.generation = generation
        self.pendingLiveProofGeneration = nil
    }

    public var requiresLiveProof: Bool {
        pendingLiveProofGeneration == generation
    }

    @discardableResult
    public mutating func invalidateForRouteSwitch() -> UInt64 {
        generation += 1
        health = nil
        pendingLiveProofGeneration = generation
        return generation
    }

    public mutating func applyRefreshHealth(
        _ nextHealth: MobileHealthResponse?,
        refreshGeneration: UInt64,
        isLiveProof: Bool
    ) {
        guard refreshGeneration == generation else {
            return
        }

        guard !requiresLiveProof || isLiveProof else {
            health = nil
            return
        }

        health = nextHealth
        pendingLiveProofGeneration = nil
    }
}

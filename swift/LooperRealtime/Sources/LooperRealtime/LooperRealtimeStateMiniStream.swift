import Foundation
import LooperClientCore

public protocol LooperRealtimeClientCoreStateMiniStreamTransport: Sendable {
    func recoverClientCoreStateMiniSnapshot(
        clientCore: LooperClientCore
    ) async throws -> ClientStateSnapshot
    func startClientCoreStateMiniStream(clientCore: LooperClientCore) async throws
    func nextClientCoreStateMiniStreamUpdate(
        clientCore: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate
    func stopClientCoreStateMiniStream(clientCore: LooperClientCore) throws
}

public enum LooperRealtimeStateMiniUpdateReason: String, Codable, Equatable, Sendable {
    case snapshot
    case delta
    case recovery
}

public struct LooperRealtimeStateMiniUpdate: Equatable, Sendable {
    public let reason: LooperRealtimeStateMiniUpdateReason
    public let snapshot: LooperRealtimeLocalSnapshot

    public init(
        reason: LooperRealtimeStateMiniUpdateReason,
        snapshot: LooperRealtimeLocalSnapshot
    ) {
        self.reason = reason
        self.snapshot = snapshot
    }
}

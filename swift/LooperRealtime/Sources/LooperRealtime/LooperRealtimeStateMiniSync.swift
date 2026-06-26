import Foundation
import LooperClientCore

public protocol LooperRealtimeStateMiniSyncTransport: Sendable {
    func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot
    func streamStateMinis(
        afterSeq: Int64,
        onDelta: @escaping @Sendable (LooperRealtimeStateMiniDelta) async throws -> Void
    ) async throws
}

public protocol LooperRealtimeClientCoreStateMiniStreamTransport: Sendable {
    func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot
    func startClientCoreStateMiniStream(clientCore: LooperClientCore) async throws
    func nextClientCoreStateMiniStreamUpdate(
        clientCore: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate
    func stopClientCoreStateMiniStream(clientCore: LooperClientCore) throws
}

public protocol LooperRealtimeStateMiniLocalState: Sendable {
    func currentStateMiniSnapshot() -> LooperRealtimeLocalSnapshot

    @discardableResult
    func replaceStateMinis(with snapshot: LooperRealtimeStateMiniSnapshot) throws
        -> LooperRealtimeLocalSnapshot

    @discardableResult
    func applyStateMiniDelta(_ delta: LooperRealtimeStateMiniDelta) throws
        -> LooperRealtimeLocalSnapshot
}

public enum LooperRealtimeStateMiniSyncUpdateReason: String, Codable, Equatable, Sendable {
    case snapshot
    case delta
    case recovery
}

public struct LooperRealtimeStateMiniSyncUpdate: Equatable, Sendable {
    public let reason: LooperRealtimeStateMiniSyncUpdateReason
    public let snapshot: LooperRealtimeLocalSnapshot

    public init(
        reason: LooperRealtimeStateMiniSyncUpdateReason,
        snapshot: LooperRealtimeLocalSnapshot
    ) {
        self.reason = reason
        self.snapshot = snapshot
    }
}

public enum LooperRealtimeStateMiniSyncCycleResult: Equatable, Sendable {
    case streamEnded(latestSeq: Int64)
    case recovered(latestSeq: Int64)
    case retry(latestSeq: Int64, errorDescription: String)
}

public struct LooperRealtimeStateMiniSynchronizer: Sendable {
    public typealias Sleep = @Sendable (Duration) async throws -> Void
    public typealias UpdateHandler = @Sendable (LooperRealtimeStateMiniSyncUpdate) async -> Void

    private let store: any LooperRealtimeStateMiniLocalState
    private let transport: any LooperRealtimeStateMiniSyncTransport
    private let retryDelay: Duration
    private let sleep: Sleep

    public init(
        store: any LooperRealtimeStateMiniLocalState,
        transport: any LooperRealtimeStateMiniSyncTransport,
        retryDelay: Duration = .milliseconds(500),
        sleep: @escaping Sleep = { duration in try await Task.sleep(for: duration) }
    ) {
        self.store = store
        self.transport = transport
        self.retryDelay = retryDelay
        self.sleep = sleep
    }

    public func runUntilCancelled(onUpdate: @escaping UpdateHandler) async {
        while !Task.isCancelled {
            let result = await runOneCycle(onUpdate: onUpdate)
            switch result {
            case .streamEnded:
                return
            case .recovered, .retry:
                do {
                    try await sleep(retryDelay)
                } catch {
                    return
                }
            }
        }
    }

    public func runOneCycle(onUpdate: @escaping UpdateHandler) async
        -> LooperRealtimeStateMiniSyncCycleResult
    {
        let afterSeq = store.currentStateMiniSnapshot().latestSeq
        do {
            try await transport.streamStateMinis(afterSeq: afterSeq) { delta in
                let snapshot = try store.applyStateMiniDelta(delta)
                await onUpdate(
                    LooperRealtimeStateMiniSyncUpdate(
                        reason: .delta,
                        snapshot: snapshot
                    )
                )
            }
            return .streamEnded(latestSeq: store.currentStateMiniSnapshot().latestSeq)
        } catch {
            do {
                let snapshot = try await transport.getStateMiniSnapshot()
                let localSnapshot = try store.replaceStateMinis(with: snapshot)
                await onUpdate(
                    LooperRealtimeStateMiniSyncUpdate(
                        reason: .recovery,
                        snapshot: localSnapshot
                    )
                )
                return .recovered(latestSeq: localSnapshot.latestSeq)
            } catch {
                return .retry(
                    latestSeq: store.currentStateMiniSnapshot().latestSeq,
                    errorDescription: error.localizedDescription
                )
            }
        }
    }
}

extension LooperRealtimeLocalStore: LooperRealtimeStateMiniLocalState {
    public func currentStateMiniSnapshot() -> LooperRealtimeLocalSnapshot {
        snapshot()
    }

    @discardableResult
    public func replaceStateMinis(with snapshot: LooperRealtimeStateMiniSnapshot) throws
        -> LooperRealtimeLocalSnapshot
    {
        try replace(with: snapshot)
    }

    @discardableResult
    public func applyStateMiniDelta(_ delta: LooperRealtimeStateMiniDelta) throws
        -> LooperRealtimeLocalSnapshot
    {
        try apply(delta)
    }
}

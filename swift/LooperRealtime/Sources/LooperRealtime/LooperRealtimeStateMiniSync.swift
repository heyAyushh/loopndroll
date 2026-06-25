import Foundation

public protocol LooperRealtimeStateMiniSyncTransport: Sendable {
    func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot
    func streamStateMinis(
        afterSeq: Int64,
        onDelta: @escaping @Sendable (LooperRealtimeStateMiniDelta) async throws -> Void
    ) async throws
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

    private let store: LooperRealtimeLocalStore
    private let transport: any LooperRealtimeStateMiniSyncTransport
    private let retryDelay: Duration
    private let sleep: Sleep

    public init(
        store: LooperRealtimeLocalStore,
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
        let afterSeq = store.snapshot().latestSeq
        do {
            try await transport.streamStateMinis(afterSeq: afterSeq) { delta in
                let snapshot = try store.apply(delta)
                await onUpdate(
                    LooperRealtimeStateMiniSyncUpdate(
                        reason: .delta,
                        snapshot: snapshot
                    )
                )
            }
            return .streamEnded(latestSeq: store.snapshot().latestSeq)
        } catch {
            do {
                let snapshot = try await transport.getStateMiniSnapshot()
                let localSnapshot = try store.replace(with: snapshot)
                await onUpdate(
                    LooperRealtimeStateMiniSyncUpdate(
                        reason: .recovery,
                        snapshot: localSnapshot
                    )
                )
                return .recovered(latestSeq: localSnapshot.latestSeq)
            } catch {
                return .retry(
                    latestSeq: store.snapshot().latestSeq,
                    errorDescription: error.localizedDescription
                )
            }
        }
    }
}

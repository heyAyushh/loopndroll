import Foundation

@MainActor
protocol CompanionRealtimeControllerDelegate: AnyObject {
    var realtimeCurrentSnapshotRevision: String? { get }
    var realtimeHasSnapshot: Bool { get }

    func handleRealtimeStreamFailure(_ error: Error)
    func refreshRealtimeSnapshotAndLoadedDetails(for sessionIDs: Set<String>) async
}

@MainActor
final class CompanionRealtimeController {
    private var mobileEventStreamTask: Task<Void, Never>?
    private var realtimeRefreshTask: Task<Void, Never>?
    private var eventStreamRevision = 0
    private var activeConnectionRevision: Int?
    private var pendingRefreshSessionIDs: Set<String> = []
    private weak var delegate: (any CompanionRealtimeControllerDelegate)?

    var isActive: Bool {
        mobileEventStreamTask != nil
    }

    isolated deinit {
        stop()
    }

    func startIfNeeded(
        client: MobileEventStreamClient,
        connectionRevision: Int,
        delegate: any CompanionRealtimeControllerDelegate
    ) {
        guard mobileEventStreamTask == nil else {
            return
        }

        eventStreamRevision += 1
        activeConnectionRevision = connectionRevision
        self.delegate = delegate
        let streamRevision = eventStreamRevision

        mobileEventStreamTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                guard let self,
                      let delegate = self.delegate,
                      self.isCurrent(
                          streamRevision: streamRevision,
                          connectionRevision: connectionRevision
                      )
                else {
                    return
                }

                do {
                    try await client.streamEvents { [weak self] event in
                        guard let self,
                              let delegate = self.delegate
                        else {
                            return
                        }

                        self.handleMobileStreamEvent(
                            event,
                            delegate: delegate,
                            streamRevision: streamRevision,
                            connectionRevision: connectionRevision
                        )
                    }
                } catch {
                    guard !Task.isCancelled else {
                        return
                    }

                    delegate.handleRealtimeStreamFailure(error)
                    if !Self.isCancellationError(error) {
                        CompanionDiagnostics.record(
                            "events:stream-error error=\(error.localizedDescription)"
                        )
                    }
                }

                guard !Task.isCancelled,
                      self.isCurrent(
                          streamRevision: streamRevision,
                          connectionRevision: connectionRevision
                      )
                else {
                    return
                }

                try? await Task.sleep(for: CompanionMetrics.eventStreamReconnectDelay)
            }
        }
    }

    func stop() {
        eventStreamRevision += 1
        activeConnectionRevision = nil
        delegate = nil
        cancelRealtimeRefresh()
        mobileEventStreamTask?.cancel()
        mobileEventStreamTask = nil
    }

    private func handleMobileStreamEvent(
        _ event: MobileStreamEvent,
        delegate: any CompanionRealtimeControllerDelegate,
        streamRevision: Int,
        connectionRevision: Int
    ) {
        guard isCurrent(
            streamRevision: streamRevision,
            connectionRevision: connectionRevision
        ) else {
            CompanionDiagnostics.record("events:stale-skip")
            return
        }

        CompanionDiagnostics.record(
            "events:received type=\(event.eventType.rawValue) thread=\(event.threadID ?? "none")"
        )

        switch event.eventType {
        case .connected:
            if CompanionRealtimeSync.shouldRefreshSnapshot(
                for: event,
                currentRevision: delegate.realtimeCurrentSnapshotRevision,
                hasSnapshot: delegate.realtimeHasSnapshot
            ) {
                scheduleRealtimeRefresh(
                    event,
                    streamRevision: streamRevision,
                    connectionRevision: connectionRevision
                )
            }
        case .sessionChanged, .promptQueued, .promptDelivered, .lifecycleChanged:
            guard CompanionRealtimeSync.shouldRefreshSnapshot(
                for: event,
                currentRevision: delegate.realtimeCurrentSnapshotRevision,
                hasSnapshot: delegate.realtimeHasSnapshot
            ) else {
                CompanionDiagnostics.record("events:duplicate-revision-skip")
                return
            }
            scheduleRealtimeRefresh(
                event,
                streamRevision: streamRevision,
                connectionRevision: connectionRevision
            )
        }
    }

    private func scheduleRealtimeRefresh(
        _ event: MobileStreamEvent,
        streamRevision: Int,
        connectionRevision: Int
    ) {
        if let threadID = event.threadID {
            pendingRefreshSessionIDs.insert(threadID)
        }

        realtimeRefreshTask?.cancel()
        realtimeRefreshTask = Task { @MainActor [weak self] in
            try? await Task.sleep(for: CompanionRealtimeSync.snapshotRefreshDebounce)
            guard let self,
                  let delegate = self.delegate,
                  !Task.isCancelled,
                  self.isCurrent(
                      streamRevision: streamRevision,
                      connectionRevision: connectionRevision
                  )
            else {
                return
            }

            let sessionIDs = self.pendingRefreshSessionIDs
            self.pendingRefreshSessionIDs = []
            self.realtimeRefreshTask = nil

            await delegate.refreshRealtimeSnapshotAndLoadedDetails(for: sessionIDs)
        }
    }

    private func cancelRealtimeRefresh() {
        realtimeRefreshTask?.cancel()
        realtimeRefreshTask = nil
        pendingRefreshSessionIDs = []
    }

    private func isCurrent(streamRevision: Int, connectionRevision: Int) -> Bool {
        streamRevision == eventStreamRevision &&
            connectionRevision == activeConnectionRevision
    }

    private static func isCancellationError(_ error: Error) -> Bool {
        if error is CancellationError {
            return true
        }

        let nsError = error as NSError
        return nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled
    }
}

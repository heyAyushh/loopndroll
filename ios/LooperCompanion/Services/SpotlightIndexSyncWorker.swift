import Foundation

actor SpotlightIndexSyncWorker {
    private var currentTask: Task<Void, Never>?

    deinit {
        currentTask?.cancel()
    }

    func clearSessions(indexer: SessionSpotlightIndexer) async {
        schedule {
            try await indexer.deleteAllSessions()
        }
    }

    func syncSessions(
        indexer: SessionSpotlightIndexer,
        rebuildsIndex: Bool,
        removedSearchableIDs: [String],
        changedSessions: [SessionSummary],
        indexableSessions: [SessionSummary]
    ) async {
        schedule {
            if rebuildsIndex {
                try await indexer.deleteAllSessions()
            } else if !removedSearchableIDs.isEmpty {
                try await indexer.deleteSessions(withIDs: removedSearchableIDs)
            }
            try Task.checkCancellation()

            let sessionsToIndex = rebuildsIndex ? indexableSessions : changedSessions
            if !sessionsToIndex.isEmpty {
                try await indexer.indexSessions(sessionsToIndex)
            }
        }
    }

    private func schedule(_ operation: @escaping @Sendable () async throws -> Void) {
        currentTask?.cancel()
        currentTask = Task.detached(priority: .utility) {
            do {
                try Task.checkCancellation()
                try await operation()
                try Task.checkCancellation()
            } catch is CancellationError {
            } catch {
                print("Failed to update Spotlight sessions: \(error)")
            }
        }
    }
}

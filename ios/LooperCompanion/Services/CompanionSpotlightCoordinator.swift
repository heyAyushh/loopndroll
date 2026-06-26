import Foundation

@MainActor
final class CompanionSpotlightCoordinator {
    private let indexer: SessionSpotlightIndexer
    private let syncWorker: SpotlightIndexSyncWorker
    private var recordsBySessionID: [String: SessionSpotlightRecord] = [:]
    private var hasRebuiltIndexThisLaunch = false
    private var didClearIndexForCachedSnapshotThisLaunch = false

    init(
        indexer: SessionSpotlightIndexer = .shared,
        syncWorker: SpotlightIndexSyncWorker = SpotlightIndexSyncWorker()
    ) {
        self.indexer = indexer
        self.syncWorker = syncWorker
    }

    func sync(with sessions: [SessionSummary]) {
        let indexableSessions = SessionSpotlightIndexingPolicy.indexableSessions(from: sessions)
        let nextRecords = Dictionary(uniqueKeysWithValues: indexableSessions.map { session in
            (session.id, SessionSpotlightRecord(session: session))
        })
        let removedIDs = Set(recordsBySessionID.keys).subtracting(nextRecords.keys)
        let removedSearchableIDs = removedIDs.flatMap { sessionID in
            recordsBySessionID[sessionID]?.searchableIdentifiers ?? [sessionID]
        }
        let changedSessions = indexableSessions.filter { session in
            nextRecords[session.id] != recordsBySessionID[session.id]
        }
        let shouldRebuildIndex = !hasRebuiltIndexThisLaunch

        guard shouldRebuildIndex || !removedIDs.isEmpty || !changedSessions.isEmpty else {
            return
        }

        recordsBySessionID = nextRecords
        hasRebuiltIndexThisLaunch = true
        let indexer = indexer
        let syncWorker = syncWorker

        Task {
            await syncWorker.syncSessions(
                indexer: indexer,
                rebuildsIndex: shouldRebuildIndex,
                removedSearchableIDs: removedSearchableIDs,
                changedSessions: changedSessions,
                indexableSessions: indexableSessions
            )
        }
    }

    func clearForCachedSnapshotIfNeeded() {
        guard !didClearIndexForCachedSnapshotThisLaunch ||
            hasRebuiltIndexThisLaunch ||
            !recordsBySessionID.isEmpty
        else {
            return
        }

        didClearIndexForCachedSnapshotThisLaunch = true
        recordsBySessionID = [:]
        hasRebuiltIndexThisLaunch = false
        let indexer = indexer
        let syncWorker = syncWorker

        Task {
            await syncWorker.clearSessions(indexer: indexer)
        }
    }
}

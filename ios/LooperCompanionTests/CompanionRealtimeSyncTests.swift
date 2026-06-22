import Foundation
import Testing
@testable import Looper

@Suite("Companion realtime sync")
struct CompanionRealtimeSyncTests {
    private let decoder = JSONDecoder()

    @Test("Connected refreshes when no snapshot exists")
    func connectedRefreshesWithoutSnapshot() throws {
        let event = try mobileEvent(eventType: "connected", revision: "server-revision")

        let shouldRefresh = CompanionRealtimeSync.shouldRefreshSnapshot(
            for: event,
            currentRevision: nil,
            hasSnapshot: false
        )

        #expect(shouldRefresh)
    }

    @Test("Connected skips unchanged cached revision")
    func connectedSkipsUnchangedCachedRevision() throws {
        let event = try mobileEvent(eventType: "connected", revision: "server-revision")

        let shouldRefresh = CompanionRealtimeSync.shouldRefreshSnapshot(
            for: event,
            currentRevision: " server-revision ",
            hasSnapshot: true
        )

        #expect(!shouldRefresh)
    }

    @Test("Connected refreshes changed cached revision")
    func connectedRefreshesChangedCachedRevision() throws {
        let event = try mobileEvent(eventType: "connected", revision: "new-server-revision")

        let shouldRefresh = CompanionRealtimeSync.shouldRefreshSnapshot(
            for: event,
            currentRevision: "old-server-revision",
            hasSnapshot: true
        )

        #expect(shouldRefresh)
    }

    @Test("Unvalidated cached snapshot cannot satisfy realtime revision gate")
    func unvalidatedCachedSnapshotCannotSatisfyRealtimeRevisionGate() {
        #expect(
            CompanionRealtimeSync.revisionForRealtimeGate(
                currentRevision: "server-revision",
                hasValidatedSnapshotWithHTTP: false
            ) == nil
        )
        #expect(
            !CompanionRealtimeSync.hasRealtimeValidatedSnapshot(
                hasSnapshot: true,
                hasValidatedSnapshotWithHTTP: false
            )
        )
    }

    @Test("HTTP validated snapshot can satisfy realtime revision gate")
    func httpValidatedSnapshotCanSatisfyRealtimeRevisionGate() {
        #expect(
            CompanionRealtimeSync.revisionForRealtimeGate(
                currentRevision: " server-revision ",
                hasValidatedSnapshotWithHTTP: true
            ) == "server-revision"
        )
        #expect(
            CompanionRealtimeSync.hasRealtimeValidatedSnapshot(
                hasSnapshot: true,
                hasValidatedSnapshotWithHTTP: true
            )
        )
    }

    private func mobileEvent(
        eventType: String,
        revision: String?
    ) throws -> MobileStreamEvent {
        let payload: [String: String?] = [
            "eventType": eventType,
            "revision": revision,
        ]
        let data = try JSONSerialization.data(withJSONObject: payload.compactMapValues { $0 })
        return try decoder.decode(MobileStreamEvent.self, from: data)
    }
}

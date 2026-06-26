import Foundation
import Testing
@testable import LooperClientCore

@Suite("LooperClientCoreTests", .serialized)
struct LooperClientCoreTests {
    private let primaryEndpoint = "http://127.0.0.1:8765"
    private let lastGoodEndpoint = "http://100.64.0.2:8765"
    private let threadID = "thread-1"
    private let serverTime = "2026-06-25T00:00:00Z"

    @Test
    func configureSessionRuntimePrefersLastGoodEndpoint() throws {
        let manager = try temporarySessionManager()

        let snapshot = try manager.start(
            endpoints: [
                ClientEndpoint(url: primaryEndpoint, lastGood: false),
                ClientEndpoint(url: lastGoodEndpoint, lastGood: true),
            ],
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        )

        #expect(snapshot.phase == .ready)
        #expect(snapshot.endpointUrl == lastGoodEndpoint)
        #expect(snapshot.outboxDepth == 0)
    }

    @Test
    func snapshotReflectsConfiguredRuntime() throws {
        let manager = try temporarySessionManager()

        _ = try manager.start(
            endpoints: [ClientEndpoint(url: primaryEndpoint, lastGood: true)],
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        )
        let snapshot = try manager.stateSnapshot()

        #expect(snapshot.phase == .ready)
        #expect(snapshot.endpointUrl == primaryEndpoint)
        #expect(snapshot.pendingMutations.isEmpty)
    }

    @Test
    func mobileProjectionBuildsSnapshotFromStateMinis() throws {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: 10,
            sessions: [
                stateMini(sessionID: "thread-2", seq: 7, revision: "rev-7", title: "queued"),
                stateMini(sessionID: threadID, seq: 9, revision: "rev-9", title: "current"),
            ],
            serverTime: serverTime
        )

        #expect(projection.hasSnapshot)
        #expect(projection.snapshotJson.contains(#""revision":"rev-9""#))
        #expect(projection.snapshotJson.contains(#""lastSyncedAt":"\#(serverTime)""#))
        #expect(projection.snapshotJson.contains(#""id":"thread-1""#))
        #expect(projection.snapshotJson.contains(#""title":"current""#))
    }

    @Test
    func sessionManagerStartsWithoutSwiftStateMutationHooks() throws {
        let manager = try temporarySessionManager()

        let stateSnapshot = try manager.stateSnapshot()
        let localSnapshot = try manager.localSnapshot()

        #expect(stateSnapshot.latestSeq == 0)
        #expect(stateSnapshot.stateMinis.isEmpty)
        #expect(localSnapshot.latestSeq == 0)
        #expect(localSnapshot.sessions.isEmpty)
    }

    private func stateMini(
        sessionID: String,
        surface: String = "codex",
        seq: Int64,
        revision: String,
        title: String
    ) -> ClientStateMini {
        ClientStateMini(
            sessionId: sessionID,
            assistantSurface: surface,
            seq: seq,
            revision: revision,
            payloadJson: #"{"id":"\#(sessionID)","sessionId":"\#(sessionID)","assistantSurface":"\#(surface)","title":"\#(title)","ref":"\#(sessionID)","status":"active","lastActivityAtMs":\#(seq)}"#
        )
    }

    private func temporarySessionManager() throws -> LooperClientCoreSessionManager {
        let directoryURL = FileManager.default.temporaryDirectory.appendingPathComponent(
            "looper-client-core-tests-\(UUID().uuidString)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return try LooperClientCoreSessionManager(
            fileURL: directoryURL.appendingPathComponent("state-minis.json")
        )
    }
}

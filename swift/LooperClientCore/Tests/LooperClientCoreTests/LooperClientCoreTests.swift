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
        let core = LooperClientCore()

        let snapshot = try core.configureSessionRuntime(
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
        let core = LooperClientCore()

        _ = try core.configureSessionRuntime(
            endpoints: [ClientEndpoint(url: primaryEndpoint, lastGood: true)],
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        )
        let snapshot = try core.snapshot()

        #expect(snapshot.phase == .ready)
        #expect(snapshot.endpointUrl == primaryEndpoint)
        #expect(snapshot.pendingMutations.isEmpty)
    }

    @Test
    func stateMiniSnapshotReplacesAndNormalizesRecords() throws {
        let core = LooperClientCore()

        let snapshot = try core.replaceStateMinis(snapshot: ClientStateMiniSnapshot(
            latestSeq: 10,
            sessions: [
                stateMini(sessionID: "thread-2", seq: 7, revision: "rev-7", title: "queued"),
                stateMini(sessionID: threadID, seq: 5, revision: "rev-5", title: "old"),
                stateMini(sessionID: threadID, seq: 9, revision: "rev-9", title: "current"),
            ],
            serverTime: serverTime
        ))

        #expect(snapshot.latestSeq == 10)
        #expect(snapshot.revision == "rev-9")
        #expect(snapshot.serverTime == serverTime)
        #expect(snapshot.stateMinis.map(\.sessionId) == ["thread-2", threadID])
        #expect(snapshot.stateMinis.last?.payloadJson == #"{"title":"current"}"#)
    }

    @Test
    func stateMiniDeltaUpsertsAndIgnoresStaleSequences() throws {
        let core = LooperClientCore()
        _ = try core.replaceStateMinis(snapshot: ClientStateMiniSnapshot(
            latestSeq: 2,
            sessions: [stateMini(sessionID: threadID, seq: 2, revision: "rev-2", title: "old")],
            serverTime: ""
        ))

        let result = try core.applyStateMiniDeltaWithResult(delta: ClientStateMiniDelta(
            seq: 3,
            latestSeq: 3,
            entityId: threadID,
            kind: "session_mini",
            revision: "rev-3",
            serverTime: serverTime,
            hasSession: true,
            session: stateMini(sessionID: threadID, seq: 3, revision: "rev-3", title: "new"),
            sessions: []
        ))
        let snapshot = result.snapshot

        #expect(result.didChange)
        #expect(snapshot.latestSeq == 3)
        #expect(snapshot.revision == "rev-3")
        #expect(snapshot.stateMinis.map(\.payloadJson) == [#"{"title":"new"}"#])

        let staleResult = try core.applyStateMiniDeltaWithResult(delta: ClientStateMiniDelta(
            seq: 2,
            latestSeq: 2,
            entityId: threadID,
            kind: "session_mini",
            revision: "rev-stale",
            serverTime: "",
            hasSession: true,
            session: stateMini(sessionID: threadID, seq: 2, revision: "rev-stale", title: "stale"),
            sessions: []
        ))
        let stale = staleResult.snapshot

        #expect(!staleResult.didChange)
        #expect(stale.latestSeq == 3)
        #expect(stale.revision == "rev-3")
        #expect(stale.stateMinis.map(\.payloadJson) == [#"{"title":"new"}"#])
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
            payloadJson: #"{"title":"\#(title)"}"#
        )
    }
}

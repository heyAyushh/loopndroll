import XCTest
@testable import LooperClientCore

final class LooperClientCoreTests: XCTestCase {
    private let primaryEndpoint = "http://127.0.0.1:8765"
    private let lastGoodEndpoint = "http://100.64.0.2:8765"
    private let threadID = "thread-1"
    private let mutationID = "mutation-1"
    private let serverTime = "2026-06-25T00:00:00Z"

    func testConnectPrefersLastGoodEndpoint() throws {
        let core = LooperClientCore()

        let snapshot = try core.connect(endpoints: [
            ClientEndpoint(url: primaryEndpoint, lastGood: false),
            ClientEndpoint(url: lastGoodEndpoint, lastGood: true),
        ])

        XCTAssertEqual(snapshot.phase, .ready)
        XCTAssertEqual(snapshot.endpointUrl, lastGoodEndpoint)
        XCTAssertEqual(snapshot.outboxDepth, 0)
    }

    func testModeCommandQueuesPendingMutationAndOutboundFrame() throws {
        let core = LooperClientCore()

        let snapshot = try core.setMode(
            threadId: threadID,
            preset: "ask",
            clientMutationId: mutationID
        )

        XCTAssertEqual(snapshot.outboxDepth, 1)
        XCTAssertEqual(snapshot.pendingMutations.map(\.clientMutationId), [mutationID])
        XCTAssertEqual(snapshot.pendingMutations.first?.commandKind, .setSessionMode)

        let frames = try core.takeOutbox()
        XCTAssertEqual(frames.count, 1)
        XCTAssertEqual(frames.first?.frameKind, .command)
        XCTAssertEqual(frames.first?.commandKind, .setSessionMode)
        XCTAssertEqual(frames.first?.threadId, threadID)
        XCTAssertEqual(frames.first?.preset, "ask")
        XCTAssertEqual(frames.first?.clientMutationId, mutationID)
        XCTAssertEqual(try core.snapshot().outboxDepth, 0)
    }

    func testAcceptedAckClearsPendingMutation() throws {
        let core = LooperClientCore()
        _ = try core.sendPrompt(
            threadId: threadID,
            prompt: "reply now",
            assistantSurface: "ios",
            clientMutationId: mutationID
        )

        let snapshot = try core.applyCommandAck(ack: ClientCommandAck(
            accepted: true,
            clientMutationId: mutationID,
            ackSeq: 42,
            entityId: threadID,
            revision: "rev-42",
            serverTime: "2026-06-25T00:00:00Z",
            idempotentReplay: false,
            errorCode: "",
            rejectReason: "",
            currentState: ""
        ))

        XCTAssertTrue(snapshot.pendingMutations.isEmpty)
        XCTAssertEqual(snapshot.latestSeq, 42)
        XCTAssertEqual(snapshot.revision, "rev-42")
        XCTAssertEqual(snapshot.lastError, "")
    }

    func testRejectedAckRecordsStableError() throws {
        let core = LooperClientCore()
        _ = try core.setMode(
            threadId: threadID,
            preset: "send",
            clientMutationId: mutationID
        )

        let snapshot = try core.applyCommandAck(ack: ClientCommandAck(
            accepted: false,
            clientMutationId: mutationID,
            ackSeq: 7,
            entityId: threadID,
            revision: "rev-7",
            serverTime: "2026-06-25T00:00:00Z",
            idempotentReplay: false,
            errorCode: "mode_required",
            rejectReason: "session is waiting for a mode",
            currentState: "awaiting_mode"
        ))

        XCTAssertEqual(snapshot.pendingMutations, [])
        XCTAssertEqual(snapshot.latestSeq, 7)
        XCTAssertEqual(snapshot.lastError, "mode_required: session is waiting for a mode")
    }

    func testCommandBatchResponseMatchesAcksInRustCore() throws {
        let response = try buildCommandBatchResponse(
            commands: [
                ClientCommandMetadata(
                    commandKind: .setSessionMode,
                    clientMutationId: "mutation-mode",
                    preset: "await-reply",
                    dispatchKind: "",
                    notificationId: ""
                ),
                ClientCommandMetadata(
                    commandKind: .sendSessionPrompt,
                    clientMutationId: "mutation-prompt",
                    preset: "",
                    dispatchKind: "accepted",
                    notificationId: ""
                ),
            ],
            acks: [
                commandAck(clientMutationID: "unknown", accepted: true, ackSeq: 40),
                commandAck(clientMutationID: "mutation-prompt", accepted: true, ackSeq: 42),
                commandAck(clientMutationID: "mutation-mode", accepted: true, ackSeq: 41),
            ]
        )

        XCTAssertTrue(response.accepted)
        XCTAssertEqual(response.commandAcks.map(\.ack.clientMutationId), [
            "mutation-prompt",
            "mutation-mode",
        ])
        XCTAssertEqual(response.commandAcks.first?.dispatchKind, "accepted")
        XCTAssertEqual(response.commandAcks.last?.preset, "await-reply")
    }

    func testRejectedCommandBatchAckUsesRejectedDispatchKind() throws {
        let response = try buildCommandBatchResponse(
            commands: [
                ClientCommandMetadata(
                    commandKind: .submitNotificationReply,
                    clientMutationId: mutationID,
                    preset: "",
                    dispatchKind: "accepted",
                    notificationId: "notification-1"
                ),
            ],
            acks: [
                commandAck(clientMutationID: mutationID, accepted: false, ackSeq: 42),
            ]
        )

        XCTAssertFalse(response.accepted)
        XCTAssertEqual(response.commandAcks.first?.dispatchKind, "rejected")
        XCTAssertEqual(response.commandAcks.first?.notificationId, "notification-1")
    }

    func testStateDeltaAdvancesSequenceAndRevision() throws {
        let core = LooperClientCore()

        let snapshot = try core.applyStateDelta(delta: ClientStateDelta(
            seq: 99,
            entityId: threadID,
            kind: "session_mini",
            revision: "rev-99",
            serverTime: serverTime,
            payloadJson: "{}"
        ))

        XCTAssertEqual(snapshot.latestSeq, 99)
        XCTAssertEqual(snapshot.revision, "rev-99")
        XCTAssertEqual(snapshot.serverTime, serverTime)
    }

    func testStateMiniSnapshotReplacesAndNormalizesRecords() throws {
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

        XCTAssertEqual(snapshot.latestSeq, 10)
        XCTAssertEqual(snapshot.revision, "rev-9")
        XCTAssertEqual(snapshot.serverTime, serverTime)
        XCTAssertEqual(snapshot.stateMinis.map(\.sessionId), ["thread-2", threadID])
        XCTAssertEqual(snapshot.stateMinis.last?.payloadJson, #"{"title":"current"}"#)
    }

    func testStateMiniDeltaUpsertsAndIgnoresStaleSequences() throws {
        let core = LooperClientCore()
        _ = try core.replaceStateMinis(snapshot: ClientStateMiniSnapshot(
            latestSeq: 2,
            sessions: [stateMini(sessionID: threadID, seq: 2, revision: "rev-2", title: "old")],
            serverTime: ""
        ))

        let snapshot = try core.applyStateMiniDelta(delta: ClientStateMiniDelta(
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

        XCTAssertEqual(snapshot.latestSeq, 3)
        XCTAssertEqual(snapshot.revision, "rev-3")
        XCTAssertEqual(snapshot.stateMinis.map(\.payloadJson), [#"{"title":"new"}"#])

        let stale = try core.applyStateMiniDelta(delta: ClientStateMiniDelta(
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

        XCTAssertEqual(stale.latestSeq, 3)
        XCTAssertEqual(stale.revision, "rev-3")
        XCTAssertEqual(stale.stateMinis.map(\.payloadJson), [#"{"title":"new"}"#])
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

    private func commandAck(
        clientMutationID: String,
        accepted: Bool,
        ackSeq: Int64
    ) -> ClientCommandAck {
        ClientCommandAck(
            accepted: accepted,
            clientMutationId: clientMutationID,
            ackSeq: ackSeq,
            entityId: threadID,
            revision: "rev-\(ackSeq)",
            serverTime: serverTime,
            idempotentReplay: false,
            errorCode: "",
            rejectReason: "",
            currentState: ""
        )
    }
}

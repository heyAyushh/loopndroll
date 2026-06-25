import XCTest
@testable import LooperClientCore

final class LooperClientCoreTests: XCTestCase {
    private let primaryEndpoint = "http://127.0.0.1:8765"
    private let lastGoodEndpoint = "http://100.64.0.2:8765"
    private let threadID = "thread-1"
    private let mutationID = "mutation-1"

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

    func testStateDeltaAdvancesSequenceAndRevision() throws {
        let core = LooperClientCore()

        let snapshot = try core.applyStateDelta(delta: ClientStateDelta(
            seq: 99,
            entityId: threadID,
            kind: "session_mini",
            revision: "rev-99",
            serverTime: "2026-06-25T00:00:00Z",
            payloadJson: "{}"
        ))

        XCTAssertEqual(snapshot.latestSeq, 99)
        XCTAssertEqual(snapshot.revision, "rev-99")
    }
}

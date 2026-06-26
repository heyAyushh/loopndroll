import Testing
@testable import LooperClientCore

@Suite("LooperClientCoreTests", .serialized)
struct LooperClientCoreTests {
    private let primaryEndpoint = "http://127.0.0.1:8765"
    private let lastGoodEndpoint = "http://100.64.0.2:8765"
    private let threadID = "thread-1"
    private let mutationID = "mutation-1"
    private let serverTime = "2026-06-25T00:00:00Z"

    @Test
    func connectPrefersLastGoodEndpoint() throws {
        let core = LooperClientCore()

        let snapshot = try core.connect(endpoints: [
            ClientEndpoint(url: primaryEndpoint, lastGood: false),
            ClientEndpoint(url: lastGoodEndpoint, lastGood: true),
        ])

        #expect(snapshot.phase == .ready)
        #expect(snapshot.endpointUrl == lastGoodEndpoint)
        #expect(snapshot.outboxDepth == 0)
    }

    @Test
    func modeCommandQueuesPendingMutationAndOutboundFrame() throws {
        let core = LooperClientCore()

        let snapshot = try core.setMode(
            threadId: threadID,
            preset: "ask",
            clientMutationId: mutationID
        )

        #expect(snapshot.outboxDepth == 1)
        #expect(snapshot.pendingMutations.map(\.clientMutationId) == [mutationID])
        #expect(snapshot.pendingMutations.first?.commandKind == .setSessionMode)

        let frames = try core.takeOutbox()
        #expect(frames.count == 1)
        #expect(frames.first?.frameKind == .command)
        #expect(frames.first?.commandKind == .setSessionMode)
        #expect(frames.first?.threadId == threadID)
        #expect(frames.first?.preset == "ask")
        #expect(frames.first?.clientMutationId == mutationID)
        #expect(try core.snapshot().outboxDepth == 0)
    }

    @Test
    func expectedOutboxValidatesMutationOrderBeforeDrain() throws {
        let core = LooperClientCore()
        _ = try core.setMode(
            threadId: threadID,
            preset: "ask",
            clientMutationId: "mutation-mode"
        )
        _ = try core.sendPrompt(
            threadId: threadID,
            prompt: "reply now",
            assistantSurface: "ios",
            clientMutationId: "mutation-prompt"
        )

        #expect(throws: ClientCoreError.UnexpectedOutboxMutations) {
            _ = try core.takeExpectedOutbox(
            expectedClientMutationIds: ["mutation-prompt", "mutation-mode"]
            )
        }
        #expect(try core.snapshot().outboxDepth == 2)

        let frames = try core.takeExpectedOutbox(
            expectedClientMutationIds: ["mutation-mode", "mutation-prompt"]
        )

        #expect(frames.map(\.clientMutationId) == ["mutation-mode", "mutation-prompt"])
        #expect(try core.snapshot().outboxDepth == 0)
    }

    @Test
    func acceptedAckClearsPendingMutation() throws {
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

        #expect(snapshot.pendingMutations.isEmpty)
        #expect(snapshot.latestSeq == 42)
        #expect(snapshot.revision == "rev-42")
        #expect(snapshot.lastError == "")
    }

    @Test
    func rejectedAckRecordsStableError() throws {
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

        #expect(snapshot.pendingMutations == [])
        #expect(snapshot.latestSeq == 7)
        #expect(snapshot.lastError == "mode_required: session is waiting for a mode")
    }

    @Test
    func commandBatchResponseReconcilesInRustCore() throws {
        let core = LooperClientCore()
        _ = try core.setMode(
            threadId: threadID,
            preset: "ask",
            clientMutationId: "mutation-mode"
        )
        _ = try core.sendPrompt(
            threadId: threadID,
            prompt: "reply now",
            assistantSurface: "ios",
            clientMutationId: "mutation-prompt"
        )

        let snapshot = try core.applyCommandBatchResponse(response: ClientCommandBatchResponse(
            accepted: false,
            commandAcks: [
                ClientCommandAckEnvelope(
                    commandKind: .setSessionMode,
                    ack: commandAck(
                        clientMutationID: "mutation-mode",
                        accepted: true,
                        ackSeq: 41
                    ),
                    preset: "ask",
                    dispatchKind: "",
                    promptId: "",
                    notificationId: ""
                ),
                ClientCommandAckEnvelope(
                    commandKind: .sendSessionPrompt,
                    ack: commandAck(
                        clientMutationID: "mutation-prompt",
                        accepted: false,
                        ackSeq: 42,
                        errorCode: "mode_required",
                        rejectReason: "session is waiting for a mode"
                    ),
                    preset: "",
                    dispatchKind: "rejected",
                    promptId: "",
                    notificationId: ""
                ),
            ]
        ))

        #expect(snapshot.pendingMutations.isEmpty)
        #expect(snapshot.latestSeq == 42)
        #expect(snapshot.revision == "rev-42")
        #expect(snapshot.lastError == "mode_required: session is waiting for a mode")
    }

    @Test
    func commandBatchResponseMatchesAcksInRustCore() throws {
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

        #expect(response.accepted)
        #expect(response.commandAcks.map(\.ack.clientMutationId) == [
            "mutation-prompt",
            "mutation-mode",
        ])
        #expect(response.commandAcks.first?.dispatchKind == "accepted")
        #expect(response.commandAcks.last?.preset == "await-reply")
    }

    @Test
    func rejectedCommandBatchAckUsesRejectedDispatchKind() throws {
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

        #expect(!response.accepted)
        #expect(response.commandAcks.first?.dispatchKind == "rejected")
        #expect(response.commandAcks.first?.notificationId == "notification-1")
    }

    @Test
    func stateDeltaAdvancesSequenceAndRevision() throws {
        let core = LooperClientCore()

        let snapshot = try core.applyStateDelta(delta: ClientStateDelta(
            seq: 99,
            entityId: threadID,
            kind: "session_mini",
            revision: "rev-99",
            serverTime: serverTime,
            payloadJson: "{}"
        ))

        #expect(snapshot.latestSeq == 99)
        #expect(snapshot.revision == "rev-99")
        #expect(snapshot.serverTime == serverTime)
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

        #expect(snapshot.latestSeq == 3)
        #expect(snapshot.revision == "rev-3")
        #expect(snapshot.stateMinis.map(\.payloadJson) == [#"{"title":"new"}"#])

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

    private func commandAck(
        clientMutationID: String,
        accepted: Bool,
        ackSeq: Int64,
        errorCode: String = "",
        rejectReason: String = ""
    ) -> ClientCommandAck {
        ClientCommandAck(
            accepted: accepted,
            clientMutationId: clientMutationID,
            ackSeq: ackSeq,
            entityId: threadID,
            revision: "rev-\(ackSeq)",
            serverTime: serverTime,
            idempotentReplay: false,
            errorCode: errorCode,
            rejectReason: rejectReason,
            currentState: ""
        )
    }
}

import Foundation
import class LooperClientCore.LooperClientCore
import enum LooperClientCore.ClientCoreError
import struct LooperClientCore.ClientCommandAck
import struct LooperClientCore.ClientCommandAckEnvelope
import struct LooperClientCore.ClientCommandBatchResponse
import struct LooperClientCore.OutboundSessionFrame
import Testing

@testable import LooperRealtime

struct LooperRealtimeModelsTests {
    @Test
    func endpointStoresBaseURL() throws {
        let endpoint = try #require(URL(string: "https://192.168.1.4:8766"))
        let realtimeEndpoint = LooperRealtimeEndpoint(baseURL: endpoint)

        #expect(realtimeEndpoint.baseURL == endpoint)
    }

    @Test
    func endpointIsStablePoolKey() throws {
        let firstURL = try #require(URL(string: "http://127.0.0.1:8766"))
        let secondURL = try #require(URL(string: "http://127.0.0.1:8766"))
        let firstEndpoint = LooperRealtimeEndpoint(baseURL: firstURL)
        let secondEndpoint = LooperRealtimeEndpoint(baseURL: secondURL)

        #expect(Set([firstEndpoint, secondEndpoint]).count == 1)
    }

    @Test
    func commandResponsesExposeAckFields() {
        let mode = LooperRealtimeModeResponse(
            accepted: true,
            threadID: "thread-main",
            preset: "await-reply",
            serverTime: "2026-06-24T00:00:00Z",
            clientMutationID: "mutation-1",
            ackSeq: 42,
            entityID: "thread-main",
            revision: "revision-1",
            idempotentReplay: true
        )
        let prompt = LooperRealtimePromptResponse(
            accepted: true,
            dispatchKind: "queued",
            promptID: "prompt-1",
            serverTime: "2026-06-24T00:00:00Z",
            clientMutationID: "mutation-1",
            ackSeq: 42,
            entityID: "thread-main",
            revision: "revision-1",
            idempotentReplay: true
        )
        let notificationReply = LooperRealtimeNotificationReplyResponse(
            accepted: true,
            dispatchKind: "queued",
            promptID: "prompt-1",
            serverTime: "2026-06-24T00:00:00Z",
            clientMutationID: "mutation-1",
            ackSeq: 42,
            entityID: "thread-main",
            revision: "revision-1",
            idempotentReplay: true,
            notificationID: "notification-1"
        )
        let expectedAck = LooperRealtimeCommandAck(
            accepted: true,
            clientMutationID: "mutation-1",
            ackSeq: 42,
            entityID: "thread-main",
            revision: "revision-1",
            serverTime: "2026-06-24T00:00:00Z",
            idempotentReplay: true
        )

        #expect(mode.clientMutationID == "mutation-1")
        #expect(mode.ackSeq == 42)
        #expect(mode.entityID == "thread-main")
        #expect(mode.revision == "revision-1")
        #expect(mode.idempotentReplay)
        #expect(mode.ack == expectedAck)
        #expect(prompt.ack == expectedAck)
        #expect(notificationReply.ack == expectedAck)
    }

    @Test
    func commandBatchResponseUsesClientCoreAckEnvelope() {
        let coreAck = ClientCommandAck(
            accepted: false,
            clientMutationId: "mutation-1",
            ackSeq: 42,
            entityId: "thread-main",
            revision: "revision-1",
            serverTime: "2026-06-24T00:00:00Z",
            idempotentReplay: true,
            errorCode: "mode_required",
            rejectReason: "session is waiting for a mode",
            currentState: ""
        )
        let response = LooperRealtimeSessionCommandBatchResponse(
            ClientCommandBatchResponse(
                accepted: false,
                commandAcks: [
                    ClientCommandAckEnvelope(
                        commandKind: .submitNotificationReply,
                        ack: coreAck,
                        preset: "",
                        dispatchKind: "rejected",
                        promptId: "",
                        notificationId: "notification-1"
                    ),
                ]
            )
        )

        #expect(!response.accepted)
        #expect(response.commandAcks.first?.commandKind == "SubmitNotificationReply")
        #expect(response.commandAcks.first?.dispatchKind == "rejected")
        #expect(response.commandAcks.first?.notificationID == "notification-1")
        #expect(response.commandAcks.first?.ack.clientCoreAck == coreAck)
    }

    @Test
    func commandBatchResponseRoundTripsToClientCoreResponse() throws {
        let response = LooperRealtimeSessionCommandBatchResponse(
            accepted: false,
            commandAcks: [
                LooperRealtimeCommandAckEnvelope(
                    commandKind: "SendSessionPrompt",
                    ack: LooperRealtimeCommandAck(
                        accepted: false,
                        clientMutationID: "mutation-1",
                        ackSeq: 42,
                        entityID: "thread-main",
                        revision: "revision-42",
                        serverTime: "2026-06-24T00:00:00Z",
                        idempotentReplay: false,
                        errorCode: "mode_required",
                        rejectReason: "session is waiting for a mode"
                    ),
                    preset: nil,
                    dispatchKind: "rejected",
                    promptID: nil,
                    notificationID: nil
                ),
            ]
        )

        let coreResponse = try response.clientCoreResponse()

        #expect(!coreResponse.accepted)
        #expect(coreResponse.commandAcks.first?.commandKind == .sendSessionPrompt)
        #expect(coreResponse.commandAcks.first?.ack.clientMutationId == "mutation-1")
        #expect(coreResponse.commandAcks.first?.ack.errorCode == "mode_required")
        #expect(coreResponse.commandAcks.first?.dispatchKind == "rejected")
    }

    @Test
    func commandBatchResponseSelectsExpectedAcknowledgementInClientCore() throws {
        let response = LooperRealtimeSessionCommandBatchResponse(
            accepted: true,
            commandAcks: [
                LooperRealtimeCommandAckEnvelope(
                    commandKind: "SetSessionMode",
                    ack: LooperRealtimeCommandAck(
                        accepted: true,
                        clientMutationID: "mutation-mode",
                        ackSeq: 41,
                        entityID: "thread-main",
                        revision: "revision-41",
                        serverTime: "2026-06-24T00:00:00Z",
                        idempotentReplay: false
                    ),
                    preset: "await-reply",
                    dispatchKind: nil,
                    promptID: nil,
                    notificationID: nil
                ),
                LooperRealtimeCommandAckEnvelope(
                    commandKind: "SendSessionPrompt",
                    ack: LooperRealtimeCommandAck(
                        accepted: true,
                        clientMutationID: "mutation-prompt",
                        ackSeq: 42,
                        entityID: "thread-main",
                        revision: "revision-42",
                        serverTime: "2026-06-24T00:00:00Z",
                        idempotentReplay: false
                    ),
                    preset: nil,
                    dispatchKind: "accepted",
                    promptID: "prompt-1",
                    notificationID: nil
                ),
            ]
        )

        let envelope = try response.expectedAcknowledgement(
            commandKind: .sendSessionPrompt,
            clientMutationID: "mutation-prompt"
        )

        #expect(envelope.commandKind == "SendSessionPrompt")
        #expect(envelope.ack.clientMutationID == "mutation-prompt")
        #expect(envelope.dispatchKind == "accepted")
        #expect(envelope.promptID == "prompt-1")
    }

    @Test
    func commandBatchResponseUsesClientCoreForMissingAcknowledgement() {
        let response = LooperRealtimeSessionCommandBatchResponse(
            accepted: true,
            commandAcks: []
        )

        #expect(throws: ClientCoreError.MissingCommandAcknowledgement) {
            _ = try response.expectedAcknowledgement(
                commandKind: .sendSessionPrompt,
                clientMutationID: "mutation-prompt"
            )
        }
    }

    @Test
    func commandBatchResponseRejectsUnknownCommandKind() {
        let response = LooperRealtimeSessionCommandBatchResponse(
            accepted: true,
            commandAcks: [
                LooperRealtimeCommandAckEnvelope(
                    commandKind: "UnexpectedCommand",
                    ack: LooperRealtimeCommandAck(
                        accepted: true,
                        clientMutationID: "mutation-1",
                        ackSeq: 42,
                        entityID: "thread-main",
                        revision: "revision-42",
                        serverTime: "2026-06-24T00:00:00Z",
                        idempotentReplay: false
                    ),
                    preset: nil,
                    dispatchKind: nil,
                    promptID: nil,
                    notificationID: nil
                ),
            ]
        )

        #expect(throws: LooperRealtimeSessionCommandFrameError.unexpectedCommandKind("UnexpectedCommand")) {
            try response.clientCoreResponse()
        }
    }

    @Test
    func clientCoreOutboxSubmitterDrainsAndReconcilesAck() async throws {
        let core = LooperClientCore()
        _ = try core.sendPrompt(
            threadId: "thread-main",
            prompt: "ship it",
            assistantSurface: "codex",
            clientMutationId: "mutation-1"
        )
        let submitter = RecordingCommandSubmitter { frames in
            #expect(frames.map(\.threadId) == ["thread-main"])
            #expect(frames.map(\.prompt) == ["ship it"])
            #expect(frames.map(\.assistantSurface) == ["codex"])
            #expect(frames.map(\.clientMutationId) == ["mutation-1"])
            return ClientCommandBatchResponse(
                accepted: true,
                commandAcks: [
                    ClientCommandAckEnvelope(
                        commandKind: .sendSessionPrompt,
                        ack: ClientCommandAck(
                            accepted: true,
                            clientMutationId: "mutation-1",
                            ackSeq: 42,
                            entityId: "thread-main",
                            revision: "revision-42",
                            serverTime: "2026-06-24T00:00:00Z",
                            idempotentReplay: false,
                            errorCode: "",
                            rejectReason: "",
                            currentState: ""
                        ),
                        preset: "",
                        dispatchKind: "accepted",
                        promptId: "",
                        notificationId: ""
                    ),
                ]
            )
        }

        let response = try await submitter.submitClientCoreOutbox(
            clientCore: core,
            expectedClientMutationIDs: ["mutation-1"]
        )
        let snapshot = try core.snapshot()

        #expect(response.accepted)
        #expect(snapshot.pendingMutations.isEmpty)
        #expect(snapshot.outboxDepth == 0)
        #expect(snapshot.latestSeq == 42)
        #expect(snapshot.revision == "revision-42")
    }

    @Test
    func clientCoreOutboxSubmitterRejectsUnexpectedMutations() async throws {
        let core = LooperClientCore()
        _ = try core.setMode(
            threadId: "thread-main",
            preset: "await-reply",
            clientMutationId: "actual-mutation"
        )
        let submitter = RecordingCommandSubmitter { _ in
            Issue.record("unexpected submit")
            return ClientCommandBatchResponse(accepted: true, commandAcks: [])
        }

        do {
            _ = try await submitter.submitClientCoreOutbox(
                clientCore: core,
                expectedClientMutationIDs: ["expected-mutation"]
            )
            Issue.record("expected unexpected mutation error")
        } catch let error as ClientCoreError {
            #expect(error == .UnexpectedOutboxMutations)
        } catch {
            Issue.record("unexpected error: \(error)")
        }
    }

}

private struct RecordingCommandSubmitter: LooperRealtimeSessionCommandSubmitting {
    let handler: @Sendable ([OutboundSessionFrame]) async throws
        -> ClientCommandBatchResponse

    func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        let frames = try clientCore.takeExpectedOutbox(
            expectedClientMutationIds: expectedClientMutationIDs
        )
        let response = try await handler(frames)
        _ = try clientCore.applyCommandBatchResponse(response: response)
        return LooperRealtimeSessionCommandBatchResponse(response)
    }
}

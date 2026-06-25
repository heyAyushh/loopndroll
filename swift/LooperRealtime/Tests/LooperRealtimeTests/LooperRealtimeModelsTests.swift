import Foundation
import GRPCCore
import Testing

@testable import LooperRealtime

struct LooperRealtimeModelsTests {
    @Test
    func endpointExposesHostPortAndTLS() throws {
        let endpoint = try #require(URL(string: "https://192.168.1.4:8766"))
        let realtimeEndpoint = LooperRealtimeEndpoint(baseURL: endpoint)

        #expect(realtimeEndpoint.host == "192.168.1.4")
        #expect(realtimeEndpoint.port == 8766)
        #expect(realtimeEndpoint.usesTLS)
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
    func latencyPolicyKeepsRealtimeTransportWarm() {
        let config = LooperRealtimeLatencyPolicy.transportConfig

        #expect(config.connection.maxIdleTime == nil)
        #expect(config.connection.keepalive?.time == LooperRealtimeLatencyPolicy.keepaliveTime)
        #expect(config.connection.keepalive?.timeout == LooperRealtimeLatencyPolicy.keepaliveTimeout)
        #expect(config.connection.keepalive?.allowWithoutCalls == true)
        #expect(config.backoff.initial == LooperRealtimeLatencyPolicy.reconnectInitialBackoff)
        #expect(config.backoff.max == LooperRealtimeLatencyPolicy.reconnectMaxBackoff)
    }

    @Test
    func latencyPolicyFailsUserActionsFast() {
        let warmupOptions = LooperRealtimeLatencyPolicy.warmupCallOptions
        let promptOptions = LooperRealtimeLatencyPolicy.promptCallOptions
        let streamOptions = LooperRealtimeLatencyPolicy.streamCallOptions

        #expect(warmupOptions.timeout == LooperRealtimeLatencyPolicy.warmupTimeout)
        #expect(warmupOptions.waitForReady == false)
        #expect(promptOptions.timeout == LooperRealtimeLatencyPolicy.promptTimeout)
        #expect(promptOptions.waitForReady == false)
        #expect(streamOptions.timeout == nil)
        #expect(streamOptions.waitForReady == false)
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

}

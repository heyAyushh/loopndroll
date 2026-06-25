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

    @Test
    func localStoreDedupesOutboxAndAppliesMiniDeltas() throws {
        let fileURL = temporaryStoreFileURL()
        let store = try LooperRealtimeLocalStore(fileURL: fileURL)
        let cached = LooperRealtimeStateMini(
            sessionID: "thread-main",
            assistantSurface: "codex",
            seq: 1,
            revision: "rev-1",
            payloadJSON: #"{"sessionId":"thread-main","assistantSurface":"codex","title":"Cached"}"#
        )

        try store.replace(with: LooperRealtimeStateMiniSnapshot(
            latestSeq: 1,
            sessions: [cached],
            serverTime: "2026-06-24T00:00:00Z"
        ))
        try store.enqueue(LooperRealtimePendingCommand(
            kind: .sendSessionPrompt,
            clientMutationID: "mutation-1",
            threadID: "thread-main",
            prompt: "continue"
        ))
        try store.enqueue(LooperRealtimePendingCommand(
            kind: .sendSessionPrompt,
            clientMutationID: "mutation-1",
            threadID: "thread-main",
            prompt: "continue"
        ))
        try store.markAttempted(clientMutationID: "mutation-1")
        try store.markAttempted(clientMutationID: "mutation-1")

        var snapshot = store.snapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.attemptCount == 2)

        let synced = LooperRealtimeStateMini(
            sessionID: "thread-main",
            assistantSurface: "codex",
            seq: 2,
            revision: "rev-2",
            payloadJSON: #"{"sessionId":"thread-main","assistantSurface":"codex","title":"Synced"}"#
        )
        try store.apply(LooperRealtimeStateMiniDelta(
            seq: 2,
            latestSeq: 2,
            entityID: "session-mini:codex:thread-main",
            kind: "session-mini.changed",
            revision: "rev-2",
            serverTime: nil,
            session: synced,
            sessionID: "thread-main",
            assistantSurface: "codex",
            sessions: [synced]
        ))
        try store.markDelivered(clientMutationID: "mutation-1")

        snapshot = store.snapshot()
        #expect(snapshot.latestSeq == 2)
        #expect(snapshot.sessions == [synced])
        #expect(snapshot.pendingCommands.isEmpty)
    }

    @Test
    func synchronizerRecoversThenResumesFromSnapshotSeq() async throws {
        let fileURL = temporaryStoreFileURL()
        let store = try LooperRealtimeLocalStore(fileURL: fileURL)
        let recovered = LooperRealtimeStateMini(
            sessionID: "thread-recovered",
            assistantSurface: "codex",
            seq: 6,
            revision: "rev-6",
            payloadJSON: #"{"sessionId":"thread-recovered","assistantSurface":"codex"}"#
        )
        let updated = LooperRealtimeStateMini(
            sessionID: "thread-recovered",
            assistantSurface: "codex",
            seq: 7,
            revision: "rev-7",
            payloadJSON: #"{"sessionId":"thread-recovered","assistantSurface":"codex"}"#
        )
        let transport = RecordingStateMiniSyncTransport(
            snapshots: [
                LooperRealtimeStateMiniSnapshot(
                    latestSeq: 6,
                    sessions: [recovered],
                    serverTime: nil
                ),
            ],
            streamPlans: [
                .recoveryRequired,
                .deltas([
                    LooperRealtimeStateMiniDelta(
                        seq: 7,
                        latestSeq: 7,
                        entityID: "session-mini:codex:thread-recovered",
                        kind: "session-mini.changed",
                        revision: "rev-7",
                        serverTime: nil,
                        session: updated,
                        sessionID: "thread-recovered",
                        assistantSurface: "codex",
                        sessions: [updated]
                    ),
                ]),
            ]
        )
        let synchronizer = LooperRealtimeStateMiniSynchronizer(
            store: store,
            transport: transport,
            sleep: { _ in }
        )

        let first = await synchronizer.runOneCycle { _ in }
        let second = await synchronizer.runOneCycle { _ in }

        #expect(first == .recovered(latestSeq: 6))
        #expect(second == .streamEnded(latestSeq: 7))
        #expect(await transport.observedAfterSeqs() == [0, 6])
        #expect(store.snapshot().sessions == [updated])
    }

    private func temporaryStoreFileURL() -> URL {
        let directoryURL = FileManager.default.temporaryDirectory
            .appending(path: "looper-realtime-tests")
            .appending(path: UUID().uuidString)
        try? FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return directoryURL.appending(path: LooperRealtimeLocalStore.defaultFileName)
    }
}

private actor RecordingStateMiniSyncTransport: LooperRealtimeStateMiniSyncTransport {
    enum StreamPlan: Sendable {
        case deltas([LooperRealtimeStateMiniDelta])
        case recoveryRequired
    }

    private var snapshots: [LooperRealtimeStateMiniSnapshot]
    private var streamPlans: [StreamPlan]
    private var afterSeqs: [Int64] = []

    init(
        snapshots: [LooperRealtimeStateMiniSnapshot],
        streamPlans: [StreamPlan]
    ) {
        self.snapshots = snapshots
        self.streamPlans = streamPlans
    }

    func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot {
        guard !snapshots.isEmpty else {
            throw LooperRealtimeError.unavailable
        }
        return snapshots.removeFirst()
    }

    func streamStateMinis(
        afterSeq: Int64,
        onDelta: @escaping @Sendable (LooperRealtimeStateMiniDelta) async throws -> Void
    ) async throws {
        afterSeqs.append(afterSeq)
        let plan = streamPlans.isEmpty ? .deltas([]) : streamPlans.removeFirst()
        switch plan {
        case let .deltas(deltas):
            for delta in deltas {
                try await onDelta(delta)
            }
        case .recoveryRequired:
            throw RecordingRecoveryRequiredError()
        }
    }

    func observedAfterSeqs() -> [Int64] {
        afterSeqs
    }
}

private struct RecordingRecoveryRequiredError: LocalizedError, Sendable {
    var errorDescription: String? {
        "state mini recovery required"
    }
}

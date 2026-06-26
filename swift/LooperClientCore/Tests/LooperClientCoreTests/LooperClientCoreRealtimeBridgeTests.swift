import Foundation
import Testing
@testable import LooperClientCore

struct LooperClientCoreRealtimeBridgeTests {
    @Test
    func notificationReplyResponseExposesAckFields() {
        let response = LooperRealtimeNotificationReplyResponse(
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

        #expect(response.ack == expectedAck)
        #expect(response.notificationID == "notification-1")
    }

    @Test
    func durableModeCommandPersistsBeforeTransport() async throws {
        let core = LooperClientCore()
        let store = try localStore(named: "durable-mode")

        do {
            _ = try await core.submitSetModeDurable(
                localStore: store,
                threadId: "thread-main",
                preset: "await-reply",
                clientMutationId: "mutation-mode"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try store.snapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .setSessionMode)
        #expect(snapshot.pendingCommands.first?.clientMutationId == "mutation-mode")
        #expect(snapshot.pendingCommands.first?.threadId == "thread-main")
        #expect(snapshot.pendingCommands.first?.preset == "await-reply")
        #expect(snapshot.pendingCommands.first?.attemptCount == 1)
        #expect(try core.snapshot().outboxDepth == 1)
    }

    @Test
    func durablePromptCommandPersistsBeforeTransport() async throws {
        let core = LooperClientCore()
        let store = try localStore(named: "durable-prompt")

        do {
            _ = try await core.submitSendPromptDurable(
                localStore: store,
                threadId: "thread-main",
                prompt: "ship it",
                assistantSurface: "codex",
                clientMutationId: "mutation-prompt"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try store.snapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .sendSessionPrompt)
        #expect(snapshot.pendingCommands.first?.clientMutationId == "mutation-prompt")
        #expect(snapshot.pendingCommands.first?.prompt == "ship it")
        #expect(snapshot.pendingCommands.first?.assistantSurface == "codex")
        #expect(snapshot.pendingCommands.first?.attemptCount == 1)
        #expect(try core.snapshot().outboxDepth == 1)
    }

    @Test
    func durableNotificationReplyRetryDedupesCommand() async throws {
        let core = LooperClientCore()
        let store = try localStore(named: "durable-notification-retry")

        for _ in 0..<2 {
            do {
                _ = try await core.submitNotificationReplyDurable(
                    localStore: store,
                    notificationId: "notification-main",
                    threadId: "thread-main",
                    prompt: "continue",
                    assistantSurface: "codex",
                    clientMutationId: "notification-reply:notification-main"
                )
                Issue.record("expected missing runtime config")
            } catch let error as ClientCoreError {
                #expect(error == .NoEndpoint)
            }
        }

        let snapshot = try store.snapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .submitNotificationReply)
        #expect(snapshot.pendingCommands.first?.clientMutationId == "notification-reply:notification-main")
        #expect(snapshot.pendingCommands.first?.notificationId == "notification-main")
        #expect(snapshot.pendingCommands.first?.prompt == "continue")
        #expect(snapshot.pendingCommands.first?.attemptCount == 2)
        #expect(try core.snapshot().outboxDepth == 1)
    }

    @Test
    func sessionManagerOwnsDurableCommandBoundary() async throws {
        let core = LooperClientCore()
        let store = try localStore(named: "session-manager-command-boundary")
        let manager = LooperClientCoreSessionManager(clientCore: core, localStore: store)

        do {
            _ = try await manager.sendPrompt(
                threadID: "thread-main",
                prompt: "continue",
                assistantSurface: "codex",
                clientMutationID: "mutation-manager-prompt"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try store.snapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .sendSessionPrompt)
        #expect(snapshot.pendingCommands.first?.clientMutationId == "mutation-manager-prompt")
        #expect(try manager.outboxDepth() == 1)
    }

    @Test
    func duplicateClientMutationReplacesCoreOutboxFrame() throws {
        let core = LooperClientCore()
        _ = try core.sendPrompt(
            threadId: "thread-main",
            prompt: "first",
            assistantSurface: "codex",
            clientMutationId: "mutation-prompt"
        )
        _ = try core.sendPrompt(
            threadId: "thread-main",
            prompt: "second",
            assistantSurface: "codex",
            clientMutationId: "mutation-prompt"
        )

        let outbox = try core.takeExpectedOutbox(
            expectedClientMutationIds: ["mutation-prompt"]
        )
        #expect(outbox.count == 1)
        #expect(outbox.first?.prompt == "second")
        #expect(outbox.first?.clientMutationId == "mutation-prompt")
    }

    private func localStore(named name: String) throws -> LooperClientCoreLocalStore {
        try LooperClientCoreLocalStore(
            filePath: FileManager.default.temporaryDirectory
                .appendingPathComponent("LooperClientCoreTests-\(UUID().uuidString)", isDirectory: true)
                .appendingPathComponent("\(name)-state-minis.json")
                .path
        )
    }
}

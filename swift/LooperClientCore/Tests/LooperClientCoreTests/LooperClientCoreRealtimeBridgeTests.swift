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
        let manager = try LooperClientCoreSessionManager(
            filePath: localStorePath(named: "durable-mode")
        )

        do {
            _ = try await manager.setMode(
                threadID: "thread-main",
                preset: "await-reply"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try manager.localSnapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .setSessionMode)
        #expect(snapshot.pendingCommands.first?.clientMutationId.hasPrefix("mode-") == true)
        #expect(snapshot.pendingCommands.first?.threadId == "thread-main")
        #expect(snapshot.pendingCommands.first?.preset == "await-reply")
        #expect(snapshot.pendingCommands.first?.attemptCount == 1)
        #expect(try manager.outboxDepth() == 1)
    }

    @Test
    func durablePromptCommandPersistsBeforeTransport() async throws {
        let manager = try LooperClientCoreSessionManager(
            filePath: localStorePath(named: "durable-prompt")
        )

        do {
            _ = try await manager.sendPrompt(
                threadID: "thread-main",
                prompt: "ship it",
                assistantSurface: "codex"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try manager.localSnapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .sendSessionPrompt)
        #expect(snapshot.pendingCommands.first?.clientMutationId.hasPrefix("prompt-") == true)
        #expect(snapshot.pendingCommands.first?.prompt == "ship it")
        #expect(snapshot.pendingCommands.first?.assistantSurface == "codex")
        #expect(snapshot.pendingCommands.first?.attemptCount == 1)
        #expect(try manager.outboxDepth() == 1)
    }

    @Test
    func durableNotificationReplyRetryDedupesCommand() async throws {
        let manager = try LooperClientCoreSessionManager(
            filePath: localStorePath(named: "durable-notification-retry")
        )

        for _ in 0..<2 {
            do {
                _ = try await manager.submitNotificationReplyWithGeneratedMutation(
                    notificationID: "notification-main",
                    threadID: "thread-main",
                    prompt: "continue",
                    assistantSurface: "codex"
                )
                Issue.record("expected missing runtime config")
            } catch let error as ClientCoreError {
                #expect(error == .NoEndpoint)
            }
        }

        let snapshot = try manager.localSnapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .submitNotificationReply)
        #expect(snapshot.pendingCommands.first?.clientMutationId == "notification-reply:notification-main")
        #expect(snapshot.pendingCommands.first?.notificationId == "notification-main")
        #expect(snapshot.pendingCommands.first?.prompt == "continue")
        #expect(snapshot.pendingCommands.first?.attemptCount == 2)
        #expect(try manager.outboxDepth() == 1)
    }

    @Test
    func sessionManagerOwnsDurableCommandBoundary() async throws {
        let manager = try LooperClientCoreSessionManager(
            filePath: localStorePath(named: "session-manager-command-boundary")
        )

        do {
            _ = try await manager.sendPrompt(
                threadID: "thread-main",
                prompt: "continue",
                assistantSurface: "codex"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try manager.localSnapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .sendSessionPrompt)
        #expect(snapshot.pendingCommands.first?.clientMutationId.hasPrefix("prompt-") == true)
        #expect(try manager.outboxDepth() == 1)
    }

    @Test
    func sessionManagerQueuesModeIntoLocalProjectionBeforeTransport() async throws {
        let manager = try LooperClientCoreSessionManager(
            filePath: localStorePath(named: "session-manager-mode-local")
        )
        _ = try manager.replaceStateMinis(snapshot: ClientStateMiniSnapshot(
            latestSeq: 5,
            sessions: [
                ClientStateMini(
                    sessionId: "thread-main",
                    assistantSurface: "codex",
                    seq: 5,
                    revision: "revision-5",
                    payloadJson: #"{"sessionId":"thread-main","assistantSurface":"codex","effectiveMode":"await-reply"}"#
                ),
            ],
            serverTime: "2026-06-25T00:00:00Z"
        ))

        do {
            _ = try await manager.setMode(
                threadID: "thread-main",
                preset: "max-turns-2"
            )
            Issue.record("expected missing runtime config")
        } catch let error as ClientCoreError {
            #expect(error == .NoEndpoint)
        }

        let snapshot = try manager.localSnapshot()
        #expect(snapshot.pendingCommands.count == 1)
        #expect(snapshot.pendingCommands.first?.kind == .setSessionMode)
        #expect(snapshot.pendingCommands.first?.clientMutationId.hasPrefix("mode-") == true)
        #expect(snapshot.sessions.first?.payloadJson.contains(#""effectiveMode":"max-turns-2""#) == true)
        #expect(try manager.outboxDepth() == 1)
    }

    @Test
    func repeatedPromptIntentsUseDistinctRustGeneratedMutations() async throws {
        let manager = try LooperClientCoreSessionManager(
            filePath: localStorePath(named: "durable-prompt-distinct")
        )

        for prompt in ["first", "second"] {
            do {
                _ = try await manager.sendPrompt(
                    threadID: "thread-main",
                    prompt: prompt,
                    assistantSurface: "codex"
                )
                Issue.record("expected missing runtime config")
            } catch let error as ClientCoreError {
                #expect(error == .NoEndpoint)
            }
        }

        let snapshot = try manager.localSnapshot()
        #expect(snapshot.pendingCommands.count == 2)
        #expect(snapshot.pendingCommands.map(\.prompt) == ["first", "second"])
        #expect(snapshot.pendingCommands.allSatisfy { $0.clientMutationId.hasPrefix("prompt-") })
        #expect(Set(snapshot.pendingCommands.map(\.clientMutationId)).count == 2)
        #expect(snapshot.pendingCommands.map(\.attemptCount) == [1, 1])
        #expect(try manager.outboxDepth() == 2)
    }

    private func localStorePath(named name: String) -> String {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("LooperClientCoreTests-\(UUID().uuidString)", isDirectory: true)
            .appendingPathComponent("\(name)-state-minis.json")
            .path
    }
}

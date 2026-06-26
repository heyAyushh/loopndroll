import Foundation
import LooperClientCore
import LooperRealtime
import Testing

@testable import LooperMenuBarCore

@Suite("Menu bar SessionMini local-first")
struct MenuBarSessionMiniLocalFirstTests {
    @Test("restores cached SessionMini rows before network")
    func testRestoresCachedSessionMinisBeforeNetwork() throws {
        let store = try MenuBarSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
        try store.replace(latestSeq: 200, records: [
            miniRecord(
                id: "thread-active",
                title: "Ship local-first menu",
                mode: "await-reply",
                blockedGoalTitle: "Fix notification replies",
                queueCount: 2,
                lifecycle: "waiting",
                notificationTargetIds: ["macos"],
                lastActivityAtMs: 200
            ),
            miniRecord(
                id: "thread-archived",
                title: "Older task",
                mode: "max-turns-2",
                archived: true,
                notificationTargetIds: ["iphone"],
                lastActivityAtMs: 100
            ),
        ])
        let noNetworkClient = NoNetworkControlPlaneClient()

        let snapshot = try #require(try store.cachedSnapshot())
        let sections = LooperMenuContent.buildThreadSections(from: snapshot.sessions)

        #expect(noNetworkClient.snapshotCalls == 0)
        #expect(sections.map(\.title) == ["Active Chats", "Archived Chats"])
        let row = try #require(sections.first?.rows.first)
        #expect(row.threadId == "thread-active")
        #expect(row.title == "Ship local-first menu")
        #expect(row.effectiveMode == "await-reply")
        #expect(row.replyable == true)
        #expect(row.blockedGoalTitle == "Fix notification replies")
        #expect(row.queueCount == 2)
        #expect(row.lifecycle == "waiting")
        #expect(row.notificationTitle == "Notify macos")
        #expect(row.subtitle.contains("Await Reply"))
        #expect(row.subtitle.contains("Blocked: Fix notification replies"))
        #expect(row.subtitle.contains("Queue 2"))
    }

    @Test("malformed cache falls back and failed ACK stays in outbox")
    func testMalformedMiniCacheFallsBackAndOutboxKeepsFailedCommand() async throws {
        let malformedFileURL = temporaryStoreFileURL()
        let genericStore = try LooperClientCoreLocalStore(filePath: malformedFileURL.path)
        _ = try genericStore.replaceStateMinis(snapshot: ClientStateMiniSnapshot(
            latestSeq: 3,
            sessions: [
                ClientStateMini(
                    sessionId: "thread-main",
                    assistantSurface: "codex",
                    seq: 3,
                    revision: "rev-3",
                    payloadJson: #"{"id":"other-thread","sessionId":"other-thread","title":"bad"}"#
                )
            ],
            serverTime: ""
        ))
        let malformedStore = try MenuBarSessionMiniLocalStore(fileURL: malformedFileURL)
        let fallbackMiniSnapshot = try? malformedStore.cachedSnapshot()
        let fallbackSections = LooperMenuContent.buildThreadSections(from: [
            desktopThread(id: "thread-main", title: "Fallback snapshot")
        ])

        #expect(fallbackMiniSnapshot == nil)
        #expect(fallbackSections.first?.rows.first?.threadId == "thread-main")

        let outboxStore = try MenuBarSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
        let failingClient = RecordingMenuBarCommandClient(
            promptError: ControlPlaneClientError.timeout
        )
        let commandCenter = MenuBarSessionCommandCenter(
            client: failingClient,
            localStore: outboxStore
        )

        await expectThrows {
            _ = try await commandCenter.sendPrompt(
                threadID: "thread-main",
                prompt: "continue",
                assistantSurface: "codex",
                clientMutationID: "mutation-offline"
            )
        }
        await expectThrows {
            _ = try await commandCenter.sendPrompt(
                threadID: "thread-main",
                prompt: "continue",
                assistantSurface: "codex",
                clientMutationID: "mutation-offline"
            )
        }

        let pendingCommands = outboxStore.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.clientMutationID == "mutation-offline")
        #expect(pendingCommands.first?.threadID == "thread-main")
        #expect(pendingCommands.first?.attemptCount == 2)
        #expect(failingClient.promptRequests.map(\.clientMutationID) == [
            "mutation-offline",
            "mutation-offline",
        ])
    }

    @Test("optimistic menu actions use client mutation IDs without snapshot refresh")
    func testOptimisticMenuActionsUseClientMutationIDsWithoutSnapshotRefresh() async throws {
        let store = try MenuBarSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
        let client = RecordingMenuBarCommandClient()
        let commandCenter = MenuBarSessionCommandCenter(client: client, localStore: store)

        let mode = try await commandCenter.setSessionMode(
            threadID: "thread-main",
            preset: "await-reply",
            clientMutationID: "mutation-mode"
        )
        let prompt = try await commandCenter.sendPrompt(
            threadID: "thread-main",
            prompt: "ship it",
            assistantSurface: "codex",
            clientMutationID: "mutation-prompt"
        )

        #expect(mode.accepted)
        #expect(mode.delivered)
        #expect(mode.clientMutationID == "mutation-mode")
        #expect(prompt.accepted)
        #expect(prompt.delivered)
        #expect(prompt.clientMutationID == "mutation-prompt")
        #expect(client.modeRequests.map(\.clientMutationID) == ["mutation-mode"])
        #expect(client.promptRequests.map(\.clientMutationID) == ["mutation-prompt"])
        #expect(client.snapshotCalls == 0)
        #expect(store.pendingCommands().isEmpty)
    }

    @Test("notification replies use durable ACK commands without snapshot refresh")
    func testNotificationRepliesUseDurableAckCommandsWithoutSnapshotRefresh() async throws {
        let store = try MenuBarSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
        let client = RecordingMenuBarCommandClient()
        let commandCenter = MenuBarSessionCommandCenter(client: client, localStore: store)

        let result = try await commandCenter.submitNotificationReply(
            notificationID: "notif-main",
            threadID: "thread-main",
            prompt: "continue from notification",
            assistantSurface: nil,
            clientMutationID: "notification-reply:notif-main"
        )

        #expect(result.accepted)
        #expect(result.delivered)
        #expect(result.notificationID == "notif-main")
        #expect(result.clientMutationID == "notification-reply:notif-main")
        #expect(client.notificationReplyRequests.map(\.clientMutationID) == [
            "notification-reply:notif-main",
        ])
        #expect(client.notificationReplyRequests.map(\.notificationID) == ["notif-main"])
        #expect(client.promptRequests.isEmpty)
        #expect(client.snapshotCalls == 0)
        #expect(store.pendingCommands().isEmpty)
    }

    @Test("failed notification reply stays durable and dedupes retry")
    func testFailedNotificationReplyStaysDurableAndDedupesRetry() async throws {
        let store = try MenuBarSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
        let client = RecordingMenuBarCommandClient(
            promptError: ControlPlaneClientError.timeout
        )
        let commandCenter = MenuBarSessionCommandCenter(client: client, localStore: store)

        for _ in 0..<2 {
            await expectThrows {
                _ = try await commandCenter.submitNotificationReply(
                    notificationID: "notif-offline",
                    threadID: "thread-main",
                    prompt: "offline reply",
                    assistantSurface: nil,
                    clientMutationID: "notification-reply:notif-offline"
                )
            }
        }

        let pendingCommands = store.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.clientMutationID == "notification-reply:notif-offline")
        #expect(pendingCommands.first?.threadID == "thread-main")
        #expect(pendingCommands.first?.notificationID == "notif-offline")
        #expect(pendingCommands.first?.prompt == "offline reply")
        #expect(pendingCommands.first?.attemptCount == 2)
        #expect(client.notificationReplyRequests.map(\.clientMutationID) == [
            "notification-reply:notif-offline",
            "notification-reply:notif-offline",
        ])
    }

    private func temporaryStoreFileURL() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("LooperMenuBarTests-\(UUID().uuidString)", isDirectory: true)
            .appendingPathComponent(MenuBarSessionMiniLocalStore.defaultFileName)
    }

    private func miniRecord(
        id: String,
        title: String,
        mode: String,
        blockedGoalTitle: String? = nil,
        queueCount: Int = 0,
        lifecycle: String = "active",
        archived: Bool = false,
        notificationTargetIds: [String],
        lastActivityAtMs: Int64
    ) throws -> MenuBarSessionMiniRecord {
        let payload = TestMiniPayload(
            id: id,
            sessionId: id,
            ref: id,
            title: title,
            status: archived ? "archived" : "active",
            effectiveMode: mode,
            canSendPrompt: true,
            replyable: true,
            blockedGoal: blockedGoalTitle.map {
                TestBlockedGoal(
                    id: "goal-main",
                    title: $0,
                    status: "blocked",
                    lifecycle: "blocked",
                    reason: "blocked"
                )
            },
            queueCount: queueCount,
            lifecycle: lifecycle,
            notificationStatus: TestNotificationStatus(
                enabled: true,
                targetIds: notificationTargetIds,
                usesDefault: true
            ),
            isArchived: archived,
            assistantPreview: "Ready",
            metadata: TestMiniMetadata(
                projectName: "looper",
                projectPath: "/Users/test/looper"
            ),
            lastActivityAtMs: lastActivityAtMs,
            updatedAtMs: lastActivityAtMs
        )
        let data = try JSONEncoder().encode(payload)
        let payloadJSON = String(decoding: data, as: UTF8.self)
        return MenuBarSessionMiniRecord(
            sessionID: id,
            assistantSurface: "codex",
            seq: lastActivityAtMs,
            revision: "rev-\(lastActivityAtMs)",
            payloadJSON: payloadJSON
        )
    }

    private func desktopThread(id: String, title: String) -> DesktopThreadSummary {
        DesktopThreadSummary(
            threadId: id,
            title: title,
            cwd: "/tmp/looper",
            transcriptPath: nil,
            source: "desktop",
            model: nil,
            reasoningEffort: nil,
            updatedAtMs: nil,
            assistantPreview: nil,
            archived: false,
            capabilities: ThreadCapabilitiesSummary(
                threadId: id,
                mcpTools: [],
                appTools: [],
                automationTools: [],
                spawn: SpawnGraphSummary(
                    parentThreadId: nil,
                    rootThreadId: id,
                    children: [],
                    launchKind: "main"
                ),
                agentNickname: nil,
                agentRole: nil,
                agentPath: nil
            )
        )
    }

    private func expectThrows(_ operation: () async throws -> Void) async {
        do {
            try await operation()
            Issue.record("expected operation to throw")
        } catch {
        }
    }
}

private struct TestMiniPayload: Encodable {
    let id: String
    let sessionId: String
    let ref: String
    let title: String
    let status: String
    let effectiveMode: String
    let canSendPrompt: Bool
    let replyable: Bool
    let blockedGoal: TestBlockedGoal?
    let queueCount: Int
    let lifecycle: String
    let notificationStatus: TestNotificationStatus
    let isArchived: Bool
    let assistantPreview: String
    let metadata: TestMiniMetadata
    let lastActivityAtMs: Int64
    let updatedAtMs: Int64
}

private struct TestBlockedGoal: Encodable {
    let id: String
    let title: String
    let status: String
    let lifecycle: String
    let reason: String
}

private struct TestNotificationStatus: Encodable {
    let enabled: Bool
    let targetIds: [String]
    let usesDefault: Bool
}

private struct TestMiniMetadata: Encodable {
    let projectName: String
    let projectPath: String
}

private struct RecordedModeRequest: Sendable {
    let threadID: String
    let preset: String?
    let clientMutationID: String
}

private struct RecordedPromptRequest: Sendable {
    let threadID: String
    let prompt: String
    let assistantSurface: String?
    let clientMutationID: String
}

private struct RecordedNotificationReplyRequest: Sendable {
    let notificationID: String
    let threadID: String
    let prompt: String
    let assistantSurface: String?
    let clientMutationID: String
}

private final class RecordingMenuBarCommandClient: MenuBarSessionCommandClient, @unchecked Sendable {
    private let lock = NSLock()
    private let modeError: Error?
    private let promptError: Error?
    private var recordedModeRequests: [RecordedModeRequest] = []
    private var recordedPromptRequests: [RecordedPromptRequest] = []
    private var recordedNotificationReplyRequests: [RecordedNotificationReplyRequest] = []
    private var recordedSnapshotCalls = 0

    init(modeError: Error? = nil, promptError: Error? = nil) {
        self.modeError = modeError
        self.promptError = promptError
    }

    var modeRequests: [RecordedModeRequest] {
        lock.withLock { recordedModeRequests }
    }

    var promptRequests: [RecordedPromptRequest] {
        lock.withLock { recordedPromptRequests }
    }

    var notificationReplyRequests: [RecordedNotificationReplyRequest] {
        lock.withLock { recordedNotificationReplyRequests }
    }

    var snapshotCalls: Int {
        lock.withLock { recordedSnapshotCalls }
    }

    func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        let frames = try clientCore.takeExpectedOutbox(
            expectedClientMutationIds: expectedClientMutationIDs
        )
        var envelopes: [LooperRealtimeCommandAckEnvelope] = []
        for frame in frames {
            switch frame.commandKind {
            case .setSessionMode:
                lock.withLock {
                    recordedModeRequests.append(
                        RecordedModeRequest(
                            threadID: frame.threadId,
                            preset: frame.preset.nilIfBlank,
                            clientMutationID: frame.clientMutationId
                        )
                    )
                }
                if let modeError {
                    throw modeError
                }
                envelopes.append(
                    commandAckEnvelope(
                        commandKind: "SetSessionMode",
                        clientMutationID: frame.clientMutationId,
                        ackSeq: 10,
                        entityID: frame.threadId,
                        revision: "rev-mode",
                        preset: frame.preset.nilIfBlank,
                        dispatchKind: nil,
                        promptID: nil,
                        notificationID: nil
                    )
                )

            case .sendSessionPrompt:
                lock.withLock {
                    recordedPromptRequests.append(
                        RecordedPromptRequest(
                            threadID: frame.threadId,
                            prompt: frame.prompt,
                            assistantSurface: frame.assistantSurface.nilIfBlank,
                            clientMutationID: frame.clientMutationId
                        )
                    )
                }
                if let promptError {
                    throw promptError
                }
                envelopes.append(
                    commandAckEnvelope(
                        commandKind: "SendSessionPrompt",
                        clientMutationID: frame.clientMutationId,
                        ackSeq: 11,
                        entityID: frame.threadId,
                        revision: "rev-prompt",
                        preset: nil,
                        dispatchKind: "queued",
                        promptID: "prompt-main",
                        notificationID: nil
                    )
                )

            case .submitNotificationReply:
                lock.withLock {
                    recordedNotificationReplyRequests.append(
                        RecordedNotificationReplyRequest(
                            notificationID: frame.notificationId,
                            threadID: frame.threadId,
                            prompt: frame.prompt,
                            assistantSurface: frame.assistantSurface.nilIfBlank,
                            clientMutationID: frame.clientMutationId
                        )
                    )
                }
                if let promptError {
                    throw promptError
                }
                envelopes.append(
                    commandAckEnvelope(
                        commandKind: "SubmitNotificationReply",
                        clientMutationID: frame.clientMutationId,
                        ackSeq: 12,
                        entityID: frame.threadId,
                        revision: "rev-notification-reply",
                        preset: nil,
                        dispatchKind: "queued",
                        promptID: "prompt-main",
                        notificationID: frame.notificationId
                    )
                )

            case .resume:
                continue
            }
        }
        return LooperRealtimeSessionCommandBatchResponse(
            accepted: true,
            commandAcks: envelopes
        )
    }

    private func commandAckEnvelope(
        commandKind: String,
        clientMutationID: String,
        ackSeq: Int64,
        entityID: String,
        revision: String,
        preset: String?,
        dispatchKind: String?,
        promptID: String?,
        notificationID: String?
    ) -> LooperRealtimeCommandAckEnvelope {
        LooperRealtimeCommandAckEnvelope(
            commandKind: commandKind,
            ack: LooperRealtimeCommandAck(
                accepted: true,
                clientMutationID: clientMutationID,
                ackSeq: ackSeq,
                entityID: entityID,
                revision: revision,
                serverTime: "now",
                idempotentReplay: false
            ),
            preset: preset,
            dispatchKind: dispatchKind,
            promptID: promptID,
            notificationID: notificationID
        )
    }
}

private final class NoNetworkControlPlaneClient: @unchecked Sendable {
    private let lock = NSLock()
    private var recordedSnapshotCalls = 0

    var snapshotCalls: Int {
        lock.withLock { recordedSnapshotCalls }
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

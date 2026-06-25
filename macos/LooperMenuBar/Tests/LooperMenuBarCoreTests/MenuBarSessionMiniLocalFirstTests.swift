import Foundation
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
        let genericStore = try LooperRealtimeLocalStore(fileURL: malformedFileURL)
        try genericStore.replace(with: LooperRealtimeStateMiniSnapshot(
            latestSeq: 3,
            sessions: [
                LooperRealtimeStateMini(
                    sessionID: "thread-main",
                    assistantSurface: "codex",
                    seq: 3,
                    revision: "rev-3",
                    payloadJSON: #"{"id":"other-thread","sessionId":"other-thread","title":"bad"}"#
                )
            ],
            serverTime: nil
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

    @Test("mini sync recovers gap with snapshot and preserves outbox")
    func testMiniSyncRecoversGapWithSnapshotAndPreservesOutbox() async throws {
        let store = try MenuBarSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
        try store.replace(latestSeq: 5, records: [
            miniRecord(
                id: "thread-old",
                title: "Old local row",
                mode: "await-reply",
                notificationTargetIds: ["macos"],
                lastActivityAtMs: 5
            ),
        ])
        try store.enqueuePromptCommand(
            threadID: "thread-old",
            prompt: "continue",
            assistantSurface: "codex",
            clientMutationID: "mutation-pending"
        )

        let recoveredRecord = try miniRecord(
            id: "thread-recovered",
            title: "Recovered row",
            mode: "await-reply",
            notificationTargetIds: ["macos"],
            lastActivityAtMs: 6
        )
        let updatedRecord = try miniRecord(
            id: "thread-recovered",
            title: "Recovered row updated",
            mode: "max-turns-1",
            notificationTargetIds: ["macos"],
            lastActivityAtMs: 7
        )
        let transport = RecordingStateMiniSyncTransport(
            snapshots: [
                LooperRealtimeStateMiniSnapshot(
                    latestSeq: 6,
                    sessions: [stateMini(from: recoveredRecord)],
                    serverTime: nil
                ),
            ],
            streamPlans: [
                .recoveryRequired,
                .deltas([
                    stateMiniDelta(from: updatedRecord),
                ]),
            ]
        )
        let synchronizer = LooperRealtimeStateMiniSynchronizer(
            store: store.realtimeLocalStore,
            transport: transport,
            sleep: { _ in }
        )

        let recoveryResult = await synchronizer.runOneCycle { _ in }
        let resumedResult = await synchronizer.runOneCycle { _ in }
        let snapshot = try #require(try store.cachedSnapshot())

        #expect(recoveryResult == .recovered(latestSeq: 6))
        #expect(resumedResult == .streamEnded(latestSeq: 7))
        #expect(await transport.observedAfterSeqs() == [5, 6])
        #expect(await transport.snapshotRequestCount() == 1)
        #expect(snapshot.sessions.map(\.sessionID) == ["thread-recovered"])
        #expect(snapshot.sessions.first?.effectiveMode == "max-turns-1")
        #expect(store.pendingCommands().map(\.clientMutationID) == ["mutation-pending"])
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

    private func stateMini(from record: MenuBarSessionMiniRecord) -> LooperRealtimeStateMini {
        LooperRealtimeStateMini(
            sessionID: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJSON: record.payloadJSON
        )
    }

    private func stateMiniDelta(from record: MenuBarSessionMiniRecord) -> LooperRealtimeStateMiniDelta {
        let mini = stateMini(from: record)
        return LooperRealtimeStateMiniDelta(
            seq: record.seq,
            latestSeq: record.seq,
            entityID: "session-mini:\(record.assistantSurface):\(record.sessionID)",
            kind: "session-mini.changed",
            revision: record.revision,
            serverTime: nil,
            session: mini,
            sessionID: record.sessionID,
            assistantSurface: record.assistantSurface,
            sessions: [mini]
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

    func submitSessionCommandBatch(
        commands: [LooperRealtimeSessionCommand]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        var envelopes: [LooperRealtimeCommandAckEnvelope] = []
        for command in commands {
            switch command {
            case let .setSessionMode(threadID, preset, clientMutationID):
                lock.withLock {
                    recordedModeRequests.append(
                        RecordedModeRequest(
                            threadID: threadID,
                            preset: preset,
                            clientMutationID: clientMutationID
                        )
                    )
                }
                if let modeError {
                    throw modeError
                }
                envelopes.append(
                    commandAckEnvelope(
                        commandKind: command.commandKind,
                        clientMutationID: clientMutationID,
                        ackSeq: 10,
                        entityID: threadID,
                        revision: "rev-mode",
                        preset: preset,
                        dispatchKind: nil,
                        promptID: nil,
                        notificationID: nil
                    )
                )

            case let .sendSessionPrompt(threadID, prompt, assistantSurface, clientMutationID):
                lock.withLock {
                    recordedPromptRequests.append(
                        RecordedPromptRequest(
                            threadID: threadID,
                            prompt: prompt,
                            assistantSurface: assistantSurface,
                            clientMutationID: clientMutationID
                        )
                    )
                }
                if let promptError {
                    throw promptError
                }
                envelopes.append(
                    commandAckEnvelope(
                        commandKind: command.commandKind,
                        clientMutationID: clientMutationID,
                        ackSeq: 11,
                        entityID: threadID,
                        revision: "rev-prompt",
                        preset: nil,
                        dispatchKind: "queued",
                        promptID: "prompt-main",
                        notificationID: nil
                    )
                )

            case let .submitNotificationReply(
                notificationID,
                threadID,
                prompt,
                assistantSurface,
                clientMutationID
            ):
                lock.withLock {
                    recordedNotificationReplyRequests.append(
                        RecordedNotificationReplyRequest(
                            notificationID: notificationID,
                            threadID: threadID,
                            prompt: prompt,
                            assistantSurface: assistantSurface,
                            clientMutationID: clientMutationID
                        )
                    )
                }
                if let promptError {
                    throw promptError
                }
                envelopes.append(
                    commandAckEnvelope(
                        commandKind: command.commandKind,
                        clientMutationID: clientMutationID,
                        ackSeq: 12,
                        entityID: threadID,
                        revision: "rev-notification-reply",
                        preset: nil,
                        dispatchKind: "queued",
                        promptID: "prompt-main",
                        notificationID: notificationID
                    )
                )
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

private actor RecordingStateMiniSyncTransport: LooperRealtimeStateMiniSyncTransport {
    enum StreamPlan: Sendable {
        case deltas([LooperRealtimeStateMiniDelta])
        case recoveryRequired
    }

    private var snapshots: [LooperRealtimeStateMiniSnapshot]
    private var streamPlans: [StreamPlan]
    private var afterSeqs: [Int64] = []
    private var snapshotRequests = 0

    init(
        snapshots: [LooperRealtimeStateMiniSnapshot],
        streamPlans: [StreamPlan]
    ) {
        self.snapshots = snapshots
        self.streamPlans = streamPlans
    }

    func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot {
        snapshotRequests += 1
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

    func snapshotRequestCount() -> Int {
        snapshotRequests
    }
}

private struct RecordingRecoveryRequiredError: LocalizedError, Sendable {
    var errorDescription: String? {
        "state mini recovery required: requested_after_seq=99 latest_seq=6"
    }
}

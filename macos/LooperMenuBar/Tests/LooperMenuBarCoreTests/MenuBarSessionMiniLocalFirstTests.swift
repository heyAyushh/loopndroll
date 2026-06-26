import Foundation
import LooperClientCore
import Testing

@testable import LooperMenuBarCore

@Suite("Menu bar SessionMini local-first")
struct MenuBarSessionMiniLocalFirstTests {
    @Test("restores cached SessionMini rows before network")
    func testRestoresCachedSessionMinisBeforeNetwork() throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        try runtime.replace(latestSeq: 200, records: [
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

        let snapshot = try #require(try runtime.cachedSnapshot())
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

    @Test("SessionMini status drives menu bar human status")
    func testSessionMiniStatusDrivesHumanStatus() throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        try runtime.replace(latestSeq: 201, records: [
            miniRecord(
                id: "thread-blocked",
                title: "Blocked task",
                mode: "await-reply",
                blockedGoalTitle: "Need user input",
                notificationTargetIds: ["macos"],
                lastActivityAtMs: 201
            ),
        ])

        let snapshot = try #require(try runtime.cachedSnapshot())
        let status = LooperHumanStatus.from(
            sessionMiniSnapshot: snapshot,
            mobileHealth: nil,
            detachOnQuit: true
        )

        #expect(status.kind == .needsAttention)
        #expect(status.title == "Needs attention")
        #expect(status.lifecycle == "Detached on quit")
        #expect(status.detail.contains("source=sessionMini"))
        #expect(status.detail.contains("blocked=1"))
    }

    @Test("SessionMini status reports realtime when unblocked")
    func testSessionMiniStatusReportsRealtimeWhenUnblocked() throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        try runtime.replace(latestSeq: 202, records: [
            miniRecord(
                id: "thread-ready",
                title: "Ready task",
                mode: "max-turns-2",
                notificationTargetIds: ["macos"],
                lastActivityAtMs: 202
            ),
        ])

        let snapshot = try #require(try runtime.cachedSnapshot())
        let status = LooperHumanStatus.from(
            sessionMiniSnapshot: snapshot,
            mobileHealth: MobileHealthResponse(
                ok: true,
                baseURL: "http://100.119.200.69:8765",
                baseURLs: ["http://100.119.200.69:8765"],
                requiresAuthentication: true
            ),
            detachOnQuit: false
        )

        #expect(status.kind == .ready)
        #expect(status.title == "Realtime")
        #expect(status.detail.contains("replyable=1"))
        #expect(status.detail.contains("iPhone=ready"))
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
        let malformedRuntime = try MenuBarSessionRuntime(fileURL: malformedFileURL)
        let fallbackMiniSnapshot = try? malformedRuntime.cachedSnapshot()
        let fallbackSections = LooperMenuContent.buildThreadSections(from: [
            desktopThread(id: "thread-main", title: "Fallback snapshot")
        ])

        #expect(fallbackMiniSnapshot == nil)
        #expect(fallbackSections.first?.rows.first?.threadId == "thread-main")

        let outboxRuntime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(
            sessionRuntime: outboxRuntime
        )

        await expectThrows {
            _ = try await commandCenter.sendPrompt(
                threadID: "thread-main",
                prompt: "continue",
                assistantSurface: "codex"
            )
        }
        await expectThrows {
            _ = try await commandCenter.sendPrompt(
                threadID: "thread-main",
                prompt: "continue",
                assistantSurface: "codex"
            )
        }

        let pendingCommands = outboxRuntime.pendingCommands()
        #expect(pendingCommands.count == 2)
        #expect(pendingCommands.allSatisfy { $0.clientMutationID.hasPrefix("prompt-") })
        #expect(pendingCommands.map(\.threadID) == ["thread-main", "thread-main"])
        #expect(pendingCommands.map(\.attemptCount) == [1, 1])
    }

    @Test("menu actions enqueue durable Rust-core commands before transport")
    func testMenuActionsEnqueueDurableRustCoreCommandsBeforeTransport() async throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(sessionRuntime: runtime)

        await expectThrows {
            _ = try await commandCenter.setSessionMode(
                threadID: "thread-main",
                preset: "await-reply"
            )
        }
        await expectThrows {
            _ = try await commandCenter.sendPrompt(
                threadID: "thread-main",
                prompt: "ship it",
                assistantSurface: "codex"
            )
        }

        let pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.map(\.kind) == [.setSessionMode, .sendSessionPrompt])
        #expect(pendingCommands[0].clientMutationID.hasPrefix("mode-"))
        #expect(pendingCommands[1].clientMutationID.hasPrefix("prompt-"))
        #expect(pendingCommands.map(\.threadID) == ["thread-main", "thread-main"])
        #expect(pendingCommands.map(\.attemptCount) == [1, 1])
    }

    @Test("menu actions can use Rust-core generated mutation IDs")
    func testMenuActionsUseRustCoreGeneratedMutationIDs() async throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(sessionRuntime: runtime)

        await expectThrows {
            _ = try await commandCenter.setSessionMode(
                threadID: "thread-main",
                preset: "await-reply"
            )
        }
        await expectThrows {
            _ = try await commandCenter.sendPrompt(
                threadID: "thread-main",
                prompt: "ship it",
                assistantSurface: "codex"
            )
        }

        let pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.map(\.kind) == [.setSessionMode, .sendSessionPrompt])
        #expect(pendingCommands[0].clientMutationID.hasPrefix("mode-"))
        #expect(pendingCommands[1].clientMutationID.hasPrefix("prompt-"))
    }

    @Test("notification replies enter durable Rust-core outbox before transport")
    func testNotificationRepliesEnterDurableRustCoreOutboxBeforeTransport() async throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(sessionRuntime: runtime)

        await expectThrows {
            _ = try await commandCenter.submitNotificationReply(
                notificationID: "notif-main",
                threadID: "thread-main",
                prompt: "continue from notification",
                assistantSurface: nil,
                clientMutationID: "notification-reply:notif-main"
            )
        }

        let pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.notificationID == "notif-main")
        #expect(pendingCommands.first?.prompt == "continue from notification")
        #expect(pendingCommands.first?.attemptCount == 1)
    }

    @Test("failed notification reply stays durable and dedupes retry")
    func testFailedNotificationReplyStaysDurableAndDedupesRetry() async throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(sessionRuntime: runtime)

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

        let pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.clientMutationID == "notification-reply:notif-offline")
        #expect(pendingCommands.first?.threadID == "thread-main")
        #expect(pendingCommands.first?.notificationID == "notif-offline")
        #expect(pendingCommands.first?.prompt == "offline reply")
        #expect(pendingCommands.first?.attemptCount == 2)
    }

    private func temporaryStoreFileURL() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("LooperMenuBarTests-\(UUID().uuidString)", isDirectory: true)
            .appendingPathComponent(MenuBarSessionRuntime.defaultFileName)
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

private final class NoNetworkControlPlaneClient: @unchecked Sendable {
    private let lock = NSLock()
    private var recordedSnapshotCalls = 0

    var snapshotCalls: Int {
        lock.withLock { recordedSnapshotCalls }
    }
}

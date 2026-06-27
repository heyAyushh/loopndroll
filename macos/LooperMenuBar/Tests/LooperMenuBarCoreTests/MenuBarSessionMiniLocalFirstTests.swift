import Foundation
import LooperClientCore
import Testing

@testable import LooperMenuBarCore

@Suite("Menu bar SessionMini local-first")
struct MenuBarSessionMiniLocalFirstTests {
    @Test("restores cached SessionMini rows before network")
    func testRestoresCachedSessionMinisBeforeNetwork() throws {
        let runtime = try seededRuntime(latestSeq: 200, records: [
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
        let runtime = try seededRuntime(latestSeq: 201, records: [
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
        let runtime = try seededRuntime(latestSeq: 202, records: [
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

    @Test("malformed cache falls back and offline prompt stays in outbox")
    func testMalformedMiniCacheFallsBackAndOutboxKeepsOfflinePrompt() async throws {
        let malformedFileURL = temporaryStoreFileURL()
        try seedMalformedMiniCache(at: malformedFileURL)
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

        let firstPrompt = try await commandCenter.sendPrompt(
            threadID: "thread-main",
            prompt: "continue",
            assistantSurface: "codex"
        )
        let secondPrompt = try await commandCenter.sendPrompt(
            threadID: "thread-main",
            prompt: "continue",
            assistantSurface: "codex"
        )
        #expect(firstPrompt.accepted)
        #expect(secondPrompt.accepted)

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

        let modeResult = try await commandCenter.setSessionMode(
            threadID: "thread-main",
            preset: "await-reply"
        )
        let promptResult = try await commandCenter.sendPrompt(
            threadID: "thread-main",
            prompt: "ship it",
            assistantSurface: "codex"
        )
        #expect(modeResult.accepted)
        #expect(promptResult.accepted)

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

        let modeResult = try await commandCenter.setSessionMode(
            threadID: "thread-main",
            preset: "await-reply"
        )
        let promptResult = try await commandCenter.sendPrompt(
            threadID: "thread-main",
            prompt: "ship it",
            assistantSurface: "codex"
        )
        #expect(modeResult.accepted)
        #expect(promptResult.accepted)

        let pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.map(\.kind) == [.setSessionMode, .sendSessionPrompt])
        #expect(pendingCommands[0].clientMutationID.hasPrefix("mode-"))
        #expect(pendingCommands[1].clientMutationID.hasPrefix("prompt-"))
    }

    @Test("notification replies enter durable Rust-core outbox before transport")
    func testNotificationRepliesEnterDurableRustCoreOutboxBeforeTransport() async throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(sessionRuntime: runtime)

        let result = try await commandCenter.submitNotificationReply(
            notificationID: "notif-main",
            threadID: "thread-main",
            prompt: "continue from notification",
            assistantSurface: nil,
            clientMutationID: "notification-reply:notif-main"
        )
        #expect(result.accepted)

        let pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.notificationID == "notif-main")
        #expect(pendingCommands.first?.prompt == "continue from notification")
        #expect(pendingCommands.first?.attemptCount == 1)
    }

    @Test("realtime endpoint resolver seeds Session without mobile health")
    func testRealtimeEndpointResolverSeedsSessionWithoutMobileHealth() throws {
        let endpoints = MenuBarRealtimeEndpointResolver.endpoints(
            controlPlaneBaseURL: try #require(URL(string: "http://127.0.0.1:8765")),
            health: nil,
            preference: .tailscale
        )

        #expect(endpoints.map(\.absoluteString) == ["http://127.0.0.1:8766"])
    }

    @Test("realtime endpoint resolver ranks health routes and local fallback")
    func testRealtimeEndpointResolverRanksHealthRoutesAndLocalFallback() throws {
        let endpoints = MenuBarRealtimeEndpointResolver.endpoints(
            controlPlaneBaseURL: try #require(URL(string: "http://127.0.0.1:8765")),
            health: MobileHealthResponse(
                ok: true,
                baseURL: "http://192.168.1.33:8765",
                baseURLs: ["http://192.168.1.33:8765"],
                grpcBaseURL: "http://100.95.2.4:8766",
                grpcBaseURLs: [
                    "http://100.95.2.4:8766",
                    "http://192.168.1.33:8766",
                ],
                requiresAuthentication: true
            ),
            preference: .lan
        )

        #expect(endpoints.map(\.absoluteString) == [
            "http://192.168.1.33:8766",
            "http://100.95.2.4:8766",
            "http://127.0.0.1:8766",
        ])
    }

    @Test("offline notification reply stays durable and dedupes retry")
    func testOfflineNotificationReplyStaysDurableAndDedupesRetry() async throws {
        let runtime = try MenuBarSessionRuntime(fileURL: temporaryStoreFileURL())
        let commandCenter = MenuBarSessionCommandCenter(sessionRuntime: runtime)

        for _ in 0..<2 {
            let result = try await commandCenter.submitNotificationReply(
                notificationID: "notif-offline",
                threadID: "thread-main",
                prompt: "offline reply",
                assistantSurface: nil,
                clientMutationID: "notification-reply:notif-offline"
            )
            #expect(result.accepted)
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

    private func seededRuntime(
        latestSeq: Int64,
        records: [TestMenuMiniFixture]
    ) throws -> MenuBarSessionRuntime {
        let fileURL = temporaryStoreFileURL()
        try seedMiniCache(at: fileURL, latestSeq: latestSeq, records: records)
        return try MenuBarSessionRuntime(fileURL: fileURL)
    }

    private func seedMiniCache(
        at fileURL: URL,
        latestSeq: Int64,
        records: [TestMenuMiniFixture]
    ) throws {
        let payload: [String: Any] = [
            "latestSeq": latestSeq,
            "sessions": records.map { record in
                [
                    "sessionId": record.sessionID,
                    "assistantSurface": record.assistantSurface,
                    "seq": record.seq,
                    "revision": record.revision,
                    "payloadJson": record.payloadJSON,
                ]
            },
            "pendingCommands": [],
            "serverTime": "",
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try data.write(to: fileURL, options: .atomic)
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
    ) throws -> TestMenuMiniFixture {
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
        return TestMenuMiniFixture(
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

    private func seedMalformedMiniCache(at fileURL: URL) throws {
        let payload: [String: Any] = [
            "latestSeq": 3,
            "sessions": [
                [
                    "sessionId": "thread-main",
                    "assistantSurface": "codex",
                    "seq": 3,
                    "revision": "rev-3",
                    "payloadJson":
                        #"{"id":"other-thread","sessionId":"other-thread","title":"bad"}"#,
                ],
            ],
            "pendingCommands": [],
            "serverTime": "",
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        try FileManager.default.createDirectory(
            at: fileURL.deletingLastPathComponent(),
            withIntermediateDirectories: true
        )
        try data.write(to: fileURL, options: .atomic)
    }
}

private struct TestMenuMiniFixture: Equatable, Sendable {
    let sessionID: String
    let assistantSurface: String
    let seq: Int64
    let revision: String
    let payloadJSON: String
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

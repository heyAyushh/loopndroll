import Foundation
import Testing
@testable import LooperMenuBarCore

struct LooperContinuationActivityTests {
    @Test
    func usesNewestActiveThreadAsContinuationTarget() throws {
        let snapshot = desktopSnapshot(threads: [
            thread(id: "old", title: "Old", updatedAtMs: 1),
            thread(id: "archived-new", title: "Archived", updatedAtMs: 3, archived: true),
            thread(
                id: "new",
                title: "Fresh",
                updatedAtMs: 2,
                assistantPreview: "Current assistant text"
            ),
        ])

        let descriptor = LooperContinuationActivityBuilder.descriptor(from: snapshot)

        #expect(descriptor.title == "Fresh")
        #expect(descriptor.targetContentIdentifier == "looper.session.new")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "new")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview] == "Current assistant text")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.updatedAtMilliseconds] == "2")
    }

    @Test
    func carriesDebugHandoffURLWithoutWebFallback() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: desktopSnapshot(threads: [
                thread(id: "thread-main", title: "Main", updatedAtMs: 1),
            ]),
            handoffBaseURL: URL(string: "http://192.168.1.4:8765")
        )

        #expect(
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.handoffWebpageURL]
                == "http://192.168.1.4:8765/handoff/sessions/thread-main"
        )
    }

    @Test
    func encodesSlashSeparatedSessionIDsInHandoffURL() throws {
        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: desktopSnapshot(threads: [
                thread(id: "acp/devin-cli/brindle-cadet", title: "Devin", updatedAtMs: 1),
            ]),
            handoffBaseURL: URL(string: "http://192.168.1.4:8765")
        )

        #expect(
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.handoffWebpageURL]
                == "http://192.168.1.4:8765/handoff/sessions/acp%2Fdevin-cli%2Fbrindle-cadet"
        )
    }

    @Test
    func fallsBackToGenericActivityWhenNoThreadsExist() {
        let descriptor = LooperContinuationActivityBuilder.descriptor(from: desktopSnapshot(threads: []))

        #expect(descriptor.title == "looper")
        #expect(descriptor.targetContentIdentifier == "looper")
        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.kind] == "app")
    }

    @Test
    func fallsBackToArchivedThreadWhenItIsOnlyVisibleTarget() {
        let descriptor = LooperContinuationActivityBuilder.descriptor(
            from: desktopSnapshot(threads: [
                thread(id: "archived", title: "Only visible", updatedAtMs: 7, archived: true),
            ])
        )

        #expect(descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] == "archived")
    }

    private func desktopSnapshot(threads: [DesktopThreadSummary]) -> DesktopSnapshotResponse {
        DesktopSnapshotResponse(
            controlPlane: ControlPlaneStatusResponse(
                hooks: HookStatusSummary(
                    enabled: true,
                    registeredEvents: [],
                    activeCommand: nil,
                    owner: "rust",
                    health: "healthy",
                    issues: [],
                    recentFailuresCount: 0
                ),
                codexServers: [],
                source: SourceStatusSummary(
                    codexHome: "/tmp/codex",
                    stateDb: nil,
                    logsDb: nil,
                    sessionsRoot: "/tmp/sessions",
                    health: "healthy",
                    degradedReason: nil
                )
            ),
            devinDesktop: DevinDesktopStatus(
                acpBridge: DevinAcpBridgeStatus(
                    available: false,
                    controlLevel: "visibility-only",
                    summary: "Unavailable",
                    actions: [],
                    agents: []
                )
            ),
            threadCount: threads.count,
            activeThreadCount: threads.filter { !$0.archived }.count,
            archivedThreadCount: threads.filter(\.archived).count,
            threads: threads,
            automations: [],
            goals: [],
            compactions: [],
            assistantAdapters: []
        )
    }

    private func thread(
        id: String,
        title: String?,
        updatedAtMs: Int64?,
        assistantPreview: String? = nil,
        archived: Bool = false
    ) -> DesktopThreadSummary {
        DesktopThreadSummary(
            threadId: id,
            title: title,
            cwd: "/tmp/looper",
            transcriptPath: nil,
            source: "desktop",
            model: nil,
            reasoningEffort: nil,
            updatedAtMs: updatedAtMs,
            assistantPreview: assistantPreview,
            archived: archived,
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
}

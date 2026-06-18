import Foundation
import Testing
@testable import LooperMenuBarCore

struct LooperMenuContentTests {
    @Test
    func groupsActiveAndArchivedThreads() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "active-1", title: "Current task"),
            thread(id: "archived-1", title: "Old task", archived: true),
            thread(id: "active-2", title: "Another task"),
        ])

        #expect(sections.map(\.title) == ["Active Chats", "Archived Chats"])
        #expect(sections[0].rows.map(\.threadId) == ["active-1", "active-2"])
        #expect(sections[1].rows.map(\.threadId) == ["archived-1"])
    }

    @Test
    func usesNearestGitProjectNameForThreadSubtitle() throws {
        let projectURL = try GitProjectFixture.makeProject(name: "looper")
        let nestedWorkingDirectory = projectURL
            .appendingPathComponent("macos", isDirectory: true)
            .appendingPathComponent("Sources", isDirectory: true)
        try FileManager.default.createDirectory(at: nestedWorkingDirectory, withIntermediateDirectories: true)

        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "thread-main", title: "Build native menu", cwd: nestedWorkingDirectory.path),
        ])

        let row = try #require(sections.first?.rows.first)
        #expect(row.subtitle == "looper")
    }

    @Test
    func marksArchivedRowsInSubtitle() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "archived-1", title: "Old task", cwd: "/tmp/archive", archived: true),
        ])

        #expect(sections.first?.rows.first?.subtitle == "Archived - archive")
    }

    @Test
    func includesZedACPSourceLabelWhenThreadMentionsZedACP() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "zed-1", title: "add Zed acp as acp targets", cwd: "/tmp/looper"),
        ])

        #expect(sections.first?.rows.first?.subtitle == "Zed ACP - looper")
    }

    @Test
    func includesTailscaleSourceLabelWhenThreadMentionsTailscale() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "tailscale-1", title: "fix Tailscale mobile route", cwd: "/tmp/looper"),
        ])

        #expect(sections.first?.rows.first?.subtitle == "Tailscale - looper")
    }

    @Test
    func includesTailscaleSourceLabelForMagicDNSHost() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "tailscale-2", title: "connect ayush-mac.tail62d9a8.ts.net", cwd: "/tmp/looper"),
        ])

        #expect(sections.first?.rows.first?.subtitle == "Tailscale - looper")
    }

    @Test
    func includesTailscaleSourceLabelForTailscaleIPv4Address() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "tailscale-3", title: "open http://100.95.2.4:8765", cwd: "/tmp/looper"),
        ])

        #expect(sections.first?.rows.first?.subtitle == "Tailscale - looper")
    }

    @Test
    func keepsMultipleSurfaceLabelsWhenThreadMentionsBoth() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "combo-1", title: "Zed ACP through 100.95.2.4", cwd: "/tmp/looper"),
        ])

        #expect(sections.first?.rows.first?.subtitle == "Zed ACP - Tailscale - looper")
    }

    @Test
    func fallsBackToThreadIdWhenTitleIsBlank() {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(id: "thread-main", title: "   "),
        ])

        #expect(sections.first?.rows.first?.title == "thread-main")
    }

    @Test
    func carriesThreadOpenTarget() throws {
        let sections = LooperMenuContent.buildThreadSections(from: [
            thread(
                id: "thread-main",
                title: "Open source",
                cwd: "/Users/test/project",
                transcriptPath: "/Users/test/.codex/sessions/thread-main.jsonl"
            ),
        ])

        let target = try #require(sections.first?.rows.first?.openTarget)
        #expect(target.threadId == "thread-main")
        #expect(target.codexURL?.absoluteString == "codex://threads/thread-main")
        #expect(target.transcriptURL?.path == "/Users/test/.codex/sessions/thread-main.jsonl")
        #expect(target.projectURL?.path == "/Users/test/project")
    }

    @Test
    func summarizesAcpTargetReadiness() {
        #expect(LooperMenuContent.acpTargetStatusTitle(from: []) == "None")
        #expect(LooperMenuContent.acpTargetStatusTitle(from: [
            acpTarget(id: "devin:looper", clientName: "Devin Desktop", ready: true),
        ]) == "1 ready")
        #expect(LooperMenuContent.acpTargetStatusTitle(from: [
            acpTarget(id: "zed:looper", ready: false, status: "read-only"),
        ]) == "1 read-only")
        #expect(LooperMenuContent.acpTargetStatusTitle(from: [
            acpTarget(id: "zed:looper", ready: false, status: "read-only"),
            acpTarget(id: "devin:codex", clientName: "Devin Desktop", ready: false),
        ]) == "1 read-only / 1 blocked")
    }

    @Test
    func buildsAcpTargetRowsWithoutLaunchValues() throws {
        let rows = LooperMenuContent.buildAcpTargetRows(from: [
            acpTarget(
                id: "devin:looper",
                clientName: "Devin Desktop",
                agentId: "looper",
                name: "Looper",
                methods: ["command"],
                ready: true,
                detail: "Devin Desktop ACP agent is enabled with sanitized launch metadata."
            ),
            acpTarget(
                id: "zed:readonly",
                clientName: "Zed",
                agentId: "readonly",
                name: "readonly",
                methods: ["command"],
                ready: false,
                status: "read-only",
                detail: "Zed External Agent target is configured; Looper reports it read-only."
            ),
            acpTarget(
                id: "zed:configured-only",
                clientName: "Zed",
                agentId: "configured-only",
                name: "   ",
                methods: [],
                ready: false,
                detail: "Zed External Agent target lacks command or transport metadata."
            ),
        ])

        #expect(rows[0].title == "Devin Desktop: Looper")
        #expect(rows[0].subtitle == "Ready - command")
        #expect(rows[0].detail == "Devin Desktop ACP agent is enabled with sanitized launch metadata.")
        #expect(rows[1].title == "Zed: readonly")
        #expect(rows[1].subtitle == "Read-only - command")
        #expect(rows[2].title == "Zed: configured-only")
        #expect(rows[2].subtitle == "Blocked - no launch metadata")
    }

    private func thread(
        id: String,
        title: String?,
        cwd: String = "/tmp/default",
        transcriptPath: String? = nil,
        archived: Bool = false
    ) -> DesktopThreadSummary {
        DesktopThreadSummary(
            threadId: id,
            title: title,
            cwd: cwd,
            transcriptPath: transcriptPath,
            source: "desktop",
            model: nil,
            reasoningEffort: nil,
            updatedAtMs: nil,
            assistantPreview: nil,
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

    private func acpTarget(
        id: String,
        clientName: String = "Zed",
        agentId: String = "looper",
        name: String = "looper",
        methods: [String] = ["command"],
        ready: Bool,
        status: String? = nil,
        detail: String = "Configured"
    ) -> AcpTargetSummary {
        AcpTargetSummary(
            id: id,
            client: clientName.lowercased(),
            clientName: clientName,
            agentId: agentId,
            name: name,
            source: "zed-agent-servers",
            sourcePath: "/Users/test/.zed/settings.json",
            enabled: true,
            preferred: false,
            launchConfigured: !methods.isEmpty,
            launch: AcpLaunchMetadataSummary(
                configured: !methods.isEmpty,
                methods: methods
            ),
            ready: ready,
            status: status ?? (ready ? "ready" : "blocked"),
            detail: detail
        )
    }
}

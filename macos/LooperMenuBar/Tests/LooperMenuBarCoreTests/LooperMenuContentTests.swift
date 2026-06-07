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
}

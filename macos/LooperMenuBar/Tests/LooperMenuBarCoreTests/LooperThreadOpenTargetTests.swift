import Foundation
import Testing
@testable import LooperMenuBarCore

struct LooperThreadOpenTargetTests {
    @Test
    func buildsCodexThreadURLAndLocalFallbacks() throws {
        let target = LooperThreadOpenTarget(
            threadId: "thread-main",
            transcriptPath: "/Users/test/.codex/sessions/thread-main.jsonl",
            workingDirectory: "/Users/test/project"
        )

        #expect(target.codexURL?.absoluteString == "codex://threads/thread-main")
        #expect(target.transcriptURL?.path == "/Users/test/.codex/sessions/thread-main.jsonl")
        #expect(target.projectURL?.path == "/Users/test/project")
        #expect(target.firstLocalFallbackURL == target.transcriptURL)
    }

    @Test
    func trimsPathsAndEncodesThreadId() {
        let target = LooperThreadOpenTarget(
            threadId: "thread with spaces",
            transcriptPath: "  /Users/test/.codex/sessions/thread.jsonl  ",
            workingDirectory: "  "
        )

        #expect(target.codexURL?.absoluteString == "codex://threads/thread%20with%20spaces")
        #expect(target.transcriptURL?.path == "/Users/test/.codex/sessions/thread.jsonl")
        #expect(target.projectURL == nil)
    }

    @Test
    func usesProjectAsFallbackWhenTranscriptIsUnavailable() {
        let target = LooperThreadOpenTarget(
            threadId: "thread-main",
            transcriptPath: nil,
            workingDirectory: "/Users/test/project"
        )

        #expect(target.firstLocalFallbackURL == target.projectURL)
    }

    @Test
    func skipsCodexDeepLinkForGrokBuildSessions() {
        let target = LooperThreadOpenTarget(
            threadId: "thread-grok",
            transcriptPath: "/Users/test/.grok/sessions/thread-grok/updates.jsonl",
            workingDirectory: "/Users/test/project"
        )

        #expect(target.codexURL == nil)
        #expect(target.transcriptURL?.path == "/Users/test/.grok/sessions/thread-grok/updates.jsonl")
        #expect(target.firstLocalFallbackURL == target.transcriptURL)
    }

    @Test
    func skipsCodexDeepLinkForGrokWorktreeAndAgentPath() {
        let worktreeTarget = LooperThreadOpenTarget(
            threadId: "thread-worktree",
            transcriptPath: nil,
            workingDirectory: "/Users/test/.grok/worktrees/documents-looper/sse"
        )
        let agentTarget = LooperThreadOpenTarget(
            threadId: "thread-agent",
            transcriptPath: nil,
            workingDirectory: "/Users/test/project",
            agentPath: "/Users/test/.grok/bin/grok"
        )

        #expect(worktreeTarget.codexURL == nil)
        #expect(agentTarget.codexURL == nil)
        #expect(agentTarget.firstLocalFallbackURL == agentTarget.projectURL)
    }

    @Test
    func skipsCodexDeepLinkForDevinSessions() {
        let target = LooperThreadOpenTarget(
            threadId: "devin:devin-cli:brindle-cadet",
            transcriptPath: "/Users/test/Library/Application Support/Devin - Next/User/acp-events/event.ndjson",
            workingDirectory: "/Users/test/project"
        )

        #expect(target.codexURL == nil)
        #expect(target.transcriptURL?.path == "/Users/test/Library/Application Support/Devin - Next/User/acp-events/event.ndjson")
        #expect(target.firstLocalFallbackURL == target.transcriptURL)
    }

    @Test
    func skipsCodexDeepLinkForReverseHandoffNonCodexSurface() {
        let target = LooperThreadOpenTarget(
            threadId: "thread-main",
            transcriptPath: nil,
            workingDirectory: "/Users/test/project",
            assistantSurface: "claude-code"
        )

        #expect(target.codexURL == nil)
        #expect(target.firstLocalFallbackURL == target.projectURL)
    }

    @Test
    func keepsCodexDeepLinkForReverseHandoffCodexSurface() {
        let target = LooperThreadOpenTarget(
            threadId: "thread-main",
            transcriptPath: nil,
            workingDirectory: "/Users/test/project",
            assistantSurface: "codex"
        )

        #expect(target.codexURL?.absoluteString == "codex://threads/thread-main")
    }
}

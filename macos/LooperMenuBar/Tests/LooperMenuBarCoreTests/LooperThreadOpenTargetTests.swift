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

        #expect(target.codexURL?.absoluteString == "codex://thread/thread-main")
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

        #expect(target.codexURL?.absoluteString == "codex://thread/thread%20with%20spaces")
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
}

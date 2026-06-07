import Testing
@testable import LooperCompanionCore

@Suite("Companion surface filtering")
struct CompanionSurfaceFilteringTests {
    @Test("Codex surface hides Devin and Grok sessions")
    func codexSurfaceFiltersAssistantClients() {
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "codex", surface: "codex"))
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "cursor", surface: "codex"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "devin", surface: "codex"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "grok-build", surface: "codex"))
    }

    @Test("Devin surface only shows Devin sessions")
    func devinSurfaceFiltersAssistantClients() {
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "devin", surface: "devin"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "codex", surface: "devin"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "grok-build", surface: "devin"))
    }

    @Test("Grok Build surface only shows Grok sessions")
    func grokSurfaceFiltersAssistantClients() {
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "grok-build", surface: "grok-build"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "codex", surface: "grok-build"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "devin", surface: "grok-build"))
    }

    @Test("Path inference matches Rust assistant client rules")
    func pathInferenceMatchesRustRules() {
        #expect(
            CompanionSurfaceFiltering.inferAssistantClient(
                transcriptPath: "/Users/test/.grok/sessions/grok-thread.jsonl",
                cwd: nil,
                source: nil,
                originator: nil,
                agentPath: nil
            ) == "grok-build"
        )
        #expect(
            CompanionSurfaceFiltering.inferAssistantClient(
                transcriptPath: nil,
                cwd: "/Users/test/.devin-next/worktrees/app",
                source: nil,
                originator: nil,
                agentPath: nil
            ) == "devin"
        )
        #expect(
            CompanionSurfaceFiltering.inferAssistantClient(
                transcriptPath: "/Users/test/.codex/sessions/thread.jsonl",
                cwd: nil,
                source: nil,
                originator: nil,
                agentPath: nil
            ) == "codex"
        )
        #expect(
            CompanionSurfaceFiltering.inferAssistantClient(
                transcriptPath: "/Users/test/.codex/sessions/thread.jsonl",
                cwd: nil,
                source: "vscode",
                originator: "Codex Desktop",
                agentPath: nil
            ) == "codex"
        )
        #expect(
            CompanionSurfaceFiltering.inferAssistantClient(
                transcriptPath: "/Users/test/.codex/sessions/thread.jsonl",
                cwd: "/Users/test/project",
                source: "vscode",
                originator: "Devin - Next",
                agentPath: nil
            ) == "devin"
        )
    }

    @Test("Path-based surface filtering mirrors server rules")
    func pathBasedSurfaceFilteringMirrorsServer() {
        #expect(
            CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.grok/sessions/grok-thread.jsonl",
                cwd: nil,
                source: nil,
                originator: nil,
                agentPath: nil,
                surface: "grok-build"
            )
        )
        #expect(
            !CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.grok/sessions/grok-thread.jsonl",
                cwd: nil,
                source: nil,
                originator: nil,
                agentPath: nil,
                surface: "codex"
            )
        )
        #expect(
            CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.codex/sessions/codex-thread.jsonl",
                cwd: nil,
                source: "vscode",
                originator: "Codex Desktop",
                agentPath: nil,
                surface: "codex"
            )
        )
        #expect(
            !CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.codex/sessions/codex-thread.jsonl",
                cwd: nil,
                source: "vscode",
                originator: "Codex Desktop",
                agentPath: nil,
                surface: "devin"
            )
        )
    }
}

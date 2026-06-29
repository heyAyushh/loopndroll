import Testing
@testable import LooperCompanionCore

@Suite("Companion surface filtering")
struct CompanionSurfaceFilteringTests {
    private struct LocalMini {
        let id: String
        let assistantClient: String
    }

    @Test("Codex surface hides Devin and Grok sessions")
    func codexSurfaceFiltersAssistantClients() {
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "codex", surface: "codex"))
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "cursor", surface: "codex"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "claude-code", surface: "codex"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "zed", surface: "codex"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "devin", surface: "codex"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "grok-build", surface: "codex"))
    }

    @Test("Claude Code surface only shows Claude Code sessions")
    func claudeCodeSurfaceFiltersAssistantClients() {
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "claude-code", surface: "claude-code"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "codex", surface: "claude-code"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "zed", surface: "claude-code"))
    }

    @Test("Zed surface only shows Zed sessions")
    func zedSurfaceFiltersAssistantClients() {
        #expect(CompanionSurfaceFiltering.matches(assistantClient: "zed", surface: "zed"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "codex", surface: "zed"))
        #expect(!CompanionSurfaceFiltering.matches(assistantClient: "claude-code", surface: "zed"))
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
                cwd: nil,
                source: "vscode",
                originator: "Claude Code",
                agentPath: nil
            ) == "claude-code"
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
        #expect(
            CompanionSurfaceFiltering.inferAssistantClient(
                transcriptPath: "/Users/test/.zed/sessions/thread.jsonl",
                cwd: "/Users/test/project",
                source: "zed-agent-servers",
                originator: "Zed ACP",
                agentPath: nil
            ) == "zed"
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
                transcriptPath: "/Users/test/.codex/sessions/claude-thread.jsonl",
                cwd: nil,
                source: "vscode",
                originator: "Claude Code",
                agentPath: nil,
                surface: "claude-code"
            )
        )
        #expect(
            !CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.codex/sessions/claude-thread.jsonl",
                cwd: nil,
                source: "vscode",
                originator: "Claude Code",
                agentPath: nil,
                surface: "codex"
            )
        )
        #expect(
            !CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.codex/sessions/claude-thread.jsonl",
                cwd: nil,
                source: "vscode",
                originator: "Claude Code",
                agentPath: nil,
                surface: "devin"
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
        #expect(
            CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.zed/sessions/zed-thread.jsonl",
                cwd: nil,
                source: "zed-agent-servers",
                originator: "Zed ACP",
                agentPath: nil,
                surface: "zed"
            )
        )
        #expect(
            !CompanionSurfaceFiltering.sessionMatchesSurface(
                transcriptPath: "/Users/test/.zed/sessions/zed-thread.jsonl",
                cwd: nil,
                source: "zed-agent-servers",
                originator: "Zed ACP",
                agentPath: nil,
                surface: "codex"
            )
        )
    }

    @Test("Rapid surface switches keep local mini source stable")
    func rapidSurfaceSwitchesKeepLocalMiniSourceStable() {
        let minis = [
            LocalMini(id: "codex-1", assistantClient: "codex"),
            LocalMini(id: "cursor-1", assistantClient: "cursor"),
            LocalMini(id: "claude-1", assistantClient: "claude-code"),
            LocalMini(id: "zed-1", assistantClient: "zed"),
            LocalMini(id: "devin-1", assistantClient: "devin"),
            LocalMini(id: "grok-1", assistantClient: "grok-build"),
        ]
        let originalIDs = minis.map(\.id)
        let surfaces = ["codex", "claude-code", "zed", "devin", "grok-build"]
        let expectedIDsBySurface = [
            "codex": ["codex-1", "cursor-1"],
            "claude-code": ["claude-1"],
            "zed": ["zed-1"],
            "devin": ["devin-1"],
            "grok-build": ["grok-1"],
        ]
        var visibleIDs: [String] = []

        for index in 0..<100 {
            let surface = surfaces[index % surfaces.count]
            visibleIDs = minis
                .filter { mini in
                    CompanionSurfaceFiltering.matches(
                        assistantClient: mini.assistantClient,
                        surface: surface
                    )
                }
                .map(\.id)

            #expect(minis.map(\.id) == originalIDs)
            #expect(visibleIDs == expectedIDsBySurface[surface])
        }

        #expect(visibleIDs == expectedIDsBySurface["grok-build"])
    }
}

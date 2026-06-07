public enum CompanionSurfaceFiltering {
    private static let codexSurfaceClients = [
        "codex",
        "cursor",
        "claude-code",
        "super-engineering",
        "openclaw",
    ]
    private static let devinSurfaceClients = ["devin"]
    private static let grokBuildSurfaceClients = ["grok-build"]

    public static func matches(assistantClient: String, surface: String) -> Bool {
        switch surface {
        case "devin":
            return devinSurfaceClients.contains(assistantClient)
        case "grok-build":
            return grokBuildSurfaceClients.contains(assistantClient)
        default:
            return codexSurfaceClients.contains(assistantClient)
        }
    }

    public static func inferAssistantClient(
        transcriptPath: String?,
        cwd: String?,
        source: String?,
        originator: String?,
        agentPath: String?
    ) -> String {
        let primaryHaystack = [transcriptPath, source, originator, agentPath]
            .compactMap { $0?.lowercased() }
            .joined(separator: " ")
        let pathHaystack = [transcriptPath, cwd, source, agentPath]
            .compactMap { $0?.lowercased() }
            .joined(separator: " ")

        if containsAny(primaryHaystack, needles: [".grok/", "/.grok/", "grok-build", "grok build"]) {
            return "grok-build"
        }
        if containsAny(originator?.lowercased() ?? "", needles: ["devin", "devin desktop", "devin next", "devin - next"]) {
            return "devin"
        }
        if containsAny(originator?.lowercased() ?? "", needles: ["codex desktop", "codex app"]) {
            return "codex"
        }

        if containsAny(pathHaystack, needles: [".grok/", "/.grok/", "grok-build", "grok build"]) {
            return "grok-build"
        }
        if containsAny(
            pathHaystack,
            needles: [
                ".devin-next",
                "/applications/devin.app/",
                "/applications/devin - next.app/",
                "devin-desktop",
            ]
        ) {
            return "devin"
        }
        if containsAny(pathHaystack, needles: ["/.cursor/", ".cursor/extensions", "/cursor.app/"]) {
            return "cursor"
        }
        if containsAny(pathHaystack, needles: [".claude/", "claude-code", "claudefordesktop"]) {
            return "claude-code"
        }
        if containsAny(pathHaystack, needles: [".superconductor", "super-engineering"]) {
            return "super-engineering"
        }
        if containsAny(pathHaystack, needles: ["openclaw", "open-claw"]) {
            return "openclaw"
        }
        if containsAny(pathHaystack, needles: ["/.codex/", ".codex/sessions"]) {
            return "codex"
        }

        return "codex"
    }

    public static func sessionMatchesSurface(
        transcriptPath: String?,
        cwd: String?,
        source: String?,
        originator: String?,
        agentPath: String?,
        surface: String
    ) -> Bool {
        let client = inferAssistantClient(
            transcriptPath: transcriptPath,
            cwd: cwd,
            source: source,
            originator: originator,
            agentPath: agentPath
        )
        return matches(assistantClient: client, surface: surface)
    }

    private static func containsAny(_ haystack: String, needles: [String]) -> Bool {
        needles.contains { haystack.contains($0.lowercased()) }
    }
}

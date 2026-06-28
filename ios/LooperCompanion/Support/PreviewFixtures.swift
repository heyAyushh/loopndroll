import Foundation

enum PreviewFixtures {
    private static let blockedGoal = SessionGoalSummary(
        id: "goal-session-1",
        title: "Make an iOS app for looper",
        status: "blocked",
        lifecycle: "blocked",
        running: false,
        tokenBudget: nil,
        tokensUsed: nil,
        timeUsedSeconds: nil,
        updatedAtMs: nil
    )

    static let notifications = [
        NotificationDestination(id: "telegram-main", label: "Telegram Main", channel: "telegram"),
        NotificationDestination(id: "slack-builds", label: "Slack Builds", channel: "slack")
    ]

    static let completionChecks = [
        CompletionCheckSummary(id: "check-1", label: "Repo Green", commandCount: 3),
        CompletionCheckSummary(id: "check-2", label: "Smoke Test", commandCount: 1)
    ]

    static let snapshot = MobileSnapshot(
        host: HostSummary(
            id: "preview-mac",
            name: "Ay's Mac",
            address: "100.64.0.12:8787",
            isReachable: true,
            lastSyncedAt: Date().ISO8601Format()
        ),
        globalSettings: GlobalSettings(
            defaultPrompt: "Keep working on the task. Do not finish yet.",
            globalMode: .infinite,
            scope: "per-task",
            notificationLabel: "Telegram Main",
            completionCheckLabel: "Repo Green",
            completionCheckWaitForReply: true
        ),
        sessions: [
            SessionSummary(
                id: "session-1",
                ref: "C22",
                title: "Make an iOS app for looper",
                status: .active,
                effectiveMode: .infinite,
                lastUpdatedAt: Date().addingTimeInterval(-180).ISO8601Format(),
                assistantPreview: "I’ve scaffolded the iPhone companion app and I’m wiring the simulator data source now.",
                isArchived: false,
                assistantClient: .codex,
                goal: blockedGoal
            ),
            SessionSummary(
                id: "session-2",
                ref: "C21",
                title: "Debug haptics on device",
                status: .waiting,
                effectiveMode: .awaitReply,
                lastUpdatedAt: Date().addingTimeInterval(-1_400).ISO8601Format(),
                assistantPreview: "I’m waiting for a reply before continuing with the haptics pass.",
                isArchived: false,
                assistantClient: .cursor
            ),
            SessionSummary(
                id: "session-3",
                ref: "C17",
                title: "Fix completion checks for release build",
                status: .stopped,
                effectiveMode: .completionChecks,
                lastUpdatedAt: Date().addingTimeInterval(-7_200).ISO8601Format(),
                assistantPreview: "Typecheck passed, but the simulator smoke test still needs work.",
                isArchived: false,
                assistantClient: .claudeCode
            ),
            SessionSummary(
                id: "session-4",
                ref: "C11",
                title: "Archive old desktop polish branch",
                status: .archived,
                effectiveMode: nil,
                lastUpdatedAt: Date().addingTimeInterval(-86_400).ISO8601Format(),
                assistantPreview: "The work is done and the session has been archived.",
                isArchived: true,
                assistantClient: .openclaw
            ),
            SessionSummary(
                id: "session-5",
                ref: "C09",
                title: "Ship Super.Engineering integration",
                status: .active,
                effectiveMode: .infinite,
                lastUpdatedAt: Date().addingTimeInterval(-2_700).ISO8601Format(),
                assistantPreview: "Wiring the Super.Engineering bridge and validating session sync.",
                isArchived: false,
                assistantClient: .superEngineering
            ),
            SessionSummary(
                id: "session-6",
                ref: "C06",
                title: "Extend Grok Build across surfaces",
                status: .active,
                effectiveMode: .infinite,
                lastUpdatedAt: Date().addingTimeInterval(-900).ISO8601Format(),
                assistantPreview: "Grok hooks registered and sessions synced across menubar, TUI, and iOS.",
                isArchived: false,
                assistantClient: .grokBuild
            )
        ],
        notifications: notifications,
        completionChecks: completionChecks,
        grokBuild: GrokBuildStatus(
            hooks: GrokBuildHookStatus(
                health: "healthy",
                owner: "looper-rust",
                registeredEvents: ["session", "stop"],
                hooksPath: "/Users/test/.grok/hooks/looper.json"
            ),
            sessionCount: 1,
            activeSessionCount: 1
        )
    )
}

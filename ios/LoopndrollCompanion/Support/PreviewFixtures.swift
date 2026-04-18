import Foundation

enum PreviewFixtures {
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
            id: "local-mac",
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
                isArchived: false
            ),
            SessionSummary(
                id: "session-2",
                ref: "C21",
                title: "Debug haptics on device",
                status: .waiting,
                effectiveMode: .awaitReply,
                lastUpdatedAt: Date().addingTimeInterval(-1_400).ISO8601Format(),
                assistantPreview: "I’m waiting for a reply before continuing with the haptics pass.",
                isArchived: false
            ),
            SessionSummary(
                id: "session-3",
                ref: "C17",
                title: "Fix completion checks for release build",
                status: .stopped,
                effectiveMode: .completionChecks,
                lastUpdatedAt: Date().addingTimeInterval(-7_200).ISO8601Format(),
                assistantPreview: "Typecheck passed, but the simulator smoke test still needs work.",
                isArchived: false
            ),
            SessionSummary(
                id: "session-4",
                ref: "C11",
                title: "Archive old desktop polish branch",
                status: .archived,
                effectiveMode: nil,
                lastUpdatedAt: Date().addingTimeInterval(-86_400).ISO8601Format(),
                assistantPreview: "The work is done and the session has been archived.",
                isArchived: true
            )
        ],
        notifications: notifications,
        completionChecks: completionChecks
    )

    static let sessionDetails: [String: SessionDetail] = [
        "session-1": SessionDetail(
            id: "session-1",
            ref: "C22",
                title: "Make an iOS app for looper",
            status: .active,
            effectiveMode: .infinite,
            lastUpdatedAt: Date().addingTimeInterval(-180).ISO8601Format(),
            assistantPreview: "I’ve scaffolded the iPhone companion app and I’m wiring the simulator data source now.",
            latestAssistantMessage: "I’ve scaffolded the iPhone app and I’m wiring the simulator data source now. Next I’m finishing the session detail view and the Bun dev API so the simulator can show real looper-shaped state.",
            isArchived: false,
            notificationIds: ["telegram-main"],
            completionCheckID: "check-1",
            completionCheckWaitForReply: true,
            availableNotifications: notifications,
            availableCompletionChecks: completionChecks
        ),
        "session-2": SessionDetail(
            id: "session-2",
            ref: "C21",
            title: "Debug haptics on device",
            status: .waiting,
            effectiveMode: .awaitReply,
            lastUpdatedAt: Date().addingTimeInterval(-1_400).ISO8601Format(),
            assistantPreview: "I’m waiting for a reply before continuing with the haptics pass.",
            latestAssistantMessage: "I’m waiting for a reply before continuing with the haptics pass.",
            isArchived: false,
            notificationIds: ["telegram-main"],
            completionCheckID: nil,
            completionCheckWaitForReply: false,
            availableNotifications: notifications,
            availableCompletionChecks: completionChecks
        ),
        "session-3": SessionDetail(
            id: "session-3",
            ref: "C17",
            title: "Fix completion checks for release build",
            status: .stopped,
            effectiveMode: .completionChecks,
            lastUpdatedAt: Date().addingTimeInterval(-7_200).ISO8601Format(),
            assistantPreview: "Typecheck passed, but the simulator smoke test still needs work.",
            latestAssistantMessage: "Typecheck passed, but the simulator smoke test still needs work.",
            isArchived: false,
            notificationIds: ["slack-builds"],
            completionCheckID: "check-1",
            completionCheckWaitForReply: true,
            availableNotifications: notifications,
            availableCompletionChecks: completionChecks
        ),
        "session-4": SessionDetail(
            id: "session-4",
            ref: "C11",
            title: "Archive old desktop polish branch",
            status: .archived,
            effectiveMode: nil,
            lastUpdatedAt: Date().addingTimeInterval(-86_400).ISO8601Format(),
            assistantPreview: "The work is done and the session has been archived.",
            latestAssistantMessage: "The work is done and the session has been archived.",
            isArchived: true,
            notificationIds: [],
            completionCheckID: nil,
            completionCheckWaitForReply: false,
            availableNotifications: notifications,
            availableCompletionChecks: completionChecks
        )
    ]
}

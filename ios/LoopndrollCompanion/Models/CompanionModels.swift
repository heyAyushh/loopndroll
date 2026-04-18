import Foundation

enum ConnectivityState: String, Sendable {
    case connecting
    case connected
    case offline
    case unauthorized
    case unpaired

    var label: String {
        switch self {
        case .connecting:
            return "Connecting"
        case .connected:
            return "Connected"
        case .offline:
            return "Offline"
        case .unauthorized:
            return "Unauthorized"
        case .unpaired:
            return "Unpaired"
        }
    }

    var symbolName: String {
        switch self {
        case .connecting:
            return "bolt.horizontal.circle"
        case .connected:
            return "checkmark.circle.fill"
        case .offline:
            return "wifi.slash"
        case .unauthorized:
            return "lock.slash"
        case .unpaired:
            return "link.badge.plus"
        }
    }

    var summary: String {
        switch self {
        case .connecting:
            return "Trying the configured Mac endpoint."
        case .connected:
            return "Live session state is flowing from your Mac."
        case .offline:
            return "The iPhone cannot reach your Mac right now."
        case .unauthorized:
            return "The Mac rejected this iPhone connection."
        case .unpaired:
            return "Scan the Mac code or enter the device code in Settings to start syncing looper."
        }
    }
}

enum SessionMode: String, CaseIterable, Codable, Sendable {
    case infinite = "infinite"
    case awaitReply = "await-reply"
    case completionChecks = "completion-checks"
    case maxTurns1 = "max-turns-1"
    case maxTurns2 = "max-turns-2"
    case maxTurns3 = "max-turns-3"

    var label: String {
        switch self {
        case .infinite:
            return "Infinite"
        case .awaitReply:
            return "Await Reply"
        case .completionChecks:
            return "Completion Checks"
        case .maxTurns1:
            return "Max Turns 1"
        case .maxTurns2:
            return "Max Turns 2"
        case .maxTurns3:
            return "Max Turns 3"
        }
    }

    var symbolName: String {
        switch self {
        case .infinite:
            return "infinity.circle"
        case .awaitReply:
            return "ellipsis.message"
        case .completionChecks:
            return "checklist"
        case .maxTurns1:
            return "1.circle"
        case .maxTurns2:
            return "2.circle"
        case .maxTurns3:
            return "3.circle"
        }
    }

    var summary: String {
        switch self {
        case .infinite:
            return "Keep the session moving without a turn cap."
        case .awaitReply:
            return "Pause again until the user replies."
        case .completionChecks:
            return "Stop when the configured checks pass."
        case .maxTurns1:
            return "Run one additional assistant turn."
        case .maxTurns2:
            return "Run two additional assistant turns."
        case .maxTurns3:
            return "Run three additional assistant turns."
        }
    }
}

enum SessionStatus: String, Codable, Sendable {
    case active
    case waiting
    case stopped
    case archived

    var label: String {
        rawValue.capitalized
    }

    var symbolName: String {
        switch self {
        case .active:
            return "bolt.fill"
        case .waiting:
            return "hourglass"
        case .stopped:
            return "pause.circle"
        case .archived:
            return "archivebox"
        }
    }

    var summary: String {
        switch self {
        case .active:
            return "Looper is still working."
        case .waiting:
            return "This session is waiting for the next signal."
        case .stopped:
            return "This session needs attention."
        case .archived:
            return "This session is no longer in the active queue."
        }
    }
}

enum QuickActionOption: String, CaseIterable, Codable, Identifiable, Sendable {
    case openSession = "open-session"
    case continueChat = "continue"
    case reply
    case archive
    case muteSession = "mute-session"

    var id: String { rawValue }

    var label: String {
        switch self {
        case .openSession:
            return "Open Session"
        case .continueChat:
            return "Continue"
        case .reply:
            return "Reply"
        case .archive:
            return "Archive"
        case .muteSession:
            return "Mute Session"
        }
    }
}

enum SettingsSearchTarget: String, CaseIterable, Hashable, Identifiable, Sendable {
    case connection
    case continuePrompt
    case stopQuickActions
    case notificationRoutes
    case completionChecks

    var id: String { rawValue }

    var title: String {
        switch self {
        case .connection:
            return "Connect to Mac"
        case .continuePrompt:
            return "Continue Prompt"
        case .stopQuickActions:
            return "Stop Quick Actions"
        case .notificationRoutes:
            return "Notification Routes"
        case .completionChecks:
            return "Completion Checks"
        }
    }

    var subtitle: String {
        switch self {
        case .connection:
            return "Scan the Mac code or enter the device code manually."
        case .continuePrompt:
            return "Edit the message looper sends when a chat continues."
        case .stopQuickActions:
            return "Choose which actions appear when a stop alert expands."
        case .notificationRoutes:
            return "Review the Mac routes that can receive alerts."
        case .completionChecks:
            return "Inspect the checks available from the Mac."
        }
    }

    var systemImage: String {
        switch self {
        case .connection:
            return "link.badge.plus"
        case .continuePrompt:
            return "text.cursor"
        case .stopQuickActions:
            return "hand.tap"
        case .notificationRoutes:
            return "bell.badge"
        case .completionChecks:
            return "checklist"
        }
    }

    var keywords: [String] {
        switch self {
        case .connection:
            return ["connect", "mac", "device code", "scan mac code", "pair", "link"]
        case .continuePrompt:
            return ["continue prompt", "prompt", "default prompt", "continue"]
        case .stopQuickActions:
            return ["stop quick actions", "quick actions", "stop alert", "actions"]
        case .notificationRoutes:
            return ["notification routes", "notifications", "routes", "alerts", "push"]
        case .completionChecks:
            return ["completion checks", "completion", "checks", "rules"]
        }
    }
}

struct HostSummary: Codable, Sendable {
    var id: String
    var name: String
    var address: String
    var isReachable: Bool
    var lastSyncedAt: String
}

struct GlobalSettings: Codable, Sendable {
    var defaultPrompt: String
    var globalMode: SessionMode?
    var scope: String
    var notificationLabel: String?
    var completionCheckLabel: String?
    var completionCheckWaitForReply: Bool
}

enum RemotePushEnvironment: String, Codable, Sendable {
    case development
    case production

    var label: String {
        switch self {
        case .development:
            return "Development"
        case .production:
            return "TestFlight"
        }
    }

    static var currentBuild: RemotePushEnvironment {
        #if DEBUG
        .development
        #else
        .production
        #endif
    }
}

enum RemotePushRegistrationState: String, Codable, Sendable {
    case enabled
    case storedAwaitingProvider = "stored-awaiting-provider"

    var label: String {
        switch self {
        case .enabled:
            return "Remote push ready"
        case .storedAwaitingProvider:
            return "Waiting for APNs"
        }
    }
}

struct RemotePushRegistrationRequest: Codable, Sendable {
    var installationId: String
    var deviceToken: String
    var bundleId: String
    var environment: RemotePushEnvironment
    var deviceName: String?
}

struct RemotePushRegistrationResponse: Codable, Sendable {
    var state: RemotePushRegistrationState
    var environment: RemotePushEnvironment
    var registeredAt: String
    var message: String
}

struct RemotePushTestResponse: Codable, Sendable {
    var delivered: Bool
    var message: String
}

struct NotificationDestination: Codable, Identifiable, Hashable, Sendable {
    var id: String
    var label: String
    var channel: String
}

struct CompletionCheckSummary: Codable, Identifiable, Hashable, Sendable {
    var id: String
    var label: String
    var commandCount: Int
}

/// Coding agent surface inferred on the Mac from cwd / transcript paths; icons are SF Symbols (native iOS assets).
enum AssistantClient: String, Codable, Sendable, CaseIterable, Hashable {
    case unknown
    case codex
    case cursor
    case claudeCode = "claude-code"
    case superEngineering = "super-engineering"
    case openclaw

    var displayTitle: String {
        switch self {
        case .unknown:
            return "Unknown"
        case .codex:
            return "Codex"
        case .cursor:
            return "Cursor"
        case .claudeCode:
            return "Claude Code"
        case .superEngineering:
            return "Super.Engineering"
        case .openclaw:
            return "OpenClaw"
        }
    }

    /// Search keywords so typing e.g. "claude" still finds matching sessions.
    var searchKeywords: [String] {
        switch self {
        case .unknown:
            return ["assistant", "agent", "cli"]
        case .codex:
            return ["codex", "openai codex"]
        case .cursor:
            return ["cursor", "cursor ide"]
        case .claudeCode:
            return ["claude", "claude code", "anthropic"]
        case .superEngineering:
            return ["super", "super.engineering", "super engineering", "superengineering"]
        case .openclaw:
            return ["openclaw", "open claw", "claw"]
        }
    }

    var systemImageName: String {
        switch self {
        case .unknown:
            return "questionmark.app.dashed"
        case .codex:
            return "terminal"
        case .cursor:
            return "cursorarrow.click.2"
        case .claudeCode:
            return "sparkles"
        case .superEngineering:
            return "gearshape.2"
        case .openclaw:
            return "pawprint.fill"
        }
    }
}

struct SessionSummary: Codable, Identifiable, Hashable, Sendable {
    var id: String
    var ref: String
    var title: String
    var status: SessionStatus
    var effectiveMode: SessionMode?
    var lastUpdatedAt: String
    var assistantPreview: String?
    var isArchived: Bool
    var assistantClient: AssistantClient

    private enum CodingKeys: String, CodingKey {
        case id
        case ref
        case title
        case status
        case effectiveMode
        case lastUpdatedAt
        case assistantPreview
        case isArchived
        case assistantClient
    }

    init(
        id: String,
        ref: String,
        title: String,
        status: SessionStatus,
        effectiveMode: SessionMode?,
        lastUpdatedAt: String,
        assistantPreview: String?,
        isArchived: Bool,
        assistantClient: AssistantClient = .unknown
    ) {
        self.id = id
        self.ref = ref
        self.title = title
        self.status = status
        self.effectiveMode = effectiveMode
        self.lastUpdatedAt = lastUpdatedAt
        self.assistantPreview = assistantPreview
        self.isArchived = isArchived
        self.assistantClient = assistantClient
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        ref = try container.decode(String.self, forKey: .ref)
        title = try container.decode(String.self, forKey: .title)
        status = try container.decode(SessionStatus.self, forKey: .status)
        effectiveMode = try container.decodeIfPresent(SessionMode.self, forKey: .effectiveMode)
        lastUpdatedAt = try container.decode(String.self, forKey: .lastUpdatedAt)
        assistantPreview = try container.decodeIfPresent(String.self, forKey: .assistantPreview)
        isArchived = try container.decode(Bool.self, forKey: .isArchived)
        assistantClient = try container.decodeIfPresent(AssistantClient.self, forKey: .assistantClient) ?? .unknown
    }
}

struct SessionDetail: Codable, Identifiable, Sendable {
    var id: String
    var ref: String
    var title: String
    var status: SessionStatus
    var effectiveMode: SessionMode?
    var lastUpdatedAt: String
    var assistantPreview: String?
    var latestAssistantMessage: String?
    var isArchived: Bool
    var assistantClient: AssistantClient
    var notificationIds: [String]
    var completionCheckID: String?
    var completionCheckWaitForReply: Bool
    var availableNotifications: [NotificationDestination]
    var availableCompletionChecks: [CompletionCheckSummary]

    private enum CodingKeys: String, CodingKey {
        case id
        case ref
        case title
        case status
        case effectiveMode
        case lastUpdatedAt
        case assistantPreview
        case latestAssistantMessage
        case isArchived
        case assistantClient
        case notificationIds
        case completionCheckID
        case completionCheckWaitForReply
        case availableNotifications
        case availableCompletionChecks
    }

    init(
        id: String,
        ref: String,
        title: String,
        status: SessionStatus,
        effectiveMode: SessionMode?,
        lastUpdatedAt: String,
        assistantPreview: String?,
        latestAssistantMessage: String?,
        isArchived: Bool,
        assistantClient: AssistantClient = .unknown,
        notificationIds: [String],
        completionCheckID: String?,
        completionCheckWaitForReply: Bool,
        availableNotifications: [NotificationDestination],
        availableCompletionChecks: [CompletionCheckSummary]
    ) {
        self.id = id
        self.ref = ref
        self.title = title
        self.status = status
        self.effectiveMode = effectiveMode
        self.lastUpdatedAt = lastUpdatedAt
        self.assistantPreview = assistantPreview
        self.latestAssistantMessage = latestAssistantMessage
        self.isArchived = isArchived
        self.assistantClient = assistantClient
        self.notificationIds = notificationIds
        self.completionCheckID = completionCheckID
        self.completionCheckWaitForReply = completionCheckWaitForReply
        self.availableNotifications = availableNotifications
        self.availableCompletionChecks = availableCompletionChecks
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        ref = try container.decode(String.self, forKey: .ref)
        title = try container.decode(String.self, forKey: .title)
        status = try container.decode(SessionStatus.self, forKey: .status)
        effectiveMode = try container.decodeIfPresent(SessionMode.self, forKey: .effectiveMode)
        lastUpdatedAt = try container.decode(String.self, forKey: .lastUpdatedAt)
        assistantPreview = try container.decodeIfPresent(String.self, forKey: .assistantPreview)
        latestAssistantMessage = try container.decodeIfPresent(String.self, forKey: .latestAssistantMessage)
        isArchived = try container.decode(Bool.self, forKey: .isArchived)
        assistantClient = try container.decodeIfPresent(AssistantClient.self, forKey: .assistantClient) ?? .unknown
        notificationIds = try container.decode([String].self, forKey: .notificationIds)
        completionCheckID = try container.decodeIfPresent(String.self, forKey: .completionCheckID)
        completionCheckWaitForReply = try container.decode(Bool.self, forKey: .completionCheckWaitForReply)
        availableNotifications = try container.decode([NotificationDestination].self, forKey: .availableNotifications)
        availableCompletionChecks = try container.decode(
            [CompletionCheckSummary].self,
            forKey: .availableCompletionChecks
        )
    }
}

struct MobileSnapshot: Codable, Sendable {
    var host: HostSummary
    var globalSettings: GlobalSettings
    var sessions: [SessionSummary]
    var notifications: [NotificationDestination]
    var completionChecks: [CompletionCheckSummary]
}

enum ModelFormatting {
    static func relativeTimestamp(_ value: String) -> String {
        let formatter = ISO8601DateFormatter()

        guard let date = formatter.date(from: value) else {
            return "Unknown"
        }

        return date.formatted(.relative(presentation: .named))
    }

    static func friendlyDateTime(_ value: String) -> String {
        let formatter = ISO8601DateFormatter()

        guard let date = formatter.date(from: value) else {
            return "Unknown"
        }

        return date.formatted(date: .abbreviated, time: .shortened)
    }

    static func friendlyMode(_ mode: SessionMode?) -> String {
        mode?.label ?? "Off"
    }
}

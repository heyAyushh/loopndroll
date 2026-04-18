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

struct SessionSummary: Codable, Identifiable, Hashable, Sendable {
    var id: String
    var ref: String
    var title: String
    var status: SessionStatus
    var effectiveMode: SessionMode?
    var lastUpdatedAt: String
    var assistantPreview: String?
    var isArchived: Bool
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
    var notificationIds: [String]
    var completionCheckID: String?
    var completionCheckWaitForReply: Bool
    var availableNotifications: [NotificationDestination]
    var availableCompletionChecks: [CompletionCheckSummary]
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

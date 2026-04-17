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
}

enum SessionStatus: String, Codable, Sendable {
    case active
    case waiting
    case stopped
    case archived

    var label: String {
        rawValue.capitalized
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

    static func friendlyMode(_ mode: SessionMode?) -> String {
        mode?.label ?? "Off"
    }
}

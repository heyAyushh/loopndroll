import Foundation

enum LooperContinuationActivity {
    static let activityType = "dev.looper.app.continue-session"
    static let deepLinkScheme = "looper"
    static let sessionDeepLinkHost = "session"

    private enum DeepLinkQueryKey {
        static let baseURL = "baseURL"
        static let baseURLSnakeCase = "base_url"
        static let url = "url"
    }

    private enum UserInfoKey {
        static let handoffWebpageURL = "handoffWebpageURL"
        static let sessionID = "sessionID"
    }

    private enum TargetContentIdentifier {
        static let sessionPrefix = "looper.session."
    }

    static func sessionID(from activity: NSUserActivity) -> String? {
        guard activity.activityType == activityType else {
            return nil
        }

        return sessionIDFromUserInfo(activity)
            ?? sessionIDFromTargetContentIdentifier(activity)
            ?? sessionIDFromWebpageURL(activity)
    }

    static func baseURL(from activity: NSUserActivity) -> URL? {
        guard activity.activityType == activityType else {
            return nil
        }

        if let webpageURL = activity.webpageURL,
           let baseURL = baseURLFromHandoffWebpageURL(webpageURL)
        {
            return baseURL
        }

        guard let rawURL = activity.userInfo?[UserInfoKey.handoffWebpageURL] as? String,
              let url = URL(string: rawURL)
        else {
            return nil
        }

        return baseURLFromHandoffWebpageURL(url)
    }

    static func sessionID(from url: URL) -> String? {
        guard url.scheme == deepLinkScheme,
              url.host == sessionDeepLinkHost
        else {
            return nil
        }

        return url.path
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
            .removingPercentEncoding?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
    }

    static func baseURL(from url: URL) -> URL? {
        guard url.scheme == deepLinkScheme,
              url.host == sessionDeepLinkHost,
              let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else {
            return nil
        }

        return components.queryItems?
            .first(where: { item in
                item.name == DeepLinkQueryKey.baseURL ||
                    item.name == DeepLinkQueryKey.baseURLSnakeCase ||
                    item.name == DeepLinkQueryKey.url
            })
            .flatMap(\.value)
            .flatMap(URL.init(string:))
    }

    private static func sessionIDFromUserInfo(_ activity: NSUserActivity) -> String? {
        (activity.userInfo?[UserInfoKey.sessionID] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
    }

    private static func sessionIDFromTargetContentIdentifier(_ activity: NSUserActivity) -> String? {
        guard let identifier = activity.targetContentIdentifier?
            .trimmingCharacters(in: .whitespacesAndNewlines),
            identifier.hasPrefix(TargetContentIdentifier.sessionPrefix)
        else {
            return nil
        }

        return String(identifier.dropFirst(TargetContentIdentifier.sessionPrefix.count))
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
    }

    private static func sessionIDFromWebpageURL(_ activity: NSUserActivity) -> String? {
        if let webpageURL = activity.webpageURL,
           let sessionID = sessionIDFromHandoffWebpageURL(webpageURL)
        {
            return sessionID
        }

        guard let rawURL = activity.userInfo?[UserInfoKey.handoffWebpageURL] as? String,
              let url = URL(string: rawURL)
        else {
            return nil
        }

        return sessionIDFromHandoffWebpageURL(url)
    }

    private static func sessionIDFromHandoffWebpageURL(_ url: URL) -> String? {
        guard isHandoffSessionWebpageURL(url) else {
            return nil
        }

        return url.pathComponents.filter { $0 != "/" }.last?
            .removingPercentEncoding?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
    }

    private static func baseURLFromHandoffWebpageURL(_ url: URL) -> URL? {
        guard isHandoffSessionWebpageURL(url),
              var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else {
            return nil
        }

        components.path = ""
        components.query = nil
        components.fragment = nil
        return components.url
    }

    private static func isHandoffSessionWebpageURL(_ url: URL) -> Bool {
        let pathComponents = url.pathComponents.filter { $0 != "/" }
        guard pathComponents.count >= HandoffWebPath.minimumComponentCount else {
            return false
        }

        return pathComponents[pathComponents.count - HandoffWebPath.sessionComponentOffset]
            == HandoffWebPath.sessionsComponent &&
            pathComponents[pathComponents.count - HandoffWebPath.handoffComponentOffset]
            == HandoffWebPath.handoffComponent
    }

    private enum HandoffWebPath {
        static let handoffComponent = "handoff"
        static let sessionsComponent = "sessions"
        static let minimumComponentCount = 3
        static let sessionComponentOffset = 2
        static let handoffComponentOffset = 3
    }
}

enum LooperConnectionDeepLink {
    static let scheme = "looper"
    static let host = "connect"

    private enum QueryKey {
        static let code = "code"
        static let connectionCode = "connectionCode"
        static let connectionCodeSnakeCase = "connection_code"
    }

    static func connectionCode(from url: URL) -> String? {
        guard url.scheme == scheme,
              url.host == host,
              let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else {
            return nil
        }

        return components.queryItems?
            .first(where: isConnectionCodeQueryItem)
            .flatMap(\.value)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
    }

    private static func isConnectionCodeQueryItem(_ item: URLQueryItem) -> Bool {
        item.name == QueryKey.code ||
            item.name == QueryKey.connectionCode ||
            item.name == QueryKey.connectionCodeSnakeCase
    }
}

enum ConnectivityState: String, Sendable {
    case connecting
    case connected
    case offline
    case unauthorized
    case locked
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
        case .locked:
            return "Locked"
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
        case .locked:
            return "faceid"
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
        case .locked:
            return "Face ID needs to refresh the secure Mac session."
        case .unpaired:
            return "Scan the Mac code or enter the device code in Settings to start syncing looper."
        }
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
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
            return "This session is stopped and can be continued."
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

enum QuickActionSettings {
    static let storageKey = "stopQuickActions"
    private static let separator: Character = ","
    private static let storageSeparator = String(separator)

    static let defaultActions: Set<QuickActionOption> = [.openSession, .continueChat]
    static let defaultStorageValue = storageValue(for: defaultActions)

    static func loadSelectedActions(
        userDefaults: UserDefaults = .standard
    ) -> Set<QuickActionOption> {
        guard let storedValue = userDefaults.object(forKey: storageKey) as? String else {
            return defaultActions
        }

        return actions(from: storedValue)
    }

    static func actions(from storageValue: String) -> Set<QuickActionOption> {
        Set(
            storageValue
                .split(separator: separator)
                .compactMap { QuickActionOption(rawValue: String($0)) }
        )
    }

    static func storageValue(for actions: Set<QuickActionOption>) -> String {
        QuickActionOption.allCases
            .filter { actions.contains($0) }
            .map(\.rawValue)
            .joined(separator: storageSeparator)
    }
}

enum SettingsSearchTarget: String, CaseIterable, Hashable, Identifiable, Sendable {
    case connection
    case continuePrompt
    case stopQuickActions
    case security
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
        case .security:
            return "App Security"
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
        case .security:
            return "Require Face ID before sessions are shown."
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
        case .security:
            return "faceid"
        case .notificationRoutes:
            return "bell.badge"
        case .completionChecks:
            return "checklist"
        }
    }

    var keywords: [String] {
        switch self {
        case .connection:
            return ["connect", "mac", "device code", "scan mac code", "pair", "link", "local network"]
        case .continuePrompt:
            return ["continue prompt", "prompt", "default prompt", "continue"]
        case .stopQuickActions:
            return ["stop quick actions", "quick actions", "stop alert", "actions"]
        case .security:
            return ["security", "face id", "faceid", "passkey", "lock", "unlock"]
        case .notificationRoutes:
            return ["notification routes", "notifications", "routes", "alerts", "push"]
        case .completionChecks:
            return ["completion checks", "completion", "checks", "rules"]
        }
    }
}

private enum SnapshotDecodingDefault {
    static let hostID = "rust-control-plane"
    static let hostName = "Looper"
    static let globalScope = "global"
}

struct HostSummary: Codable, Sendable {
    var id: String
    var name: String
    var address: String
    var isReachable: Bool
    var lastSyncedAt: String

    private enum CodingKeys: String, CodingKey {
        case id
        case name
        case address
        case isReachable
        case lastSyncedAt
    }

    init(
        id: String,
        name: String,
        address: String,
        isReachable: Bool,
        lastSyncedAt: String
    ) {
        self.id = id
        self.name = name
        self.address = address
        self.isReachable = isReachable
        self.lastSyncedAt = lastSyncedAt
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decodeIfPresent(String.self, forKey: .id) ?? SnapshotDecodingDefault.hostID
        name = try container.decodeIfPresent(String.self, forKey: .name) ?? SnapshotDecodingDefault.hostName
        address = try container.decodeIfPresent(String.self, forKey: .address) ?? ""
        isReachable = try container.decodeIfPresent(Bool.self, forKey: .isReachable) ?? false
        lastSyncedAt = try container.decodeIfPresent(String.self, forKey: .lastSyncedAt) ?? ""
    }
}

struct CompanionServerHealth: Codable, Sendable {
    var ok: Bool
    var baseURL: String
    var baseURLs: [String]
    var serverTime: String
}

enum CompanionAssistantSurface: String, Codable, CaseIterable, Identifiable, Sendable {
    case codex
    case devin
    case grokBuild = "grok-build"

    static let defaultSurface = Self.codex

    var id: String {
        rawValue
    }

    var displayTitle: String {
        switch self {
        case .codex:
            return "Codex"
        case .devin:
            return "Devin"
        case .grokBuild:
            return "Grok Build"
        }
    }
}

struct GlobalSettings: Codable, Sendable {
    var defaultPrompt: String
    var globalMode: SessionMode?
    var scope: String
    var notificationLabel: String?
    var completionCheckLabel: String?
    var completionCheckWaitForReply: Bool
    var assistantSurface: CompanionAssistantSurface

    init(
        defaultPrompt: String,
        globalMode: SessionMode?,
        scope: String,
        notificationLabel: String?,
        completionCheckLabel: String?,
        completionCheckWaitForReply: Bool,
        assistantSurface: CompanionAssistantSurface = .defaultSurface
    ) {
        self.defaultPrompt = defaultPrompt
        self.globalMode = globalMode
        self.scope = scope
        self.notificationLabel = notificationLabel
        self.completionCheckLabel = completionCheckLabel
        self.completionCheckWaitForReply = completionCheckWaitForReply
        self.assistantSurface = assistantSurface
    }

    private enum CodingKeys: String, CodingKey {
        case defaultPrompt
        case globalMode
        case scope
        case notificationLabel
        case completionCheckLabel
        case completionCheckWaitForReply
        case assistantSurface
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        defaultPrompt = try container.decodeIfPresent(String.self, forKey: .defaultPrompt) ?? ""
        globalMode = try container.decodeIfPresent(SessionMode.self, forKey: .globalMode)
        scope = try container.decodeIfPresent(String.self, forKey: .scope) ??
            SnapshotDecodingDefault.globalScope
        notificationLabel = try container.decodeIfPresent(String.self, forKey: .notificationLabel)
        completionCheckLabel = try container.decodeIfPresent(String.self, forKey: .completionCheckLabel)
        completionCheckWaitForReply = try container.decodeIfPresent(
            Bool.self,
            forKey: .completionCheckWaitForReply
        ) ?? false

        let assistantSurfaceRawValue = try container.decodeIfPresent(String.self, forKey: .assistantSurface)
        assistantSurface = assistantSurfaceRawValue
            .flatMap(CompanionAssistantSurface.init(rawValue:)) ?? .defaultSurface
    }
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
    case devin
    case cursor
    case claudeCode = "claude-code"
    case superEngineering = "super-engineering"
    case openclaw
    case grokBuild = "grok-build"

    var displayTitle: String {
        switch self {
        case .unknown:
            return "Unknown"
        case .codex:
            return "Codex"
        case .devin:
            return "Devin"
        case .cursor:
            return "Cursor"
        case .claudeCode:
            return "Claude Code"
        case .superEngineering:
            return "Super.Engineering"
        case .openclaw:
            return "OpenClaw"
        case .grokBuild:
            return "Grok Build"
        }
    }

    /// Search keywords so typing e.g. "claude" still finds matching sessions.
    var searchKeywords: [String] {
        switch self {
        case .unknown:
            return ["assistant", "agent", "cli"]
        case .codex:
            return ["codex", "openai codex"]
        case .devin:
            return ["devin", "devin desktop"]
        case .cursor:
            return ["cursor", "cursor ide"]
        case .claudeCode:
            return ["claude", "claude code", "anthropic"]
        case .superEngineering:
            return ["super", "super.engineering", "super engineering", "superengineering"]
        case .openclaw:
            return ["openclaw", "open claw", "claw"]
        case .grokBuild:
            return ["grok", "grok build", "xai"]
        }
    }

    var systemImageName: String {
        switch self {
        case .unknown:
            return "questionmark.app.dashed"
        case .codex:
            return "terminal"
        case .devin:
            return "d.square"
        case .cursor:
            return "cursorarrow.click.2"
        case .claudeCode:
            return "sparkles"
        case .superEngineering:
            return "gearshape.2"
        case .openclaw:
            return "pawprint.fill"
        case .grokBuild:
            return "sparkle"
        }
    }
}

enum SessionKind: String, Codable, Sendable {
    case project
    case instantChat = "instant-chat"

    var label: String {
        switch self {
        case .project:
            return "Project"
        case .instantChat:
            return "Instant Chat"
        }
    }

    var symbolName: String {
        switch self {
        case .project:
            return "folder"
        case .instantChat:
            return "bubble.left.and.bubble.right"
        }
    }
}

struct InstalledPluginSummary: Codable, Hashable, Sendable {
    var id: String
    var name: String
    var source: String?
}

enum SessionTaskKind: String, Codable, Sendable {
    case unknown
    case plan
    case todo
    case implementation

    var label: String {
        switch self {
        case .unknown:
            return "Unknown"
        case .plan:
            return "Plan"
        case .todo:
            return "To Do"
        case .implementation:
            return "Implementation"
        }
    }
}

struct GitRepositoryMetadata: Codable, Hashable, Sendable {
    var repositoryName: String
    var repositoryPath: String
    var remoteURL: String?
    var branch: String?
    var commit: String?
}

enum SessionSourceReferenceKind: String, Codable, Sendable {
    case cwd
    case transcript
    case git
    case pullRequest = "pull-request"
    case plugin
    case subagent
}

struct SessionSourceReference: Codable, Hashable, Sendable {
    var kind: SessionSourceReferenceKind
    var label: String
    var value: String
    var url: String?
}

struct SessionMetadata: Codable, Hashable, Sendable {
    var kind: SessionKind
    var source: String
    var sourceDisplayName: String
    var projectName: String?
    var projectPath: String?
    var taskKind: SessionTaskKind
    var transcriptAvailable: Bool
    var gitRepository: GitRepositoryMetadata?
    var pullRequestURL: String?
    var supportsSubagents: Bool
    var installedPlugins: [InstalledPluginSummary]
    var sources: [SessionSourceReference]
    var tags: [String]

    static let empty = SessionMetadata(
        kind: .instantChat,
        source: "unknown",
        sourceDisplayName: "Unknown",
        projectName: nil,
        projectPath: nil,
        taskKind: .unknown,
        transcriptAvailable: false,
        gitRepository: nil,
        pullRequestURL: nil,
        supportsSubagents: false,
        installedPlugins: [],
        sources: [],
        tags: []
    )

    private enum CodingKeys: String, CodingKey {
        case kind
        case source
        case sourceDisplayName
        case projectName
        case projectPath
        case taskKind
        case transcriptAvailable
        case gitRepository
        case pullRequestURL
        case supportsSubagents
        case installedPlugins
        case sources
        case tags
    }

    init(
        kind: SessionKind,
        source: String,
        sourceDisplayName: String,
        projectName: String?,
        projectPath: String?,
        taskKind: SessionTaskKind,
        transcriptAvailable: Bool,
        gitRepository: GitRepositoryMetadata?,
        pullRequestURL: String?,
        supportsSubagents: Bool,
        installedPlugins: [InstalledPluginSummary],
        sources: [SessionSourceReference],
        tags: [String]
    ) {
        self.kind = kind
        self.source = source
        self.sourceDisplayName = sourceDisplayName
        self.projectName = projectName
        self.projectPath = projectPath
        self.taskKind = taskKind
        self.transcriptAvailable = transcriptAvailable
        self.gitRepository = gitRepository
        self.pullRequestURL = pullRequestURL
        self.supportsSubagents = supportsSubagents
        self.installedPlugins = installedPlugins
        self.sources = sources
        self.tags = tags
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        kind = try container.decodeIfPresent(SessionKind.self, forKey: .kind) ?? .instantChat
        source = try container.decodeIfPresent(String.self, forKey: .source) ?? "unknown"
        sourceDisplayName = try container.decodeIfPresent(String.self, forKey: .sourceDisplayName) ??
            Self.displayName(forRawSource: source)
        projectName = try container.decodeIfPresent(String.self, forKey: .projectName)
        projectPath = try container.decodeIfPresent(String.self, forKey: .projectPath)
        taskKind = try container.decodeIfPresent(SessionTaskKind.self, forKey: .taskKind) ?? .unknown
        transcriptAvailable = try container.decodeIfPresent(Bool.self, forKey: .transcriptAvailable) ?? false
        gitRepository = try container.decodeIfPresent(GitRepositoryMetadata.self, forKey: .gitRepository)
        pullRequestURL = try container.decodeIfPresent(String.self, forKey: .pullRequestURL)
        supportsSubagents = try container.decodeIfPresent(Bool.self, forKey: .supportsSubagents) ?? false
        installedPlugins = try container.decodeIfPresent(
            [InstalledPluginSummary].self,
            forKey: .installedPlugins
        ) ?? []
        sources = try container.decodeIfPresent([SessionSourceReference].self, forKey: .sources) ?? []
        tags = try container.decodeIfPresent([String].self, forKey: .tags) ?? []
    }

    var displayTitle: String {
        switch kind {
        case .project:
            return projectName ?? "Project"
        case .instantChat:
            return "Instant Chat"
        }
    }

    var userFacingTags: [String] {
        let rawSourceTags = Set(["vscode", "devin-desktop", "grok-build", source])
        return tags.filter { !rawSourceTags.contains($0) }
    }

    private static func displayName(forRawSource source: String) -> String {
        switch source {
        case "vscode":
            return "Codex"
        case "devin-desktop":
            return "Devin"
        case "grok-build":
            return "Grok Build"
        case "unknown":
            return "Unknown"
        default:
            return source
                .split(separator: "-")
                .map { $0.capitalized }
                .joined(separator: " ")
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
    var metadata: SessionMetadata

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
        case metadata
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
        assistantClient: AssistantClient = .unknown,
        metadata: SessionMetadata = .empty
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
        self.metadata = metadata
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
        isArchived = try container.decodeIfPresent(Bool.self, forKey: .isArchived) ??
            (status == .archived)
        assistantClient = try container.decodeIfPresent(AssistantClient.self, forKey: .assistantClient) ?? .unknown
        metadata = try container.decodeIfPresent(SessionMetadata.self, forKey: .metadata) ?? .empty
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
    var metadata: SessionMetadata
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
        case metadata
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
        metadata: SessionMetadata = .empty,
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
        self.metadata = metadata
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
        metadata = try container.decodeIfPresent(SessionMetadata.self, forKey: .metadata) ?? .empty
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

struct GrokBuildHookStatus: Codable, Equatable, Sendable {
    var health: String
    var owner: String
    var registeredEvents: [String]
    var hooksPath: String?

    private enum CodingKeys: String, CodingKey {
        case health
        case owner
        case registeredEvents
        case hooksPath
    }
}

struct GrokBuildStatus: Codable, Equatable, Sendable {
    var hooks: GrokBuildHookStatus
    var sessionCount: Int
    var activeSessionCount: Int

    var hooksHealthTitle: String {
        hooks.health.capitalized
    }
}

struct MobileSnapshot: Codable, Sendable {
    var host: HostSummary
    var globalSettings: GlobalSettings
    var sessions: [SessionSummary]
    var surfaceSessions: [String: [SessionSummary]]
    var notifications: [NotificationDestination]
    var completionChecks: [CompletionCheckSummary]
    var grokBuild: GrokBuildStatus?

    init(
        host: HostSummary,
        globalSettings: GlobalSettings,
        sessions: [SessionSummary],
        surfaceSessions: [String: [SessionSummary]] = [:],
        notifications: [NotificationDestination],
        completionChecks: [CompletionCheckSummary],
        grokBuild: GrokBuildStatus? = nil
    ) {
        self.host = host
        self.globalSettings = globalSettings
        self.sessions = sessions
        self.surfaceSessions = surfaceSessions
        self.notifications = notifications
        self.completionChecks = completionChecks
        self.grokBuild = grokBuild
    }

    private enum CodingKeys: String, CodingKey {
        case host
        case globalSettings
        case sessions
        case surfaceSessions
        case notifications
        case completionChecks
        case grokBuild
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        host = try container.decode(HostSummary.self, forKey: .host)
        globalSettings = try container.decode(GlobalSettings.self, forKey: .globalSettings)
        sessions = try container.decode([SessionSummary].self, forKey: .sessions)
        surfaceSessions = try container.decodeIfPresent(
            [String: [SessionSummary]].self,
            forKey: .surfaceSessions
        ) ?? [:]
        notifications = try container.decodeIfPresent(
            [NotificationDestination].self,
            forKey: .notifications
        ) ?? []
        completionChecks = try container.decodeIfPresent(
            [CompletionCheckSummary].self,
            forKey: .completionChecks
        ) ?? []
        grokBuild = try container.decodeIfPresent(GrokBuildStatus.self, forKey: .grokBuild)
    }

    func visibleSnapshot(for surface: CompanionAssistantSurface) -> MobileSnapshot {
        var visibleSnapshot = self
        visibleSnapshot.globalSettings.assistantSurface = surface
        visibleSnapshot.sessions = sessions(for: surface)
        return visibleSnapshot
    }

    func sessions(for surface: CompanionAssistantSurface) -> [SessionSummary] {
        if let sessions = surfaceSessions[surface.rawValue] {
            return sessions
        }

        guard globalSettings.assistantSurface == surface else {
            return []
        }

        return sessions
    }

    var sessionsAcrossSurfaces: [SessionSummary] {
        var sessionsByID: [String: SessionSummary] = [:]
        for surface in CompanionAssistantSurface.allCases {
            for session in sessions(for: surface) {
                sessionsByID[session.id] = session
            }
        }
        return Array(sessionsByID.values)
    }

    func assistantSurface(containingSessionID sessionID: String) -> CompanionAssistantSurface? {
        CompanionAssistantSurface.allCases.first { surface in
            sessions(for: surface).contains { session in
                session.id == sessionID
            }
        }
    }
}

struct SessionSections: Sendable {
    static let empty = SessionSections(sessions: [])

    let active: [SessionSummary]
    let running: [SessionSummary]
    let waiting: [SessionSummary]
    let stopped: [SessionSummary]
    let needsAttention: [SessionSummary]
    let archived: [SessionSummary]

    init(sessions: [SessionSummary]) {
        var active: [SessionSummary] = []
        var running: [SessionSummary] = []
        var waiting: [SessionSummary] = []
        var stopped: [SessionSummary] = []
        var needsAttention: [SessionSummary] = []
        var archived: [SessionSummary] = []

        for session in sessions.sorted(by: Self.isNewerOrLowerRef) {
            if session.isArchived {
                archived.append(session)
                continue
            }

            active.append(session)

            switch session.status {
            case .active:
                running.append(session)
            case .waiting:
                waiting.append(session)
                needsAttention.append(session)
            case .stopped:
                stopped.append(session)
            case .archived:
                archived.append(session)
            }
        }

        self.active = active
        self.running = running
        self.waiting = waiting
        self.stopped = stopped
        self.needsAttention = needsAttention
        self.archived = archived
    }

    var needsAttentionCount: Int {
        needsAttention.count
    }

    private static func isNewerOrLowerRef(
        leftSession: SessionSummary,
        rightSession: SessionSummary
    ) -> Bool {
        if leftSession.lastUpdatedAt != rightSession.lastUpdatedAt {
            return leftSession.lastUpdatedAt > rightSession.lastUpdatedAt
        }

        return leftSession.ref < rightSession.ref
    }
}

enum ModelFormatting {
    private static let unavailableTimestampLabel = "Unknown"

    static func relativeTimestamp(_ value: String) -> String {
        guard let date = date(from: value) else {
            return unavailableTimestampLabel
        }

        return date.formatted(.relative(presentation: .named))
    }

    static func friendlyDateTime(_ value: String) -> String {
        guard let date = date(from: value) else {
            return unavailableTimestampLabel
        }

        return date.formatted(date: .abbreviated, time: .shortened)
    }

    static func friendlyMode(_ mode: SessionMode?) -> String {
        mode?.label ?? "Off"
    }

    private static func date(from value: String) -> Date? {
        iso8601Formatter(formatOptions: [.withInternetDateTime, .withFractionalSeconds])
            .date(from: value) ??
            iso8601Formatter(formatOptions: [.withInternetDateTime])
            .date(from: value)
    }

    private static func iso8601Formatter(
        formatOptions: ISO8601DateFormatter.Options
    ) -> ISO8601DateFormatter {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = formatOptions
        return formatter
    }
}

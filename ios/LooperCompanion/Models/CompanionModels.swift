import AppIntents
import CoreSpotlight
import Foundation
import LooperCompanionCore

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

    static func isSupportedActivityType(_ activityType: String) -> Bool {
        activityType == Self.activityType || activityType == CSSearchableItemActionType
    }

    static func appEntityIdentifier(
        sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> EntityIdentifier? {
        guard let normalizedSessionID = normalizedString(sessionID)?.nilIfEmpty else {
            return nil
        }

        return EntityIdentifier(
            for: LooperSessionEntity.self,
            identifier: LooperSessionEntityIdentifier(
                assistantSurface: assistantSurface,
                sessionID: normalizedSessionID
            ).rawValue
        )
    }

    static func configureContinuationActivity(
        _ activity: NSUserActivity,
        sessionID: String,
        assistantSurface: CompanionAssistantSurface,
        handoffBaseURL: URL?
    ) {
        guard let normalizedSessionID = normalizedString(sessionID)?.nilIfEmpty else {
            return
        }

        var userInfo = [
            UserInfoKey.sessionID: normalizedSessionID
        ]
        if let handoffWebpageURL = handoffWebpageURL(
            for: normalizedSessionID,
            handoffBaseURL: handoffBaseURL
        ) {
            userInfo[UserInfoKey.handoffWebpageURL] = handoffWebpageURL.absoluteString
            activity.webpageURL = handoffWebpageURL
        } else {
            activity.webpageURL = nil
        }
        activity.userInfo = userInfo
        activity.targetContentIdentifier = "\(TargetContentIdentifier.sessionPrefix)\(normalizedSessionID)"
        activity.persistentIdentifier = activity.targetContentIdentifier
        if #available(iOS 18.2, macOS 15.2, watchOS 11.2, tvOS 18.2, visionOS 2.2, *) {
            activity.appEntityIdentifier = appEntityIdentifier(
                sessionID: normalizedSessionID,
                assistantSurface: assistantSurface
            )
        }
        activity.title = "Looper Session \(normalizedSessionID)"
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.needsSave = true
    }

    static func handoffWebpageURL(
        for sessionID: String,
        handoffBaseURL: URL?
    ) -> URL? {
        guard let handoffBaseURL, let normalizedSessionID = normalizedString(sessionID)?.nilIfEmpty else {
            return nil
        }

        let allowedSessionCharacters = CharacterSet.urlPathAllowed.subtracting(
            CharacterSet(charactersIn: "/")
        )
        guard let encodedSessionID = normalizedSessionID.addingPercentEncoding(
            withAllowedCharacters: allowedSessionCharacters
        ) else {
            return nil
        }

        var components = URLComponents(url: handoffBaseURL, resolvingAgainstBaseURL: false)
        components?.query = nil
        components?.fragment = nil
        let trimmedPath = components?.path.trimmingCharacters(in: CharacterSet(charactersIn: "/")) ?? ""
        let normalizedPath = trimmedPath.isEmpty ? "" : "/\(trimmedPath)"
        components?.path = [
            normalizedPath,
            HandoffWebPath.handoffComponent,
            HandoffWebPath.sessionsComponent,
            encodedSessionID
        ].joined(separator: "/")
        return components?.url
    }

    static func sessionID(from activity: NSUserActivity) -> String? {
        guard isSupportedActivityType(activity.activityType) else {
            return nil
        }

        return sessionIDFromUserInfo(activity)
            ?? sessionIDFromSpotlightActivity(activity)
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
            .flatMap(normalizedSessionID(from:))
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
            .flatMap(normalizedSessionID(from:))
    }

    private static func sessionIDFromSpotlightActivity(_ activity: NSUserActivity) -> String? {
        guard activity.activityType == CSSearchableItemActionType else {
            return nil
        }

        return (activity.userInfo?[CSSearchableItemActivityIdentifier] as? String)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
            .flatMap(normalizedSessionID(from:))
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
            .flatMap(normalizedSessionID(from:))
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

        return handoffSessionPathComponents(from: url.pathComponents.filter { $0 != "/" })?
            .joined(separator: "/")
            .removingPercentEncoding?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
            .flatMap(normalizedSessionID(from:))
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
        return handoffSessionPathComponents(
            from: url.pathComponents.filter { $0 != "/" }
        ) != nil
    }

    private static func handoffSessionPathComponents(
        from pathComponents: [String]
    ) -> [String]? {
        guard pathComponents.count >= HandoffWebPath.minimumComponentCount else {
            return nil
        }

        guard let handoffIndex = pathComponents.lastIndex(of: HandoffWebPath.handoffComponent),
              pathComponents.count > handoffIndex + HandoffWebPath.sessionsComponentOffset
        else {
            return nil
        }

        let sessionsIndex = handoffIndex + 1
        guard sessionsIndex < pathComponents.count,
              pathComponents[sessionsIndex] == HandoffWebPath.sessionsComponent
        else {
            return nil
        }

        let startIndex = sessionsIndex + 1
        let sessionPathComponents = Array(pathComponents[startIndex...]).filter { !$0.isEmpty }
        return sessionPathComponents.isEmpty ? nil : sessionPathComponents
    }

    private static func normalizedSessionID(from rawSessionID: String) -> String? {
        guard let decodedSessionID = rawSessionID.removingPercentEncoding?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
        else {
            return nil
        }

        let decodedEntitySessionID = LooperSessionEntityIdentifier(rawValue: decodedSessionID)?.sessionID
            ?? decodedSessionID

        if let normalizedDevinSessionID = normalizedLegacyDevinSessionID(from: decodedEntitySessionID) {
            return normalizedDevinSessionID
        }

        return decodedEntitySessionID
    }

    private static func normalizedLegacyDevinSessionID(
        from rawSessionID: String
    ) -> String? {
        guard rawSessionID.hasPrefix(LegacySessionID.devinPrefix) else {
            return nil
        }

        let providerAndSessionID = String(rawSessionID.dropFirst(LegacySessionID.devinPrefix.count))
        let chunks = providerAndSessionID.split(
            separator: LegacySessionID.sessionPathSeparator,
            omittingEmptySubsequences: true
        )
        guard chunks.count >= 2,
              let providerID = chunks.first?.description.nilIfEmpty
        else {
            return nil
        }

        let legacySessionID = chunks
            .dropFirst()
            .joined(separator: String(LegacySessionID.sessionPathSeparator))
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty?
            .replacingOccurrences(
                of: String(LegacySessionID.sessionPathSeparator),
                with: LegacySessionID.sessionCanonicalSeparator
            )

        guard let canonicalSessionID = legacySessionID else {
            return nil
        }
        return "\(LegacySessionID.publicPrefix)\(providerID)\(LegacySessionID.providerSeparator)\(canonicalSessionID)"
    }

    private static func normalizedString(_ value: String) -> String? {
        value.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private enum HandoffWebPath {
        static let handoffComponent = "handoff"
        static let sessionsComponent = "sessions"
        static let minimumComponentCount = 3
        static let sessionsComponentOffset = 2
    }

    private enum LegacySessionID {
        static let devinPrefix = "acp/"
        static let publicPrefix = "devin:"
        static let providerSeparator = ":"
        static let sessionPathSeparator: Character = "/"
        static let sessionCanonicalSeparator = ":"
    }
}

struct LooperSiriOpenSessionRequest: Codable, Equatable {
    private static let currentVersion = 1

    let version: Int
    let sessionID: String
    let assistantSurfaceRawValue: String?
    let createdAt: Date

    init(
        sessionID: String,
        assistantSurfaceRawValue: String?,
        createdAt: Date = Date()
    ) {
        version = Self.currentVersion
        self.sessionID = sessionID
        self.assistantSurfaceRawValue = assistantSurfaceRawValue
        self.createdAt = createdAt
    }

    var assistantSurface: CompanionAssistantSurface? {
        assistantSurfaceRawValue.flatMap(CompanionAssistantSurface.init(rawValue:))
    }
}

enum LooperSiriOpenSessionRequestStore {
    private static let storageKey = "looper.pendingSiriOpenSessionRequest.v1"

    static func save(
        _ request: LooperSiriOpenSessionRequest,
        userDefaults: UserDefaults = .standard
    ) throws {
        let data = try JSONEncoder().encode(request)
        userDefaults.set(data, forKey: storageKey)
    }

    static func drain(userDefaults: UserDefaults = .standard) -> LooperSiriOpenSessionRequest? {
        guard let data = userDefaults.data(forKey: storageKey) else {
            return nil
        }
        userDefaults.removeObject(forKey: storageKey)

        do {
            return try JSONDecoder().decode(LooperSiriOpenSessionRequest.self, from: data)
        } catch {
            CompanionDiagnostics.record(
                "siri-open:pending-decode-failed error=\(error.localizedDescription)"
            )
            return nil
        }
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

    var allowsConnectionRoutePresentation: Bool {
        switch self {
        case .connected, .unauthorized, .locked:
            return true
        case .connecting, .offline, .unpaired:
            return false
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
            return [
                "connect",
                "mac",
                "device code",
                "scan mac code",
                "pair",
                "link",
                "local network",
                "route",
                "remote",
                "tailscale",
                "tailnet",
                "lan",
                "vpn",
            ]
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
    var grpcAddress: String
    var grpcAddresses: [String]
    var isReachable: Bool
    var lastSyncedAt: String

    private enum CodingKeys: String, CodingKey {
        case id
        case name
        case address
        case grpcAddress
        case grpcAddresses
        case isReachable
        case lastSyncedAt
    }

    init(
        id: String,
        name: String,
        address: String,
        grpcAddress: String = "",
        grpcAddresses: [String] = [],
        isReachable: Bool,
        lastSyncedAt: String
    ) {
        self.id = id
        self.name = name
        self.address = address
        self.grpcAddress = grpcAddress
        self.grpcAddresses = grpcAddresses
        self.isReachable = isReachable
        self.lastSyncedAt = lastSyncedAt
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decodeIfPresent(String.self, forKey: .id) ?? SnapshotDecodingDefault.hostID
        name = try container.decodeIfPresent(String.self, forKey: .name) ?? SnapshotDecodingDefault.hostName
        address = try container.decodeIfPresent(String.self, forKey: .address) ?? ""
        grpcAddress = try container.decodeIfPresent(String.self, forKey: .grpcAddress) ?? ""
        grpcAddresses = try container.decodeIfPresent([String].self, forKey: .grpcAddresses) ?? []
        isReachable = try container.decodeIfPresent(Bool.self, forKey: .isReachable) ?? false
        lastSyncedAt = try container.decodeIfPresent(String.self, forKey: .lastSyncedAt) ?? ""
    }
}

struct CompanionTailscaleStatus: Codable, Sendable {
    var available: Bool
    var running: Bool
    var backendState: String?
    var source: String?
    var version: String?
    var hostname: String?
    var dnsName: String?
    var tailnetName: String?
    var magicDNSSuffix: String?
    var magicDNSEnabled: Bool?
    var ipAddresses: [String]
    var baseURL: String?
    var grpcBaseURL: String?
    var health: [String]
    var error: String?

    var statusLabel: String {
        if running {
            return "Running"
        }

        return available ? "Available" : "Not Detected"
    }

    var detailLabel: String {
        let primaryName = dnsName ?? hostname ?? tailnetName
        let primaryAddress = baseURL ?? ipAddresses.first
        return [primaryName, primaryAddress]
            .compactMap { value in
                value?.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty
            }
            .joined(separator: " ")
    }

    private enum CodingKeys: String, CodingKey {
        case available
        case running
        case backendState
        case source
        case version
        case hostname
        case dnsName
        case tailnetName
        case magicDNSSuffix
        case magicDNSEnabled
        case ipAddresses
        case baseURL
        case grpcBaseURL
        case health
        case error
    }

    init(
        available: Bool,
        running: Bool,
        backendState: String? = nil,
        source: String? = nil,
        version: String? = nil,
        hostname: String? = nil,
        dnsName: String? = nil,
        tailnetName: String? = nil,
        magicDNSSuffix: String? = nil,
        magicDNSEnabled: Bool? = nil,
        ipAddresses: [String] = [],
        baseURL: String? = nil,
        grpcBaseURL: String? = nil,
        health: [String] = [],
        error: String? = nil
    ) {
        self.available = available
        self.running = running
        self.backendState = backendState
        self.source = source
        self.version = version
        self.hostname = hostname
        self.dnsName = dnsName
        self.tailnetName = tailnetName
        self.magicDNSSuffix = magicDNSSuffix
        self.magicDNSEnabled = magicDNSEnabled
        self.ipAddresses = ipAddresses
        self.baseURL = baseURL
        self.grpcBaseURL = grpcBaseURL
        self.health = health
        self.error = error
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        available = try container.decodeIfPresent(Bool.self, forKey: .available) ?? false
        running = try container.decodeIfPresent(Bool.self, forKey: .running) ?? false
        backendState = try container.decodeIfPresent(String.self, forKey: .backendState)
        source = try container.decodeIfPresent(String.self, forKey: .source)
        version = try container.decodeIfPresent(String.self, forKey: .version)
        hostname = try container.decodeIfPresent(String.self, forKey: .hostname)
        dnsName = try container.decodeIfPresent(String.self, forKey: .dnsName)
        tailnetName = try container.decodeIfPresent(String.self, forKey: .tailnetName)
        magicDNSSuffix = try container.decodeIfPresent(String.self, forKey: .magicDNSSuffix)
        magicDNSEnabled = try container.decodeIfPresent(Bool.self, forKey: .magicDNSEnabled)
        ipAddresses = try container.decodeIfPresent([String].self, forKey: .ipAddresses) ?? []
        baseURL = try container.decodeIfPresent(String.self, forKey: .baseURL)
        grpcBaseURL = try container.decodeIfPresent(String.self, forKey: .grpcBaseURL)
        health = try container.decodeIfPresent([String].self, forKey: .health) ?? []
        error = try container.decodeIfPresent(String.self, forKey: .error)
    }
}

struct CompanionServerHealth: Codable, Sendable {
    var ok: Bool
    var baseURL: String
    var baseURLs: [String]
    var grpcBaseURL: String
    var grpcBaseURLs: [String]
    var serverTime: String
    var tailscale: CompanionTailscaleStatus?

    private enum CodingKeys: String, CodingKey {
        case ok
        case baseURL
        case baseURLs
        case grpcBaseURL
        case grpcBaseURLs
        case serverTime
        case tailscale
    }

    init(
        ok: Bool,
        baseURL: String,
        baseURLs: [String],
        grpcBaseURL: String = "",
        grpcBaseURLs: [String] = [],
        serverTime: String,
        tailscale: CompanionTailscaleStatus? = nil
    ) {
        self.ok = ok
        self.baseURL = baseURL
        self.baseURLs = baseURLs
        self.grpcBaseURL = grpcBaseURL
        self.grpcBaseURLs = grpcBaseURLs
        self.serverTime = serverTime
        self.tailscale = tailscale
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        ok = try container.decodeIfPresent(Bool.self, forKey: .ok) ?? false
        baseURL = try container.decodeIfPresent(String.self, forKey: .baseURL) ?? ""
        baseURLs = try container.decodeIfPresent([String].self, forKey: .baseURLs) ?? []
        grpcBaseURL = try container.decodeIfPresent(String.self, forKey: .grpcBaseURL) ?? ""
        grpcBaseURLs = try container.decodeIfPresent([String].self, forKey: .grpcBaseURLs) ?? []
        serverTime = try container.decodeIfPresent(String.self, forKey: .serverTime) ?? ""
        tailscale = try container.decodeIfPresent(CompanionTailscaleStatus.self, forKey: .tailscale)
    }
}

enum CompanionAssistantSurface: String, Codable, CaseIterable, Identifiable, Sendable {
    case codex
    case claudeCode = "claude-code"
    case devin
    case grokBuild = "grok-build"
    case zed

    static let defaultSurface = Self.codex

    var id: String {
        rawValue
    }

    var displayTitle: String {
        switch self {
        case .codex:
            return "Codex"
        case .claudeCode:
            return "Claude Code"
        case .devin:
            return "Devin"
        case .grokBuild:
            return "Grok Build"
        case .zed:
            return "Zed"
        }
    }
}

struct LooperSessionEntityIdentifier: Hashable, Sendable {
    let assistantSurface: CompanionAssistantSurface
    let sessionID: String

    var rawValue: String {
        LooperSiriEntityIdentifier(
            assistantSurface: assistantSurface.rawValue,
            sessionID: sessionID
        ).rawValue
    }

    init(assistantSurface: CompanionAssistantSurface, sessionID: String) {
        self.assistantSurface = assistantSurface
        self.sessionID = sessionID
    }

    init?(rawValue: String) {
        guard let identifier = LooperSiriEntityIdentifier(rawValue: rawValue),
              let assistantSurface = CompanionAssistantSurface(rawValue: identifier.assistantSurface)
        else {
            return nil
        }

        self.assistantSurface = assistantSurface
        sessionID = identifier.sessionID
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
    var siriDefaultSessionId: String?
    var siriDefaultAssistantSurface: CompanionAssistantSurface?
    var siriCurrentSessionId: String?
    var siriCurrentAssistantSurface: CompanionAssistantSurface?
    var siriCurrentUpdatedAtMs: Int64?

    init(
        defaultPrompt: String,
        globalMode: SessionMode?,
        scope: String,
        notificationLabel: String?,
        completionCheckLabel: String?,
        completionCheckWaitForReply: Bool,
        assistantSurface: CompanionAssistantSurface = .defaultSurface,
        siriDefaultSessionId: String? = nil,
        siriDefaultAssistantSurface: CompanionAssistantSurface? = nil,
        siriCurrentSessionId: String? = nil,
        siriCurrentAssistantSurface: CompanionAssistantSurface? = nil,
        siriCurrentUpdatedAtMs: Int64? = nil
    ) {
        self.defaultPrompt = defaultPrompt
        self.globalMode = globalMode
        self.scope = scope
        self.notificationLabel = notificationLabel
        self.completionCheckLabel = completionCheckLabel
        self.completionCheckWaitForReply = completionCheckWaitForReply
        self.assistantSurface = assistantSurface
        self.siriDefaultSessionId = siriDefaultSessionId
        self.siriDefaultAssistantSurface = siriDefaultAssistantSurface
        self.siriCurrentSessionId = siriCurrentSessionId
        self.siriCurrentAssistantSurface = siriCurrentAssistantSurface
        self.siriCurrentUpdatedAtMs = siriCurrentUpdatedAtMs
    }

    private enum CodingKeys: String, CodingKey {
        case defaultPrompt
        case globalMode
        case scope
        case notificationLabel
        case completionCheckLabel
        case completionCheckWaitForReply
        case assistantSurface
        case siriDefaultSessionId
        case siriDefaultAssistantSurface
        case siriCurrentSessionId
        case siriCurrentAssistantSurface
        case siriCurrentUpdatedAtMs
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
        siriDefaultSessionId = try container.decodeIfPresent(String.self, forKey: .siriDefaultSessionId)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
        let siriDefaultAssistantSurfaceRawValue = try container.decodeIfPresent(
            String.self,
            forKey: .siriDefaultAssistantSurface
        )
        siriDefaultAssistantSurface = siriDefaultAssistantSurfaceRawValue
            .flatMap(CompanionAssistantSurface.init(rawValue:))
        siriCurrentSessionId = try container.decodeIfPresent(String.self, forKey: .siriCurrentSessionId)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
        let siriCurrentAssistantSurfaceRawValue = try container.decodeIfPresent(
            String.self,
            forKey: .siriCurrentAssistantSurface
        )
        siriCurrentAssistantSurface = siriCurrentAssistantSurfaceRawValue
            .flatMap(CompanionAssistantSurface.init(rawValue:))
        siriCurrentUpdatedAtMs = try container.decodeIfPresent(Int64.self, forKey: .siriCurrentUpdatedAtMs)
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

/// Coding agent surface inferred on the Mac from cwd / transcript paths.
enum AssistantClient: String, Codable, Sendable, CaseIterable, Hashable {
    case unknown
    case codex
    case devin
    case cursor
    case claudeCode = "claude-code"
    case superEngineering = "super-engineering"
    case openclaw
    case grokBuild = "grok-build"
    case zed

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
        case .zed:
            return "Zed"
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
        case .zed:
            return ["zed", "zed acp", "zed external agents"]
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
        case .zed:
            return "bolt.square"
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

struct SessionGoalSummary: Codable, Hashable, Sendable {
    var id: String
    var title: String
    var status: String
    var lifecycle: String
    var running: Bool
    var tokenBudget: Int?
    var tokensUsed: Int?
    var timeUsedSeconds: Int?
    var updatedAtMs: Int64?
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
        let rawSourceTags = Set(["vscode", "devin-desktop", "grok-build", "zed-agent-servers", source])
        return tags.filter { !rawSourceTags.contains($0) }
    }

    private static func displayName(forRawSource source: String) -> String {
        switch source {
        case "vscode":
            return "Codex"
        case "claude-code":
            return "Claude Code"
        case "devin-desktop":
            return "Devin"
        case "grok-build":
            return "Grok Build"
        case "zed":
            return "Zed"
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
    var lastActivityAt: String
    var lastMessageAt: String?
    var assistantPreview: String?
    var isArchived: Bool
    var canSendPrompt: Bool
    var promptDeliveryUnavailableReason: String?
    var assistantClient: AssistantClient
    var goal: SessionGoalSummary?
    var metadata: SessionMetadata

    private enum CodingKeys: String, CodingKey {
        case id
        case ref
        case title
        case status
        case effectiveMode
        case lastUpdatedAt
        case lastActivityAt
        case lastMessageAt
        case assistantPreview
        case isArchived
        case canSendPrompt
        case promptDeliveryUnavailableReason
        case assistantClient
        case goal
        case metadata
    }

    init(
        id: String,
        ref: String,
        title: String,
        status: SessionStatus,
        effectiveMode: SessionMode?,
        lastUpdatedAt: String,
        lastActivityAt: String? = nil,
        lastMessageAt: String? = nil,
        assistantPreview: String?,
        isArchived: Bool,
        canSendPrompt: Bool = true,
        promptDeliveryUnavailableReason: String? = nil,
        assistantClient: AssistantClient = .unknown,
        goal: SessionGoalSummary? = nil,
        metadata: SessionMetadata = .empty
    ) {
        self.id = id
        self.ref = ref
        self.title = title
        self.status = status
        self.effectiveMode = effectiveMode
        self.lastUpdatedAt = lastUpdatedAt
        self.lastActivityAt = lastActivityAt ?? lastUpdatedAt
        self.lastMessageAt = lastMessageAt
        self.assistantPreview = assistantPreview
        self.isArchived = isArchived
        self.canSendPrompt = canSendPrompt
        self.promptDeliveryUnavailableReason = promptDeliveryUnavailableReason
        self.assistantClient = assistantClient
        self.goal = goal
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
        lastActivityAt = try container.decodeIfPresent(String.self, forKey: .lastActivityAt) ?? lastUpdatedAt
        lastMessageAt = try container.decodeIfPresent(String.self, forKey: .lastMessageAt)
        assistantPreview = try container.decodeIfPresent(String.self, forKey: .assistantPreview)
        isArchived = try container.decodeIfPresent(Bool.self, forKey: .isArchived) ??
            (status == .archived)
        canSendPrompt = try container.decodeIfPresent(Bool.self, forKey: .canSendPrompt) ?? true
        promptDeliveryUnavailableReason = try container.decodeIfPresent(
            String.self,
            forKey: .promptDeliveryUnavailableReason
        )
        assistantClient = try container.decodeIfPresent(AssistantClient.self, forKey: .assistantClient) ?? .unknown
        goal = try container.decodeIfPresent(SessionGoalSummary.self, forKey: .goal)
        metadata = try container.decodeIfPresent(SessionMetadata.self, forKey: .metadata) ?? .empty
    }
}

extension SessionSummary {
    var lastUpdatedDate: Date? {
        SessionTimestampParser.date(from: lastUpdatedAt)
    }

    var lastActivityDate: Date? {
        LooperSessionFreshness.date(from: lastActivityAt)
    }

    var lastMessageDate: Date? {
        lastMessageAt.flatMap(LooperSessionFreshness.date)
    }

    var displayFreshnessAt: String {
        LooperSessionFreshness.displayTimestamp(lastActivityAt: lastActivityAt)
    }

    var displayFreshnessDate: Date? {
        LooperSessionFreshness.displayDate(lastActivityAt: lastActivityAt)
    }

    var displayFreshnessPrefix: String {
        LooperSessionFreshness.displayPrefix()
    }

    static func isNewerOrLowerRef(
        leftSession: SessionSummary,
        rightSession: SessionSummary
    ) -> Bool {
        LooperSessionFreshness.isNewerActivityOrLowerReference(
            leftLastActivityAt: leftSession.lastActivityAt,
            leftRef: leftSession.ref,
            rightLastActivityAt: rightSession.lastActivityAt,
            rightRef: rightSession.ref
        )
    }
}

struct SessionDetail: Codable, Identifiable, Sendable {
    var id: String
    var ref: String
    var title: String
    var status: SessionStatus
    var effectiveMode: SessionMode?
    var lastUpdatedAt: String
    var lastActivityAt: String
    var lastMessageAt: String?
    var assistantPreview: String?
    var latestAssistantMessage: String?
    var isArchived: Bool
    var canSendPrompt: Bool
    var promptDeliveryUnavailableReason: String?
    var assistantClient: AssistantClient
    var goal: SessionGoalSummary?
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
        case lastActivityAt
        case lastMessageAt
        case assistantPreview
        case latestAssistantMessage
        case isArchived
        case canSendPrompt
        case promptDeliveryUnavailableReason
        case assistantClient
        case goal
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
        lastActivityAt: String? = nil,
        lastMessageAt: String? = nil,
        assistantPreview: String?,
        latestAssistantMessage: String?,
        isArchived: Bool,
        canSendPrompt: Bool = true,
        promptDeliveryUnavailableReason: String? = nil,
        assistantClient: AssistantClient = .unknown,
        goal: SessionGoalSummary? = nil,
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
        self.lastActivityAt = lastActivityAt ?? lastUpdatedAt
        self.lastMessageAt = lastMessageAt
        self.assistantPreview = assistantPreview
        self.latestAssistantMessage = latestAssistantMessage
        self.isArchived = isArchived
        self.canSendPrompt = canSendPrompt
        self.promptDeliveryUnavailableReason = promptDeliveryUnavailableReason
        self.assistantClient = assistantClient
        self.goal = goal
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
        lastActivityAt = try container.decodeIfPresent(String.self, forKey: .lastActivityAt) ?? lastUpdatedAt
        lastMessageAt = try container.decodeIfPresent(String.self, forKey: .lastMessageAt)
        assistantPreview = try container.decodeIfPresent(String.self, forKey: .assistantPreview)
        latestAssistantMessage = try container.decodeIfPresent(String.self, forKey: .latestAssistantMessage)
        isArchived = try container.decode(Bool.self, forKey: .isArchived)
        canSendPrompt = try container.decodeIfPresent(Bool.self, forKey: .canSendPrompt) ?? true
        promptDeliveryUnavailableReason = try container.decodeIfPresent(
            String.self,
            forKey: .promptDeliveryUnavailableReason
        )
        assistantClient = try container.decodeIfPresent(AssistantClient.self, forKey: .assistantClient) ?? .unknown
        goal = try container.decodeIfPresent(SessionGoalSummary.self, forKey: .goal)
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

struct DevinDesktopStatus: Codable, Equatable, Sendable {
    var running: Bool
    var installed: Bool
    var acpAvailable: Bool
    var registryExists: Bool
    var registryAgentCount: Int
    var enabledAgentCount: Int
    var preferredAgentIds: [String]
    var sessionCount: Int
    var activeSessionCount: Int

    var connectionTitle: String {
        if running {
            return "running"
        }
        if installed {
            return "installed"
        }
        return "not installed"
    }
}

struct MobileSnapshot: Codable, Sendable {
    var revision: String?
    var host: HostSummary
    var globalSettings: GlobalSettings
    var sessions: [SessionSummary]
    var surfaceSessions: [String: [SessionSummary]]
    var notifications: [NotificationDestination]
    var completionChecks: [CompletionCheckSummary]
    var devinDesktop: DevinDesktopStatus?
    var grokBuild: GrokBuildStatus?

    init(
        revision: String? = nil,
        host: HostSummary,
        globalSettings: GlobalSettings,
        sessions: [SessionSummary],
        surfaceSessions: [String: [SessionSummary]] = [:],
        notifications: [NotificationDestination],
        completionChecks: [CompletionCheckSummary],
        devinDesktop: DevinDesktopStatus? = nil,
        grokBuild: GrokBuildStatus? = nil
    ) {
        self.revision = revision
        self.host = host
        self.globalSettings = globalSettings
        self.sessions = sessions
        self.surfaceSessions = surfaceSessions
        self.notifications = notifications
        self.completionChecks = completionChecks
        self.devinDesktop = devinDesktop
        self.grokBuild = grokBuild
    }

    private enum CodingKeys: String, CodingKey {
        case revision
        case host
        case globalSettings
        case sessions
        case surfaceSessions
        case notifications
        case completionChecks
        case devinDesktop
        case grokBuild
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        revision = try container.decodeIfPresent(String.self, forKey: .revision)
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
        devinDesktop = try container.decodeIfPresent(DevinDesktopStatus.self, forKey: .devinDesktop)
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
                if let existingSession = sessionsByID[session.id] {
                    if SessionSummary.isNewerOrLowerRef(
                        leftSession: session,
                        rightSession: existingSession
                    ) {
                        sessionsByID[session.id] = session
                    }
                } else {
                    sessionsByID[session.id] = session
                }
            }
        }
        return sessionsByID.values.sorted(by: SessionSummary.isNewerOrLowerRef)
    }

    func session(withID sessionID: String) -> SessionSummary? {
        if let session = sessions.first(where: { session in session.id == sessionID }) {
            return session
        }

        return sessionsAcrossSurfaces.first { session in
            session.id == sessionID
        }
    }

    func assistantSurface(containingSessionID sessionID: String) -> CompanionAssistantSurface? {
        CompanionAssistantSurface.allCases.first { surface in
            sessions(for: surface).contains { session in
                session.id == sessionID
            }
        }
    }
}

struct SessionIndex: Equatable, Sendable {
    static let empty = SessionIndex(
        allSessions: [],
        sessionsByID: [:],
        surfaceBySessionID: [:],
        identity: "empty"
    )

    let allSessions: [SessionSummary]
    private let sessionsByID: [String: SessionSummary]
    private let surfaceBySessionID: [String: CompanionAssistantSurface]
    let identity: String

    init(snapshot: MobileSnapshot) {
        var sessionsByID: [String: SessionSummary] = [:]
        var surfaceBySessionID: [String: CompanionAssistantSurface] = [:]

        for surface in CompanionAssistantSurface.allCases {
            for session in snapshot.sessions(for: surface) {
                if let existingSession = sessionsByID[session.id],
                   !SessionSummary.isNewerOrLowerRef(
                       leftSession: session,
                       rightSession: existingSession
                   )
                {
                    continue
                }

                sessionsByID[session.id] = session
                surfaceBySessionID[session.id] = surface
            }
        }

        let allSessions = sessionsByID.values.sorted(by: SessionSummary.isNewerOrLowerRef)
        self.init(
            allSessions: allSessions,
            sessionsByID: sessionsByID,
            surfaceBySessionID: surfaceBySessionID,
            identity: Self.identity(snapshot: snapshot, sessions: allSessions)
        )
    }

    private init(
        allSessions: [SessionSummary],
        sessionsByID: [String: SessionSummary],
        surfaceBySessionID: [String: CompanionAssistantSurface],
        identity: String
    ) {
        self.allSessions = allSessions
        self.sessionsByID = sessionsByID
        self.surfaceBySessionID = surfaceBySessionID
        self.identity = identity
    }

    func session(withID sessionID: String) -> SessionSummary? {
        sessionsByID[sessionID]
    }

    func assistantSurface(containingSessionID sessionID: String) -> CompanionAssistantSurface? {
        surfaceBySessionID[sessionID]
    }

    private static func identity(snapshot: MobileSnapshot, sessions: [SessionSummary]) -> String {
        let revision = snapshot.revision?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty ?? "no-revision"
        let sessionFingerprints = sessions.map { session in
            [
                session.id,
                session.status.rawValue,
                session.lastActivityAt,
                session.lastMessageAt ?? "",
                session.isArchived ? "archived" : "visible",
            ].joined(separator: ":")
        }
        return ([revision, String(sessions.count)] + sessionFingerprints).joined(separator: "|")
    }
}

enum SessionDisplayPolicy {
    static let collapsedSectionLimit = 40
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
        SessionSummary.isNewerOrLowerRef(leftSession: leftSession, rightSession: rightSession)
    }
}

enum SessionTimestampParser {
    static func date(from value: String) -> Date? {
        LooperSessionFreshness.date(from: value)
    }
}

enum ModelFormatting {
    private static let unavailableTimestampLabel = "Unknown"

    static func relativeTimestamp(_ value: String) -> String {
        guard let date = SessionTimestampParser.date(from: value) else {
            return unavailableTimestampLabel
        }

        return date.formatted(.relative(presentation: .named))
    }

    static func sessionFreshness(_ session: SessionSummary) -> String {
        "\(session.displayFreshnessPrefix) \(relativeTimestamp(session.displayFreshnessAt))"
    }

    static func friendlyDateTime(_ value: String) -> String {
        guard let date = SessionTimestampParser.date(from: value) else {
            return unavailableTimestampLabel
        }

        return date.formatted(date: .abbreviated, time: .shortened)
    }

    static func friendlyMode(_ mode: SessionMode?) -> String {
        mode?.label ?? "Off"
    }
}

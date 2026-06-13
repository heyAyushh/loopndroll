import Foundation

public enum LooperContinuationActivity {
    public static let activityType = "dev.looper.app.continue-session"
    public static let persistentIdentifier = "dev.looper.app.continuation.current-session"
    fileprivate static let sessionTargetContentIdentifierPrefix = "looper.session."

    public enum UserInfoKey {
        public static let kind = "kind"
        public static let sessionID = "sessionID"
        public static let sessionTitle = "sessionTitle"
        public static let sessionSubtitle = "sessionSubtitle"
        public static let sessionPreview = "sessionPreview"
        public static let handoffWebpageURL = "handoffWebpageURL"
        public static let updatedAtMilliseconds = "updatedAtMilliseconds"
    }

    public static func isSupportedActivityType(_ activityType: String) -> Bool {
        activityType == Self.activityType
    }

    public static func sessionID(from activity: NSUserActivity) -> String? {
        guard isSupportedActivityType(activity.activityType) else {
            return nil
        }

        if let sessionID = normalizedString(activity.userInfo?[UserInfoKey.sessionID] as? String) {
            return sessionID
        }

        guard let targetContentIdentifier = normalizedString(activity.targetContentIdentifier),
              targetContentIdentifier.hasPrefix(sessionTargetContentIdentifierPrefix)
        else {
            return nil
        }

        return normalizedString(
            String(targetContentIdentifier.dropFirst(sessionTargetContentIdentifierPrefix.count))
        )
    }

    private static func normalizedString(_ value: String?) -> String? {
        let trimmedValue = value?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedValue, !trimmedValue.isEmpty else {
            return nil
        }
        return trimmedValue
    }
}

public enum LooperHandoffFocusAssist: String, CaseIterable, Identifiable, Sendable {
    case afterThirtyIdleSeconds
    case afterOneIdleMinute
    case afterTwoIdleMinutes
    case afterFiveIdleMinutes
    case afterFifteenIdleMinutes
    case rightNow
    case never

    private enum Timing {
        static let secondsPerMinute: TimeInterval = 60
        static let thirtyIdleSeconds: TimeInterval = 30
        static let oneIdleMinute: TimeInterval = 1
        static let twoIdleMinutes: TimeInterval = 2
        static let fiveIdleMinutes: TimeInterval = 5
        static let fifteenIdleMinutes: TimeInterval = 15
    }

    public static let userDefaultsKey = "handoffFocusAssist"
    public static let defaultOption: LooperHandoffFocusAssist = .afterFiveIdleMinutes

    public var id: String {
        rawValue
    }

    public var idleThresholdSeconds: TimeInterval? {
        switch self {
        case .afterThirtyIdleSeconds:
            Timing.thirtyIdleSeconds
        case .afterOneIdleMinute:
            Timing.oneIdleMinute * Timing.secondsPerMinute
        case .afterTwoIdleMinutes:
            Timing.twoIdleMinutes * Timing.secondsPerMinute
        case .afterFiveIdleMinutes:
            Timing.fiveIdleMinutes * Timing.secondsPerMinute
        case .afterFifteenIdleMinutes:
            Timing.fifteenIdleMinutes * Timing.secondsPerMinute
        case .rightNow, .never:
            nil
        }
    }

    public var activatesWithoutIdleDelay: Bool {
        self == .rightNow
    }

    public var menuTitle: String {
        switch self {
        case .afterThirtyIdleSeconds:
            "After 30s idle"
        case .afterOneIdleMinute:
            "After 1 min idle"
        case .afterTwoIdleMinutes:
            "After 2 min idle"
        case .afterFiveIdleMinutes:
            "After 5 min idle"
        case .afterFifteenIdleMinutes:
            "After 15 min idle"
        case .rightNow:
            "Right now"
        case .never:
            "Never"
        }
    }

    public var statusTitle: String {
        switch self {
        case .rightNow, .afterThirtyIdleSeconds, .afterOneIdleMinute, .afterTwoIdleMinutes,
             .afterFiveIdleMinutes, .afterFifteenIdleMinutes:
            menuTitle
        case .never:
            "Off"
        }
    }

    public static func stored(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) -> LooperHandoffFocusAssist {
        guard let storedValue = userDefaults.string(forKey: key),
              let option = LooperHandoffFocusAssist(rawValue: storedValue)
        else {
            return defaultOption
        }

        return option
    }

    public func save(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) {
        userDefaults.set(rawValue, forKey: key)
    }
}

public struct LooperContinuationActivityDescriptor: Equatable, Sendable {
    public let title: String
    public let targetContentIdentifier: String
    public let userInfo: [String: String]
}

public enum LooperContinuationActivityBuilder {
    private static let genericActivityKind = "app"
    private static let sessionActivityKind = "session"
    private static let genericActivityTitle = "looper"
    private static let genericTargetContentIdentifier = "looper"
    private static let pathSeparator = "/"
    private static let handoffPathComponent = "handoff"
    private static let sessionsPathComponent = "sessions"
    private static let pathSegmentReservedCharacters = CharacterSet(charactersIn: "/")
    private static let pathSegmentAllowedCharacters = CharacterSet.urlPathAllowed
        .subtracting(pathSegmentReservedCharacters)

    public static func descriptor(
        from snapshot: DesktopSnapshotResponse,
        handoffBaseURL: URL? = nil
    ) -> LooperContinuationActivityDescriptor {
        guard let thread = continuationThread(from: snapshot.threads) else {
            return genericDescriptor()
        }

        let title = titleText(for: thread)
        let subtitle = subtitleText(for: thread)
        let webpageURL = handoffBaseURL.map { handoffWebpageURL(baseURL: $0, threadID: thread.threadId) }
        var userInfo = [
            LooperContinuationActivity.UserInfoKey.kind: sessionActivityKind,
            LooperContinuationActivity.UserInfoKey.sessionID: thread.threadId,
            LooperContinuationActivity.UserInfoKey.sessionTitle: title,
            LooperContinuationActivity.UserInfoKey.sessionSubtitle: subtitle,
        ]

        if let updatedAtMs = thread.updatedAtMs {
            userInfo[LooperContinuationActivity.UserInfoKey.updatedAtMilliseconds] = String(updatedAtMs)
        }

        if let webpageURL {
            userInfo[LooperContinuationActivity.UserInfoKey.handoffWebpageURL] = webpageURL.absoluteString
        }

        if let assistantPreview = thread.assistantPreview?.trimmingCharacters(in: .whitespacesAndNewlines),
           !assistantPreview.isEmpty
        {
            userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview] = assistantPreview
        }

        return LooperContinuationActivityDescriptor(
            title: title,
            targetContentIdentifier: "\(LooperContinuationActivity.sessionTargetContentIdentifierPrefix)\(thread.threadId)",
            userInfo: userInfo
        )
    }

    public static func genericDescriptor() -> LooperContinuationActivityDescriptor {
        LooperContinuationActivityDescriptor(
            title: genericActivityTitle,
            targetContentIdentifier: genericTargetContentIdentifier,
            userInfo: [
                LooperContinuationActivity.UserInfoKey.kind: genericActivityKind,
            ]
        )
    }

    private static func continuationThread(from threads: [DesktopThreadSummary]) -> DesktopThreadSummary? {
        let activeThreads = threads.filter { !$0.archived }

        return newestThread(from: activeThreads)
            ?? newestThread(from: threads)
    }

    private static func newestThread(from threads: [DesktopThreadSummary]) -> DesktopThreadSummary? {
        threads.max { left, right in
            timestamp(for: left) < timestamp(for: right)
        }
    }

    private static func timestamp(for thread: DesktopThreadSummary) -> Int64 {
        thread.updatedAtMs ?? Int64.min
    }

    private static func titleText(for thread: DesktopThreadSummary) -> String {
        let title = thread.title?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let title, !title.isEmpty {
            return title
        }
        return thread.threadId
    }

    private static func subtitleText(for thread: DesktopThreadSummary) -> String {
        let fallback = thread.source ?? thread.capabilities.spawn.launchKind
        return ProjectNameResolver.displayName(
            forWorkingDirectory: thread.cwd,
            fallback: fallback
        )
    }

    private static func handoffWebpageURL(baseURL: URL, threadID: String) -> URL {
        guard let encodedThreadID = threadID.addingPercentEncoding(
            withAllowedCharacters: pathSegmentAllowedCharacters
        ), !encodedThreadID.isEmpty,
              var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false)
        else {
            return baseURL
        }

        let basePath = components.percentEncodedPath.trimmingCharacters(
            in: pathSegmentReservedCharacters
        )
        components.percentEncodedPath = [
            basePath,
            handoffPathComponent,
            sessionsPathComponent,
            encodedThreadID,
        ]
        .filter { !$0.isEmpty }
        .joined(separator: pathSeparator)
        .withLeadingPathSeparator
        return components.url ?? baseURL
    }
}

private extension String {
    var withLeadingPathSeparator: String {
        hasPrefix("/") ? self : "/\(self)"
    }
}

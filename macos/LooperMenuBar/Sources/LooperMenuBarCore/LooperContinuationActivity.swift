import Foundation

public enum LooperContinuationActivity {
    public static let activityType = "dev.looper.app.continue-session"
    public static let persistentIdentifier = "dev.looper.app.continuation.current-session"

    public enum UserInfoKey {
        public static let kind = "kind"
        public static let sessionID = "sessionID"
        public static let sessionTitle = "sessionTitle"
        public static let sessionSubtitle = "sessionSubtitle"
        public static let sessionPreview = "sessionPreview"
        public static let handoffWebpageURL = "handoffWebpageURL"
        public static let updatedAtMilliseconds = "updatedAtMilliseconds"
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
    private static let sessionTargetContentIdentifierPrefix = "looper.session."
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
            targetContentIdentifier: "\(sessionTargetContentIdentifierPrefix)\(thread.threadId)",
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
        newestThread(from: threads.filter { !$0.archived }) ?? newestThread(from: threads)
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

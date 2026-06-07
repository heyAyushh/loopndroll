import AppKit
import CoreSpotlight
import LooperMenuBarCore
import OSLog

@MainActor
final class LooperContinuationActivityPublisher {
    private enum ActivityRefresh {
        static let currentActivityRefreshInterval: Duration = .seconds(3)
    }

    private enum Logging {
        static let subsystem = "dev.looper.app.ios"
        static let category = "handoff"
        static let missingValue = "none"
    }

    private static let persistentActivityIdentifier = NSUserActivityPersistentIdentifier(
        LooperContinuationActivity.persistentIdentifier
    )

    private let logger = Logger(subsystem: Logging.subsystem, category: Logging.category)
    private weak var activityHost: NSResponder?
    private var currentActivity: NSUserActivity?
    private var currentDescriptor: LooperContinuationActivityDescriptor?
    private var currentActivityRefreshTask: Task<Void, Never>?

    func attachHost(_ host: NSResponder?) {
        activityHost?.userActivity = nil
        activityHost = host
        logger.info("handoff host attached host=\(self.hostClassName(for: host), privacy: .public)")
        if let currentActivity {
            host?.userActivity = currentActivity
            refreshCurrentActivity()
        }
    }

    func publish(_ descriptor: LooperContinuationActivityDescriptor) {
        guard descriptor != currentDescriptor else {
            refreshCurrentActivity()
            return
        }

        let activity = currentActivity ?? NSUserActivity(activityType: LooperContinuationActivity.activityType)
        configure(activity, with: descriptor)
        activityHost?.userActivity = activity
        markActivityCurrent(activity)
        logPublishedActivity(activity, descriptor: descriptor)

        currentActivity = activity
        currentDescriptor = descriptor
        startCurrentActivityRefreshLoop()
    }

    func publishFallbackIfIdle(_ descriptor: LooperContinuationActivityDescriptor) {
        guard currentActivity != nil else {
            publish(descriptor)
            return
        }

        refreshCurrentActivity()
    }

    func invalidate() {
        currentActivityRefreshTask?.cancel()
        currentActivity?.invalidate()
        activityHost?.userActivity = nil
        logger.info("handoff activity invalidated")
        currentActivityRefreshTask = nil
        currentActivity = nil
        currentDescriptor = nil
    }

    private func startCurrentActivityRefreshLoop() {
        guard currentActivityRefreshTask == nil else {
            return
        }

        currentActivityRefreshTask = Task { @MainActor [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: ActivityRefresh.currentActivityRefreshInterval)
                self?.refreshCurrentActivity()
            }
        }
    }

    private func refreshCurrentActivity() {
        guard let currentActivity else {
            return
        }

        if let currentDescriptor {
            configure(currentActivity, with: currentDescriptor)
        }
        activityHost?.userActivity = currentActivity
        markActivityCurrent(currentActivity)
        logger.debug("handoff activity refreshed host=\(self.hostClassName(for: self.activityHost), privacy: .public)")
    }

    private func markActivityCurrent(_ activity: NSUserActivity) {
        activity.needsSave = true
        activity.becomeCurrent()
    }

    private func configure(
        _ activity: NSUserActivity,
        with descriptor: LooperContinuationActivityDescriptor
    ) {
        activity.title = activityTitle(for: descriptor)
        if activity.persistentIdentifier != Self.persistentActivityIdentifier {
            activity.persistentIdentifier = Self.persistentActivityIdentifier
        }
        activity.targetContentIdentifier = LooperContinuationActivity.targetContentIdentifier
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
        activity.keywords = activityKeywords(for: descriptor)
        activity.contentAttributeSet = contentAttributeSet(for: descriptor)
        activity.addUserInfoEntries(from: descriptor.userInfo)
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
    }

    private func logPublishedActivity(
        _ activity: NSUserActivity,
        descriptor: LooperContinuationActivityDescriptor
    ) {
        let kind = descriptor.userInfo[LooperContinuationActivity.UserInfoKey.kind] ?? Logging.missingValue
        let sessionID = descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionID] ?? Logging.missingValue
        let activityTarget = activity.targetContentIdentifier ?? Logging.missingValue
        logger.info(
            """
            handoff activity published kind=\(kind, privacy: .public) \
            session=\(sessionID, privacy: .public) \
            host=\(self.hostClassName(for: self.activityHost), privacy: .public) \
            activityTarget=\(activityTarget, privacy: .public) \
            sessionTarget=\(descriptor.targetContentIdentifier, privacy: .public)
            """
        )
    }

    private func hostClassName(for host: NSResponder?) -> String {
        guard let host else {
            return Logging.missingValue
        }

        return String(describing: type(of: host))
    }

    private func activityTitle(for descriptor: LooperContinuationActivityDescriptor) -> String {
        let project = descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle]?
            .trimmingCharacters(in: .whitespacesAndNewlines)
        guard let project, !project.isEmpty else {
            return descriptor.title
        }

        return "\(descriptor.title) - \(project)"
    }

    private func activityKeywords(for descriptor: LooperContinuationActivityDescriptor) -> Set<String> {
        [
            "looper",
            "Codex",
            descriptor.title,
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle],
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview],
        ]
        .compactMap { value in
            value?.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        .filter { !$0.isEmpty }
        .reduce(into: Set<String>()) { keywords, value in
            keywords.insert(value)
        }
    }

    private func contentAttributeSet(
        for descriptor: LooperContinuationActivityDescriptor
    ) -> CSSearchableItemAttributeSet {
        let attributes = CSSearchableItemAttributeSet(contentType: .item)
        attributes.title = activityTitle(for: descriptor)
        attributes.displayName = descriptor.title
        attributes.contentDescription = contentDescription(for: descriptor)
        return attributes
    }

    private func contentDescription(for descriptor: LooperContinuationActivityDescriptor) -> String {
        [
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle],
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview],
        ]
        .compactMap { value in
            value?.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        .filter { !$0.isEmpty }
        .joined(separator: "\n")
    }

}

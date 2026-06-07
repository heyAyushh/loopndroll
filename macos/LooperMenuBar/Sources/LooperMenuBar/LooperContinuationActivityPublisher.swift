import AppKit
import CoreSpotlight
import LooperMenuBarCore
import OSLog

@MainActor
final class LooperContinuationActivityPublisher {
    fileprivate enum AnchorWindowLayout {
        static let size = NSSize(width: 1, height: 1)
        static let screenInset: CGFloat = 1
        static let alphaValue: CGFloat = 0.001
        static let minimumKeyWindowAlphaValue: CGFloat = 0.01
    }

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
    private let activityAnchor = LooperContinuationActivityAnchor()
    private var currentActivity: NSUserActivity?
    private var currentDescriptor: LooperContinuationActivityDescriptor?
    private var currentActivityRefreshTask: Task<Void, Never>?

    func attachHost(_ host: NSResponder?) {
        activityAnchor.attachStatusHost(host)
        logger.info("handoff host attached host=\(self.hostClassName(for: host), privacy: .public)")
        if currentActivity != nil {
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
        activityAnchor.publish(activity, descriptor: descriptor)
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
        activityAnchor.detach()
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
        activityAnchor.refreshCurrentActivity()
        markActivityCurrent(currentActivity)
        logger.debug("handoff activity refreshed host=\(self.activityAnchor.hostDescription, privacy: .public)")
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
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.webpageURL = nil
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
            host=\(self.activityAnchor.hostDescription, privacy: .public) \
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

@MainActor
private final class LooperContinuationActivityAnchor {
    private weak var statusHost: NSResponder?
    private let viewController = LooperContinuationActivityAnchorViewController()
    private lazy var window: LooperContinuationActivityAnchorPanel = {
        // AppKit Handoff promotes responder activities through a main/key window responder chain.
        let window = LooperContinuationActivityAnchorPanel(
            contentRect: NSRect(origin: .zero, size: LooperContinuationActivityPublisher.AnchorWindowLayout.size),
            styleMask: [.borderless, .nonactivatingPanel],
            backing: .buffered,
            defer: false
        )
        window.backgroundColor = .clear
        window.isOpaque = false
        window.alphaValue = max(
            LooperContinuationActivityPublisher.AnchorWindowLayout.alphaValue,
            LooperContinuationActivityPublisher.AnchorWindowLayout.minimumKeyWindowAlphaValue
        )
        window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]
        window.ignoresMouseEvents = true
        window.isExcludedFromWindowsMenu = true
        window.isReleasedWhenClosed = false
        window.level = .normal
        window.title = LooperContinuationActivityAnchorViewController.Content.windowTitle
        window.contentViewController = viewController
        positionWindowInScreen(window)
        return window
    }()

    var hostDescription: String {
        [
            responderClassName(for: statusHost),
            responderClassName(for: viewController.view),
        ]
        .compactMap { $0 }
        .joined(separator: "+")
        .nilIfEmpty ?? "none"
    }

    func attachStatusHost(_ host: NSResponder?) {
        statusHost?.userActivity = nil
        statusHost = host
        if let activity = viewController.userActivity {
            statusHost?.userActivity = activity
        }
    }

    func publish(
        _ activity: NSUserActivity,
        descriptor: LooperContinuationActivityDescriptor
    ) {
        _ = window
        viewController.descriptor = descriptor
        viewController.userActivity = activity
        viewController.view.userActivity = activity
        statusHost?.userActivity = activity
        positionWindowInScreen(window)
        window.makeKeyAndOrderFront(nil)
        refreshCurrentActivity()
    }

    func refreshCurrentActivity() {
        guard let activity = viewController.userActivity else {
            return
        }

        if let descriptor = viewController.descriptor {
            update(activity, with: descriptor)
        }
        viewController.view.userActivity = activity
        statusHost?.userActivity = activity
        viewController.refreshActivity(activity)
        activity.needsSave = true
        activity.becomeCurrent()
    }

    func detach() {
        statusHost?.userActivity = nil
        viewController.view.userActivity = nil
        viewController.userActivity = nil
        viewController.descriptor = nil
        window.orderOut(nil)
    }

    private func update(
        _ activity: NSUserActivity,
        with descriptor: LooperContinuationActivityDescriptor
    ) {
        activity.addUserInfoEntries(from: descriptor.userInfo)
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.webpageURL = nil
    }

    private func positionWindowInScreen(_ window: NSWindow) {
        guard let visibleFrame = NSScreen.main?.visibleFrame else {
            window.setFrameOrigin(.zero)
            return
        }

        window.setFrameOrigin(
            NSPoint(
                x: visibleFrame.minX + LooperContinuationActivityPublisher.AnchorWindowLayout.screenInset,
                y: visibleFrame.maxY - LooperContinuationActivityPublisher.AnchorWindowLayout.screenInset
            )
        )
    }

    private func responderClassName(for responder: NSResponder?) -> String? {
        guard let responder else {
            return nil
        }

        return String(describing: type(of: responder))
    }
}

private final class LooperContinuationActivityAnchorPanel: NSPanel {
    override var canBecomeKey: Bool {
        true
    }

    override var canBecomeMain: Bool {
        true
    }
}

@MainActor
private final class LooperContinuationActivityAnchorViewController: NSViewController {
    enum Content {
        static let windowTitle = "looper Handoff Activity Anchor"
    }

    var descriptor: LooperContinuationActivityDescriptor?

    override func loadView() {
        view = NSView(
            frame: NSRect(
                origin: .zero,
                size: LooperContinuationActivityPublisher.AnchorWindowLayout.size
            )
        )
    }

    override func updateUserActivityState(_ activity: NSUserActivity) {
        super.updateUserActivityState(activity)
        applyDescriptor(to: activity)
    }

    func refreshActivity(_ activity: NSUserActivity) {
        applyDescriptor(to: activity)
    }

    private func applyDescriptor(to activity: NSUserActivity) {
        guard let descriptor else {
            return
        }

        activity.title = [
            descriptor.title,
            descriptor.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle],
        ]
        .compactMap { value in
            value?.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        .filter { !$0.isEmpty }
        .joined(separator: " - ")
        activity.addUserInfoEntries(from: descriptor.userInfo)
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.webpageURL = nil
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

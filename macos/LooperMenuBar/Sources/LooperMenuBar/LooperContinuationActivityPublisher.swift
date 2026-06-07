import AppKit
import CoreSpotlight
import LooperMenuBarCore

@MainActor
final class LooperContinuationActivityPublisher {
    fileprivate enum HostWindowLayout {
        static let size = NSSize(width: 1, height: 1)
        static let offscreenInset: CGFloat = 96
        static let alphaValue: CGFloat = 0.01
        static let currentActivityRefreshInterval: Duration = .seconds(3)
    }

    private static let persistentActivityIdentifier = NSUserActivityPersistentIdentifier(
        LooperContinuationActivity.persistentIdentifier
    )

    private let activityHost = LooperContinuationActivityHost()
    private var currentActivity: NSUserActivity?
    private var currentDescriptor: LooperContinuationActivityDescriptor?
    private var currentActivityRefreshTask: Task<Void, Never>?

    func publish(_ descriptor: LooperContinuationActivityDescriptor) {
        guard descriptor != currentDescriptor else {
            refreshCurrentActivity()
            return
        }

        let activity = currentActivity ?? NSUserActivity(activityType: LooperContinuationActivity.activityType)
        configure(activity, with: descriptor, shouldUpdateIdentity: shouldUpdateActivityIdentity(for: descriptor))
        activityHost.attach(activity, descriptor: descriptor)
        markActivityCurrent(activity)

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
        activityHost.detach()
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
                try? await Task.sleep(for: HostWindowLayout.currentActivityRefreshInterval)
                self?.refreshCurrentActivity()
            }
        }
    }

    private func refreshCurrentActivity() {
        activityHost.refreshCurrentActivity()
        if let currentActivity {
            markActivityCurrent(currentActivity)
        }
    }

    private func markActivityCurrent(_ activity: NSUserActivity) {
        activity.needsSave = true
        activity.becomeCurrent()
    }

    private func shouldUpdateActivityIdentity(for descriptor: LooperContinuationActivityDescriptor) -> Bool {
        guard let currentDescriptor else {
            return true
        }

        return currentDescriptor.targetContentIdentifier != descriptor.targetContentIdentifier
    }

    private func configure(
        _ activity: NSUserActivity,
        with descriptor: LooperContinuationActivityDescriptor,
        shouldUpdateIdentity: Bool
    ) {
        activity.title = activityTitle(for: descriptor)
        if activity.persistentIdentifier != Self.persistentActivityIdentifier {
            activity.persistentIdentifier = Self.persistentActivityIdentifier
        }
        if shouldUpdateIdentity {
            activity.targetContentIdentifier = descriptor.targetContentIdentifier
        }
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
        activity.keywords = activityKeywords(for: descriptor)
        activity.contentAttributeSet = contentAttributeSet(for: descriptor)
        activity.addUserInfoEntries(from: descriptor.userInfo)
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
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
private final class LooperContinuationActivityHost {
    private let viewController = LooperContinuationActivityHostViewController()
    private lazy var window: NSWindow = {
        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: LooperContinuationActivityPublisher.HostWindowLayout.size),
            styleMask: [.borderless],
            backing: .buffered,
            defer: false
        )
        window.backgroundColor = .clear
        window.isOpaque = false
        window.alphaValue = LooperContinuationActivityPublisher.HostWindowLayout.alphaValue
        window.collectionBehavior = [.canJoinAllSpaces, .stationary]
        window.ignoresMouseEvents = true
        window.isReleasedWhenClosed = false
        window.level = .normal
        window.title = LooperContinuationActivityHostViewController.Content.windowTitle
        window.contentViewController = viewController
        positionWindowOffscreen(window)
        return window
    }()

    func attach(
        _ activity: NSUserActivity,
        descriptor: LooperContinuationActivityDescriptor
    ) {
        _ = window
        viewController.descriptor = descriptor
        viewController.userActivity = activity
        positionWindowOffscreen(window)
        window.orderFrontRegardless()
        refreshCurrentActivity()
    }

    func refreshCurrentActivity() {
        guard let activity = viewController.userActivity else {
            return
        }

        viewController.updateUserActivityState(activity)
        activity.needsSave = true
        activity.becomeCurrent()
    }

    func detach() {
        viewController.userActivity = nil
        viewController.descriptor = nil
        window.orderOut(nil)
    }

    private func positionWindowOffscreen(_ window: NSWindow) {
        guard let visibleFrame = NSScreen.main?.visibleFrame else {
            window.setFrameOrigin(.zero)
            return
        }

        window.setFrameOrigin(
            NSPoint(
                x: visibleFrame.maxX + LooperContinuationActivityPublisher.HostWindowLayout.offscreenInset,
                y: visibleFrame.maxY + LooperContinuationActivityPublisher.HostWindowLayout.offscreenInset
            )
        )
    }
}

@MainActor
private final class LooperContinuationActivityHostViewController: NSViewController {
    enum Content {
        static let windowTitle = "looper Handoff"
    }

    var descriptor: LooperContinuationActivityDescriptor?

    override func loadView() {
        view = NSView(
            frame: NSRect(
                origin: .zero,
                size: LooperContinuationActivityPublisher.HostWindowLayout.size
            )
        )
    }

    override func updateUserActivityState(_ activity: NSUserActivity) {
        super.updateUserActivityState(activity)
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
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
    }
}

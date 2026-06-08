import AppKit
import CoreSpotlight
import CoreGraphics
import LooperMenuBarCore
import OSLog

@MainActor
final class LooperContinuationActivityPublisher {
    fileprivate enum UtilityPanelLayout {
        static let contentSize = NSSize(width: 1, height: 1)
        static let screenInset: CGFloat = 1
        static let backgroundAlpha: CGFloat = 0.02
        static let cornerRadius: CGFloat = 0
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
    private let activityOwner = LooperContinuationActivityPanelOwner()
    private let idleTimeProvider: LooperUserIdleTimeProviding
    private var currentActivity: NSUserActivity?
    private var currentDescriptor: LooperContinuationActivityDescriptor?
    private var currentActivityRefreshTask: Task<Void, Never>?
    private var focusAssistedActivationLease = LooperHandoffActivationLease()

    var focusAssist = LooperHandoffFocusAssist.defaultOption {
        didSet {
            guard focusAssist != oldValue else {
                return
            }
            refreshCurrentActivity()
        }
    }

    var focusAssistHoldDuration = LooperHandoffHoldDuration.defaultOption {
        didSet {
            guard focusAssistHoldDuration != oldValue else {
                return
            }
            refreshCurrentActivity()
        }
    }

    init(idleTimeProvider: LooperUserIdleTimeProviding = QuartzLooperUserIdleTimeProvider()) {
        self.idleTimeProvider = idleTimeProvider
    }

    func requestFocusAssistedActivation() {
        activateFocusAssist(reason: "hotkey")
        refreshCurrentActivity()
    }

    func attachHost(_ host: NSResponder?) {
        activityOwner.attachStatusHost(host)
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
        activityOwner.publish(
            activity,
            descriptor: descriptor,
            allowsFocusAssistedActivation: allowsFocusAssistedActivation()
        )
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
        activityOwner.detach()
        logger.info("handoff activity invalidated")
        focusAssistedActivationLease.invalidate()
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
        activityOwner.refreshCurrentActivity(allowsFocusAssistedActivation: allowsFocusAssistedActivation())
        markActivityCurrent(currentActivity)
        logger.debug("handoff activity refreshed host=\(self.activityOwner.hostDescription, privacy: .public)")
    }

    private func allowsFocusAssistedActivation() -> Bool {
        if focusAssistedActivationLease.isActive() {
            return true
        }

        if focusAssist == .rightNow {
            return false
        }

        guard let idleThresholdSeconds = focusAssist.idleThresholdSeconds else {
            return false
        }

        guard idleTimeProvider.secondsSinceLastUserInput() >= idleThresholdSeconds else {
            return false
        }

        activateFocusAssist(reason: "idle")
        return true
    }

    private func activateFocusAssist(reason: String) {
        focusAssistedActivationLease.activate(holdDuration: focusAssistHoldDuration)
        logger.debug(
            """
            handoff focus activation leased reason=\(reason, privacy: .public) \
            hold=\(self.focusAssistHoldDuration.menuTitle, privacy: .public)
            """
        )
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
            host=\(self.activityOwner.hostDescription, privacy: .public) \
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
protocol LooperUserIdleTimeProviding {
    func secondsSinceLastUserInput() -> TimeInterval
}

private struct QuartzLooperUserIdleTimeProvider: LooperUserIdleTimeProviding {
    private enum EventSource {
        static let anyInputEventType = CGEventType(rawValue: UInt32.max)!
    }

    func secondsSinceLastUserInput() -> TimeInterval {
        CGEventSource.secondsSinceLastEventType(
            .hidSystemState,
            eventType: EventSource.anyInputEventType
        )
    }
}

@MainActor
private final class LooperContinuationActivityPanelOwner {
    private weak var statusHost: NSResponder?
    private let viewController = LooperContinuationActivityPanelViewController()
    private var isFocusAssistedPresentationActive = false
    private lazy var panel: LooperContinuationActivityUtilityPanel = {
        // Handoff needs a live AppKit responder owner; this panel provides one without taking focus.
        let panel = LooperContinuationActivityUtilityPanel(
            contentRect: NSRect(origin: .zero, size: LooperContinuationActivityPublisher.UtilityPanelLayout.contentSize),
            styleMask: [.titled, .utilityWindow, .nonactivatingPanel, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        panel.animationBehavior = .none
        panel.backgroundColor = .clear
        panel.becomesKeyOnlyIfNeeded = false
        panel.canHide = false
        panel.hasShadow = false
        panel.hidesOnDeactivate = false
        panel.isExcludedFromWindowsMenu = true
        panel.isFloatingPanel = true
        panel.isMovable = false
        panel.isMovableByWindowBackground = false
        panel.isOpaque = false
        panel.isReleasedWhenClosed = false
        panel.isRestorable = false
        panel.ignoresMouseEvents = true
        panel.level = .floating
        panel.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle, .fullScreenAuxiliary]
        panel.tabbingMode = .disallowed
        panel.title = LooperContinuationActivityPanelViewController.Content.windowTitle
        panel.titleVisibility = .hidden
        panel.titlebarAppearsTransparent = true
        panel.contentViewController = viewController
        hideStandardWindowButtons(in: panel)
        positionPanelInScreen(panel)
        return panel
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
        descriptor: LooperContinuationActivityDescriptor,
        allowsFocusAssistedActivation: Bool
    ) {
        _ = panel
        viewController.descriptor = descriptor
        viewController.userActivity = activity
        viewController.view.userActivity = activity
        statusHost?.userActivity = activity
        positionPanelInScreen(panel)
        presentPanel(allowsFocusAssistedActivation: allowsFocusAssistedActivation)
        refreshCurrentActivity(allowsFocusAssistedActivation: allowsFocusAssistedActivation)
    }

    func refreshCurrentActivity(allowsFocusAssistedActivation: Bool) {
        guard let activity = viewController.userActivity else {
            return
        }

        if let descriptor = viewController.descriptor {
            update(activity, with: descriptor)
        }
        viewController.view.userActivity = activity
        statusHost?.userActivity = activity
        presentPanel(allowsFocusAssistedActivation: allowsFocusAssistedActivation)
        viewController.refreshActivity(activity)
        activity.needsSave = true
        activity.becomeCurrent()
    }

    func detach() {
        statusHost?.userActivity = nil
        viewController.view.userActivity = nil
        viewController.userActivity = nil
        viewController.descriptor = nil
        panel.orderOut(nil)
        restoreNonActivatingPresentation()
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

    private func positionPanelInScreen(_ panel: NSWindow) {
        guard let visibleFrame = NSScreen.main?.visibleFrame else {
            panel.setFrameOrigin(.zero)
            return
        }

        panel.setFrameOrigin(
            NSPoint(
                x: visibleFrame.maxX - panel.frame.width - LooperContinuationActivityPublisher.UtilityPanelLayout.screenInset,
                y: visibleFrame.maxY - panel.frame.height - LooperContinuationActivityPublisher.UtilityPanelLayout.screenInset
            )
        )
    }

    private func hideStandardWindowButtons(in panel: NSPanel) {
        [
            NSWindow.ButtonType.closeButton,
            .miniaturizeButton,
            .zoomButton,
        ].forEach { buttonType in
            panel.standardWindowButton(buttonType)?.isHidden = true
        }
    }

    private func responderClassName(for responder: NSResponder?) -> String? {
        guard let responder else {
            return nil
        }

        return String(describing: type(of: responder))
    }

    private func presentPanel(allowsFocusAssistedActivation: Bool) {
        if allowsFocusAssistedActivation {
            enableFocusAssistedPresentation()
            return
        }

        restoreNonActivatingPresentation()
        panel.orderFrontRegardless()
    }

    private func enableFocusAssistedPresentation() {
        guard !isFocusAssistedPresentationActive else {
            panel.orderFrontRegardless()
            return
        }

        panel.allowsKeyAndMainPresentation = true
        panel.styleMask.remove(.nonactivatingPanel)
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        panel.makeKeyAndOrderFront(nil)
        panel.orderFrontRegardless()
        isFocusAssistedPresentationActive = true
    }

    private func restoreNonActivatingPresentation() {
        panel.allowsKeyAndMainPresentation = false
        panel.styleMask.insert(.nonactivatingPanel)
        NSApp.setActivationPolicy(.accessory)
        isFocusAssistedPresentationActive = false
    }
}

private final class LooperContinuationActivityUtilityPanel: NSPanel {
    var allowsKeyAndMainPresentation = false

    override var canBecomeKey: Bool {
        allowsKeyAndMainPresentation
    }

    override var canBecomeMain: Bool {
        allowsKeyAndMainPresentation
    }
}

@MainActor
private final class LooperContinuationActivityPanelViewController: NSViewController {
    enum Content {
        static let windowTitle = "looper Handoff Activity"
    }

    var descriptor: LooperContinuationActivityDescriptor?

    override func loadView() {
        view = NSView(
            frame: NSRect(
                origin: .zero,
                size: LooperContinuationActivityPublisher.UtilityPanelLayout.contentSize
            )
        )
        view.wantsLayer = true
        view.layer?.backgroundColor = NSColor.windowBackgroundColor
            .withAlphaComponent(LooperContinuationActivityPublisher.UtilityPanelLayout.backgroundAlpha)
            .cgColor
        view.layer?.cornerRadius = LooperContinuationActivityPublisher.UtilityPanelLayout.cornerRadius
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

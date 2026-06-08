import AppKit
import CoreSpotlight
import CoreGraphics
import LooperMenuBarCore
import OSLog

@MainActor
final class LooperContinuationActivityPublisher {
    fileprivate enum UtilityPanelLayout {
        static let contentSize = NSSize(width: 360, height: 88)
        static let screenInset: CGFloat = 16
        static let cornerRadius: CGFloat = 12
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
        republishCurrentActivity(presentation: .activateApplication)
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
            presentation: presentationForRefresh()
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
        activityOwner.refreshCurrentActivity(presentation: presentationForRefresh())
        markActivityCurrent(currentActivity)
        logger.debug("handoff activity refreshed host=\(self.activityOwner.hostDescription, privacy: .public)")
    }

    private func republishCurrentActivity(presentation: LooperContinuationPresentation) {
        guard let currentDescriptor else {
            refreshCurrentActivity()
            return
        }

        currentActivity?.invalidate()
        let activity = NSUserActivity(activityType: LooperContinuationActivity.activityType)
        configure(activity, with: currentDescriptor)
        activityOwner.publish(
            activity,
            descriptor: currentDescriptor,
            presentation: presentation
        )
        markActivityCurrent(activity)
        logPublishedActivity(activity, descriptor: currentDescriptor)
        currentActivity = activity
        startCurrentActivityRefreshLoop()
    }

    private func presentationForRefresh() -> LooperContinuationPresentation {
        if focusAssistedActivationLease.isActive() {
            return .maintainFocusAssisted
        }

        guard shouldActivateForIdleFocusAssist() else {
            return .nonActivating
        }

        activateFocusAssist(reason: "idle")
        return .activateApplication
    }

    private func shouldActivateForIdleFocusAssist() -> Bool {
        guard focusAssist != .rightNow else {
            return false
        }
        guard let idleThresholdSeconds = focusAssist.idleThresholdSeconds else {
            return false
        }
        return idleTimeProvider.secondsSinceLastUserInput() >= idleThresholdSeconds
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

private enum LooperContinuationPresentation {
    case nonActivating
    case maintainFocusAssisted
    case activateApplication
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
        // Handoff needs a live AppKit responder owner. The panel becomes visible only for explicit activation.
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
        presentation: LooperContinuationPresentation
    ) {
        _ = panel
        viewController.descriptor = descriptor
        viewController.userActivity = activity
        viewController.view.userActivity = activity
        statusHost?.userActivity = activity
        positionPanelInScreen(panel)
        presentPanel(presentation: presentation)
        refreshCurrentActivity(presentation: presentation)
    }

    func refreshCurrentActivity(presentation: LooperContinuationPresentation) {
        guard let activity = viewController.userActivity else {
            return
        }

        if let descriptor = viewController.descriptor {
            update(activity, with: descriptor)
        }
        viewController.view.userActivity = activity
        statusHost?.userActivity = activity
        presentPanel(presentation: presentation)
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

    private func presentPanel(presentation: LooperContinuationPresentation) {
        switch presentation {
        case .activateApplication:
            enableFocusAssistedPresentation(activatesApplication: true)
        case .maintainFocusAssisted:
            enableFocusAssistedPresentation(activatesApplication: false)
        case .nonActivating:
            restoreNonActivatingPresentation()
            panel.orderOut(nil)
        }
    }

    private func enableFocusAssistedPresentation(activatesApplication: Bool) {
        if !isFocusAssistedPresentationActive {
            panel.allowsKeyAndMainPresentation = true
            panel.styleMask.remove(.nonactivatingPanel)
            NSApp.setActivationPolicy(.regular)
            isFocusAssistedPresentationActive = true
        }

        NSApp.setActivationPolicy(.regular)
        if activatesApplication {
            NSApp.activate(ignoringOtherApps: true)
            panel.makeKeyAndOrderFront(nil)
        } else if NSApp.isActive {
            panel.orderFrontRegardless()
        } else {
            panel.orderOut(nil)
        }
    }

    private func restoreNonActivatingPresentation() {
        panel.allowsKeyAndMainPresentation = false
        panel.styleMask.insert(.nonactivatingPanel)
        NSApp.setActivationPolicy(.accessory)
        panel.orderOut(nil)
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
        static let statusText = "Ready for Handoff"
        static let fallbackTitle = "looper"
        static let fallbackSubtitle = "Continue on iPhone"
    }

    var descriptor: LooperContinuationActivityDescriptor?
    private let titleLabel = NSTextField(labelWithString: Content.fallbackTitle)
    private let subtitleLabel = NSTextField(labelWithString: Content.fallbackSubtitle)

    override func loadView() {
        let visualEffectView = NSVisualEffectView(
            frame: NSRect(
                origin: .zero,
                size: LooperContinuationActivityPublisher.UtilityPanelLayout.contentSize
            )
        )
        visualEffectView.material = .popover
        visualEffectView.blendingMode = .behindWindow
        visualEffectView.state = .active
        visualEffectView.wantsLayer = true
        visualEffectView.layer?.cornerRadius = LooperContinuationActivityPublisher.UtilityPanelLayout.cornerRadius
        visualEffectView.layer?.masksToBounds = true
        view = visualEffectView
        configureLabels()
        layoutLabels(in: visualEffectView)
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
        refreshLabels(with: descriptor)
    }

    private func configureLabels() {
        titleLabel.font = .systemFont(ofSize: 13, weight: .semibold)
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.maximumNumberOfLines = 1
        titleLabel.textColor = .labelColor
        titleLabel.translatesAutoresizingMaskIntoConstraints = false

        subtitleLabel.font = .systemFont(ofSize: 12, weight: .regular)
        subtitleLabel.lineBreakMode = .byTruncatingTail
        subtitleLabel.maximumNumberOfLines = 1
        subtitleLabel.textColor = .secondaryLabelColor
        subtitleLabel.translatesAutoresizingMaskIntoConstraints = false
    }

    private func layoutLabels(in visualEffectView: NSVisualEffectView) {
        visualEffectView.addSubview(titleLabel)
        visualEffectView.addSubview(subtitleLabel)

        NSLayoutConstraint.activate([
            titleLabel.leadingAnchor.constraint(equalTo: visualEffectView.leadingAnchor, constant: 16),
            titleLabel.trailingAnchor.constraint(equalTo: visualEffectView.trailingAnchor, constant: -16),
            titleLabel.topAnchor.constraint(equalTo: visualEffectView.topAnchor, constant: 20),
            subtitleLabel.leadingAnchor.constraint(equalTo: titleLabel.leadingAnchor),
            subtitleLabel.trailingAnchor.constraint(equalTo: titleLabel.trailingAnchor),
            subtitleLabel.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 6),
        ])
    }

    private func refreshLabels(with descriptor: LooperContinuationActivityDescriptor?) {
        titleLabel.stringValue = descriptor?.title.nilIfEmpty ?? Content.fallbackTitle
        let subtitle = [
            Content.statusText,
            descriptor?.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle]?.nilIfEmpty,
        ]
        .compactMap { $0 }
        .joined(separator: " - ")
        subtitleLabel.stringValue = subtitle.nilIfEmpty ?? Content.fallbackSubtitle
    }
}

private extension String {
    var nilIfEmpty: String? {
        trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : self
    }
}

import AppKit
import CoreSpotlight
import CoreGraphics
import LooperMenuBarCore
import OSLog
import QuartzCore
import SwiftUI

@MainActor
final class LooperContinuationActivityPublisher {
    fileprivate enum UtilityPanelLayout {
        static let contentSize = NSSize(width: 620, height: 160)
    }

    private enum ActivityRefresh {
        static let currentActivityRefreshInterval: Duration = .seconds(3)
    }

    private enum LeaseExpiration {
        static let minimumDelaySeconds: TimeInterval = 0.05
        static let nanosecondsPerSecond: TimeInterval = 1_000_000_000
        static let inputTimestampToleranceSeconds: TimeInterval = 0.25
    }

    private enum Logging {
        static let subsystem = "dev.looper.app.menubar"
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
    private var focusAssistedLeaseExpirationTask: Task<Void, Never>?
    private var focusAssistedActivationLease = LooperHandoffActivationLease()
    private var suppressedIdleFocusAssistLastInputDate: Date?

    var isHandoffSupported = false {
        didSet {
            guard isHandoffSupported != oldValue else {
                return
            }
            if !isHandoffSupported {
                deactivateFocusAssist()
            }
            refreshCurrentActivity(allowsIdleActivation: false)
        }
    }

    var focusAssist = LooperHandoffFocusAssist.defaultOption {
        didSet {
            guard focusAssist != oldValue else {
                return
            }
            if focusAssist.activatesWithoutIdleDelay {
                _ = requestFocusAssistedActivation()
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
            if focusAssistedActivationLease.expiresAt != nil {
                focusAssistedActivationLease.replace(holdDuration: focusAssistHoldDuration)
                scheduleFocusAssistedLeaseExpiration()
            }
            refreshCurrentActivity()
        }
    }

    init(idleTimeProvider: LooperUserIdleTimeProviding = QuartzLooperUserIdleTimeProvider()) {
        self.idleTimeProvider = idleTimeProvider
    }

    @discardableResult
    func requestFocusAssistedActivation() -> Bool {
        guard isHandoffSupported else {
            deactivateFocusAssist()
            refreshCurrentActivity(allowsIdleActivation: false)
            logger.info("handoff activation ignored because support gate is closed")
            return false
        }

        activateFocusAssist(reason: "hotkey")
        presentCurrentActivity(presentation: .activateApplication)
        return true
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
        let presentation = presentationForRefresh()
        activityOwner.publish(activity, descriptor: descriptor, presentation: presentation)
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
        focusAssistedLeaseExpirationTask?.cancel()
        currentActivity?.invalidate()
        activityOwner.detach()
        logger.info("handoff activity invalidated")
        focusAssistedActivationLease.invalidate()
        suppressedIdleFocusAssistLastInputDate = nil
        currentActivityRefreshTask = nil
        focusAssistedLeaseExpirationTask = nil
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

    private func refreshCurrentActivity(allowsIdleActivation: Bool = true) {
        guard let currentActivity else {
            return
        }

        if let currentDescriptor {
            configure(currentActivity, with: currentDescriptor)
        }
        let presentation = presentationForRefresh(allowsIdleActivation: allowsIdleActivation)
        activityOwner.refreshCurrentActivity(presentation: presentation)
        logger.debug("handoff activity refreshed host=\(self.activityOwner.hostDescription, privacy: .public)")
    }

    private func presentCurrentActivity(presentation: LooperContinuationPresentation) {
        guard let currentDescriptor else {
            refreshCurrentActivity()
            return
        }

        let activity = currentActivity ?? NSUserActivity(activityType: LooperContinuationActivity.activityType)
        configure(activity, with: currentDescriptor)
        activityOwner.publish(activity, descriptor: currentDescriptor, presentation: presentation)
        logPublishedActivity(activity, descriptor: currentDescriptor)
        currentActivity = activity
        startCurrentActivityRefreshLoop()
    }

    private func presentationForRefresh(allowsIdleActivation: Bool = true) -> LooperContinuationPresentation {
        guard isHandoffSupported else {
            deactivateFocusAssist()
            return .inactive
        }

        let hadFocusAssistedLease = focusAssistedActivationLease.expiresAt != nil
        if focusAssistedActivationLease.isActive() {
            guard activityOwner.canMaintainCurrentHandoffPresentation else {
                markIdleFocusAssistSuppressedUntilUserInput()
                focusAssistedActivationLease.invalidate()
                return .currentNonActivating
            }

            return .maintainFocusAssisted
        }

        if hadFocusAssistedLease {
            markIdleFocusAssistSuppressedUntilUserInput()
        }
        guard allowsIdleActivation, shouldActivateForIdleFocusAssist() else {
            return .currentNonActivating
        }

        activateFocusAssist(reason: "idle")
        return .activateApplication
    }

    private func shouldActivateForIdleFocusAssist() -> Bool {
        let currentIdleSeconds = idleTimeProvider.secondsSinceLastUserInput()
        guard !isIdleFocusAssistSuppressed(currentIdleSeconds: currentIdleSeconds) else {
            return false
        }
        guard !focusAssist.activatesWithoutIdleDelay else {
            return true
        }
        guard let idleThresholdSeconds = focusAssist.idleThresholdSeconds else {
            return false
        }

        return currentIdleSeconds >= idleThresholdSeconds
    }

    private func activateFocusAssist(reason: String) {
        focusAssistedActivationLease.activate(holdDuration: focusAssistHoldDuration)
        suppressedIdleFocusAssistLastInputDate = nil
        scheduleFocusAssistedLeaseExpiration()
        logger.debug(
            """
            handoff focus activation leased reason=\(reason, privacy: .public) \
            hold=\(self.focusAssistHoldDuration.menuTitle, privacy: .public)
            """
        )
    }

    private func deactivateFocusAssist() {
        focusAssistedLeaseExpirationTask?.cancel()
        focusAssistedLeaseExpirationTask = nil
        focusAssistedActivationLease.invalidate()
    }

    private func markIdleFocusAssistSuppressedUntilUserInput() {
        deactivateFocusAssist()
        suppressedIdleFocusAssistLastInputDate = lastInputDate(
            currentIdleSeconds: idleTimeProvider.secondsSinceLastUserInput()
        )
    }

    private func isIdleFocusAssistSuppressed(currentIdleSeconds: TimeInterval) -> Bool {
        guard let suppressedIdleFocusAssistLastInputDate else {
            return false
        }

        let currentLastInputDate = lastInputDate(currentIdleSeconds: currentIdleSeconds)
        let inputAdvanced = currentLastInputDate.timeIntervalSince(suppressedIdleFocusAssistLastInputDate)
            > LeaseExpiration.inputTimestampToleranceSeconds
        guard inputAdvanced else {
            return true
        }

        self.suppressedIdleFocusAssistLastInputDate = nil
        return false
    }

    private func lastInputDate(currentIdleSeconds: TimeInterval) -> Date {
        Date().addingTimeInterval(-currentIdleSeconds)
    }

    private func scheduleFocusAssistedLeaseExpiration() {
        focusAssistedLeaseExpirationTask?.cancel()

        guard let leaseExpiresAt = focusAssistedActivationLease.expiresAt else {
            focusAssistedLeaseExpirationTask = nil
            return
        }

        let delaySeconds = max(leaseExpiresAt.timeIntervalSinceNow, LeaseExpiration.minimumDelaySeconds)
        let delayNanoseconds = UInt64(delaySeconds * LeaseExpiration.nanosecondsPerSecond)

        focusAssistedLeaseExpirationTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: delayNanoseconds)
            guard !Task.isCancelled, let self else {
                return
            }

            self.focusAssistedLeaseExpirationTask = nil
            self.refreshCurrentActivity(allowsIdleActivation: false)
        }
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
        activity.webpageURL = descriptor.webpageURL
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
        activity.keywords = activityKeywords(for: descriptor)
        activity.contentAttributeSet = contentAttributeSet(for: descriptor)
        activity.userInfo = descriptor.userInfo
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
    case inactive
    case currentNonActivating
    case maintainFocusAssisted
    case activateApplication

    var publishesCurrentHandoff: Bool {
        switch self {
        case .currentNonActivating, .activateApplication, .maintainFocusAssisted:
            true
        case .inactive:
            false
        }
    }

}

private enum LooperContinuationPanelVisualState: Equatable {
    case focused
    case resting

    var isFocused: Bool {
        self == .focused
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

private final class LooperNotificationObserverBag: @unchecked Sendable {
    private var observers: [NSObjectProtocol] = []

    var isEmpty: Bool {
        observers.isEmpty
    }

    func replace(with observers: [NSObjectProtocol]) {
        for observer in self.observers {
            NotificationCenter.default.removeObserver(observer)
        }
        self.observers = observers
    }

    deinit {
        for observer in observers {
            NotificationCenter.default.removeObserver(observer)
        }
    }
}

@MainActor
private final class LooperContinuationActivityPanelOwner {
    private enum PanelAnimation {
        static let appearanceDuration: TimeInterval = 0.18
        static let disappearanceDuration: TimeInterval = 0.14
        static let appearanceInitialScale: CGFloat = 0.975
        static let disappearanceFinalScale: CGFloat = 0.985
        static let hiddenAlpha: CGFloat = 0
        static let visibleAlpha: CGFloat = 1
        static let easeOutControlPoint1 = CGPoint(x: 0.16, y: 1)
        static let easeOutControlPoint2 = CGPoint(x: 0.3, y: 1)
        static let easeInControlPoint1 = CGPoint(x: 0.4, y: 0)
        static let easeInControlPoint2 = CGPoint(x: 1, y: 1)
    }

    private weak var statusHost: NSResponder?
    private let viewController = LooperContinuationActivityPanelViewController()
    private var isFocusAssistedPresentationActive = false
    private var visiblePanelState: LooperContinuationPanelVisualState?
    private var panelAnimationGeneration = 0
    private let applicationActivationObservers = LooperNotificationObserverBag()
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

    var canMaintainCurrentHandoffPresentation: Bool {
        NSApp.isActive && visiblePanelState == .focused
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
        if !panel.isVisible {
            positionPanelInScreen(panel)
        }
        presentPanel(presentation: presentation)
        refreshActivityState(activity, presentation: presentation)
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
        refreshActivityState(activity, presentation: presentation)
    }

    private func refreshActivityState(
        _ activity: NSUserActivity,
        presentation: LooperContinuationPresentation
    ) {
        viewController.refreshActivity(activity)
        activity.needsSave = true
        if presentation.publishesCurrentHandoff {
            activity.becomeCurrent()
        } else {
            activity.resignCurrent()
        }
    }

    func detach() {
        statusHost?.userActivity = nil
        viewController.view.userActivity = nil
        viewController.userActivity = nil
        viewController.descriptor = nil
        hidePanel(animated: false)
    }

    private func update(
        _ activity: NSUserActivity,
        with descriptor: LooperContinuationActivityDescriptor
    ) {
        activity.userInfo = descriptor.userInfo
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.webpageURL = descriptor.webpageURL
    }

    private func positionPanelInScreen(_ panel: NSWindow) {
        panel.setFrame(centeredPanelFrame(for: panel), display: false)
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
            maintainFocusAssistedPresentation()
        case .inactive, .currentNonActivating:
            hidePanel(animated: true)
        }
    }

    private func maintainFocusAssistedPresentation() {
        guard panel.isVisible, visiblePanelState == .focused, NSApp.isActive else {
            hidePanel(animated: true)
            return
        }

        configurePresentationMode(for: .focused)
        viewController.setVisualState(.focused)
    }

    private func enableFocusAssistedPresentation(activatesApplication: Bool) {
        installApplicationActivationObserversIfNeeded()

        let visualState = visualStateForFocusAssistedPresentation(activatesApplication: activatesApplication)
        configurePresentationMode(for: visualState)
        viewController.setVisualState(visualState)

        if activatesApplication {
            NSApp.activate(ignoringOtherApps: true)
            showPanel(visualState: visualState) {
                self.panel.makeKeyAndOrderFront(nil)
            }
        } else {
            showPanel(visualState: visualState) {
                self.panel.orderFrontRegardless()
            }
        }
    }

    private func visualStateForFocusAssistedPresentation(
        activatesApplication: Bool
    ) -> LooperContinuationPanelVisualState {
        if activatesApplication || NSApp.isActive {
            return .focused
        }

        return .resting
    }

    private func configurePresentationMode(for visualState: LooperContinuationPanelVisualState) {
        switch visualState {
        case .focused:
            panel.allowsKeyAndMainPresentation = true
            panel.styleMask.remove(.nonactivatingPanel)
            NSApp.setActivationPolicy(.regular)
        case .resting:
            panel.allowsKeyAndMainPresentation = false
            panel.styleMask.insert(.nonactivatingPanel)
            NSApp.setActivationPolicy(.accessory)
        }
        isFocusAssistedPresentationActive = true
    }

    private func restoreHiddenPresentationMode() {
        panel.allowsKeyAndMainPresentation = false
        panel.styleMask.insert(.nonactivatingPanel)
        NSApp.setActivationPolicy(.accessory)
        isFocusAssistedPresentationActive = false
    }

    private func showPanel(
        visualState: LooperContinuationPanelVisualState,
        orderPanel: () -> Void
    ) {
        let wasHidden = visiblePanelState == nil || !panel.isVisible
        let targetFrame = centeredPanelFrame(for: panel)
        advancePanelAnimationGeneration()
        visiblePanelState = visualState

        if wasHidden {
            panel.alphaValue = PanelAnimation.hiddenAlpha
            panel.setFrame(
                scaledPanelFrame(from: targetFrame, scale: PanelAnimation.appearanceInitialScale),
                display: false
            )
        } else {
            panel.setFrame(targetFrame, display: true)
        }

        orderPanel()

        guard wasHidden else {
            panel.alphaValue = PanelAnimation.visibleAlpha
            return
        }

        animatePanel(to: targetFrame, alpha: PanelAnimation.visibleAlpha, duration: PanelAnimation.appearanceDuration)
    }

    private func hidePanel(animated: Bool) {
        guard visiblePanelState != nil || panel.isVisible else {
            restoreHiddenPresentationMode()
            return
        }

        let targetFrame = centeredPanelFrame(for: panel)
        visiblePanelState = nil
        viewController.setVisualState(.resting)

        guard animated else {
            panel.alphaValue = PanelAnimation.visibleAlpha
            panel.setFrame(targetFrame, display: false)
            panel.orderOut(nil)
            restoreHiddenPresentationMode()
            return
        }

        let generation = advancePanelAnimationGeneration()
        let hiddenFrame = scaledPanelFrame(from: targetFrame, scale: PanelAnimation.disappearanceFinalScale)

        NSAnimationContext.runAnimationGroup { context in
            context.duration = PanelAnimation.disappearanceDuration
            context.timingFunction = CAMediaTimingFunction(
                controlPoints: Float(PanelAnimation.easeInControlPoint1.x),
                Float(PanelAnimation.easeInControlPoint1.y),
                Float(PanelAnimation.easeInControlPoint2.x),
                Float(PanelAnimation.easeInControlPoint2.y)
            )
            panel.animator().alphaValue = PanelAnimation.hiddenAlpha
            panel.animator().setFrame(hiddenFrame, display: true)
        } completionHandler: { [weak self] in
            Task { @MainActor in
                guard let self, generation == self.panelAnimationGeneration else {
                    return
                }

                self.panel.orderOut(nil)
                self.panel.alphaValue = PanelAnimation.visibleAlpha
                self.panel.setFrame(targetFrame, display: false)
                self.restoreHiddenPresentationMode()
            }
        }
    }

    private func animatePanel(to frame: NSRect, alpha: CGFloat, duration: TimeInterval) {
        NSAnimationContext.runAnimationGroup { context in
            context.duration = duration
            context.timingFunction = CAMediaTimingFunction(
                controlPoints: Float(PanelAnimation.easeOutControlPoint1.x),
                Float(PanelAnimation.easeOutControlPoint1.y),
                Float(PanelAnimation.easeOutControlPoint2.x),
                Float(PanelAnimation.easeOutControlPoint2.y)
            )
            panel.animator().alphaValue = alpha
            panel.animator().setFrame(frame, display: true)
        }
    }

    private func centeredPanelFrame(for panel: NSWindow) -> NSRect {
        guard let visibleFrame = NSScreen.main?.visibleFrame else {
            return NSRect(origin: .zero, size: panelFrameSize(for: panel))
        }

        let size = panelFrameSize(for: panel)
        return NSRect(
            x: visibleFrame.midX - size.width / 2,
            y: visibleFrame.midY - size.height / 2,
            width: size.width,
            height: size.height
        )
    }

    private func panelFrameSize(for panel: NSWindow) -> NSSize {
        let contentRect = NSRect(origin: .zero, size: LooperContinuationActivityPublisher.UtilityPanelLayout.contentSize)
        return panel.frameRect(forContentRect: contentRect).size
    }

    private func scaledPanelFrame(from frame: NSRect, scale: CGFloat) -> NSRect {
        let scaledSize = NSSize(width: frame.width * scale, height: frame.height * scale)
        return NSRect(
            x: frame.midX - scaledSize.width / 2,
            y: frame.midY - scaledSize.height / 2,
            width: scaledSize.width,
            height: scaledSize.height
        )
    }

    @discardableResult
    private func advancePanelAnimationGeneration() -> Int {
        panelAnimationGeneration += 1
        return panelAnimationGeneration
    }

    private func installApplicationActivationObserversIfNeeded() {
        guard applicationActivationObservers.isEmpty else {
            return
        }

        let notificationCenter = NotificationCenter.default
        applicationActivationObservers.replace(with: [
            notificationCenter.addObserver(
                forName: NSApplication.didResignActiveNotification,
                object: NSApp,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in
                    self?.updateVisiblePanelVisualState()
                }
            },
            notificationCenter.addObserver(
                forName: NSApplication.didBecomeActiveNotification,
                object: NSApp,
                queue: .main
            ) { [weak self] _ in
                Task { @MainActor in
                    self?.updateVisiblePanelVisualState()
                }
            },
        ])
    }

    private func updateVisiblePanelVisualState() {
        guard visiblePanelState != nil else {
            return
        }

        guard NSApp.isActive else {
            hidePanel(animated: true)
            return
        }

        let visualState = visualStateForFocusAssistedPresentation(activatesApplication: false)
        guard visualState != visiblePanelState else {
            return
        }

        configurePresentationMode(for: visualState)
        viewController.setVisualState(visualState)
        visiblePanelState = visualState
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

    private enum Layout {
        static let looperOrbResourceName = "notification-orb"
        static let fallbackStatusIconResourceName = "looper-status-icon"
    }

    var descriptor: LooperContinuationActivityDescriptor?
    private var hostingView: NSHostingView<LooperContinuationActivityGlassPanelView>?
    private var visualState = LooperContinuationPanelVisualState.resting
    private lazy var looperLogoImage = Self.looperLogoImage()

    override func loadView() {
        let hostingView = NSHostingView(
            rootView: makeGlassPanelView(
                title: Content.fallbackTitle,
                subtitle: Content.fallbackSubtitle
            )
        )
        hostingView.frame = NSRect(
            origin: .zero,
            size: LooperContinuationActivityPublisher.UtilityPanelLayout.contentSize
        )
        hostingView.wantsLayer = true
        hostingView.layer?.backgroundColor = NSColor.clear.cgColor
        view = hostingView
        self.hostingView = hostingView
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
        activity.userInfo = descriptor.userInfo
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.webpageURL = descriptor.webpageURL
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
        refreshLabels()
    }

    private func makeGlassPanelView(title: String, subtitle: String) -> LooperContinuationActivityGlassPanelView {
        LooperContinuationActivityGlassPanelView(
            title: title,
            subtitle: subtitle,
            logoImage: looperLogoImage,
            visualState: visualState
        )
    }

    func setVisualState(_ visualState: LooperContinuationPanelVisualState) {
        guard self.visualState != visualState else {
            return
        }

        self.visualState = visualState
        refreshGlassPanelView()
    }

    private static func looperLogoImage() -> NSImage? {
        for resourceName in [Layout.looperOrbResourceName, Layout.fallbackStatusIconResourceName] {
            if let image = imageResource(named: resourceName, fileExtension: "png") {
                return image
            }
        }

        return NSWorkspace.shared.icon(forFile: Bundle.main.bundlePath)
    }

    private static func imageResource(named resourceName: String, fileExtension: String) -> NSImage? {
        guard let url = Bundle.main.url(forResource: resourceName, withExtension: fileExtension) else {
            return nil
        }

        return NSImage(contentsOf: url)
    }

    private func refreshLabels() {
        refreshGlassPanelView()
    }

    private func refreshGlassPanelView() {
        let subtitle = [
            descriptor?.title.nilIfEmpty,
            descriptor?.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle]?.nilIfEmpty,
        ]
        .compactMap { $0 }
        .joined(separator: " - ")
        hostingView?.rootView = makeGlassPanelView(
            title: descriptor == nil ? Content.fallbackTitle : Content.statusText,
            subtitle: subtitle.nilIfEmpty ?? Content.fallbackSubtitle
        )
    }
}

private struct LooperContinuationActivityGlassPanelView: View {
    private enum Layout {
        static let canvasSize = CGSize(width: 620, height: 160)
        static let glassOrigin = CGPoint(x: 36, y: 24)
        static let glassSize = CGSize(width: 544, height: 112)
        static let glassCornerRadius: CGFloat = 48
        static let contentLeadingPadding: CGFloat = 30
        static let contentTrailingPadding: CGFloat = 30
        static let textColumnMaxWidth: CGFloat = 360
        static let textRowSpacing: CGFloat = 8
        static let markColumnWidth: CGFloat = 102
        static let markColumnHeight: CGFloat = 86
        static let logoSize: CGFloat = 68
        static let badgeSize: CGFloat = 23
        static let badgeIconSize: CGFloat = 12
        static let logoCenterX: CGFloat = 45
        static let logoCenterY: CGFloat = 52
        static let badgeCenterX: CGFloat = 70
        static let badgeCenterY: CGFloat = 29
        static let titleFontSize: CGFloat = 24
        static let subtitleFontSize: CGFloat = 14.5
        static let titleFontWeight: Font.Weight = .bold
        static let subtitleFontWeight: Font.Weight = .semibold
        static let brandFontDesign: Font.Design = .rounded
        static let focusedSurfaceScale: CGFloat = 1
        static let restingSurfaceScale: CGFloat = 0.985
        static let focusedContentOpacity = 1.0
        static let restingContentOpacity = 1.0
        static let glassLayerMergeSpacing: CGFloat = 0
        static let outerGlassOutset: CGFloat = 7
        static let outerGlassCornerRadius: CGFloat = 54
        static let innerGlassInset: CGFloat = 10
        static let innerGlassCornerRadius: CGFloat = 38
        static let stateAnimationDuration: TimeInterval = 0.22
        static let stateAnimationBounce = 0.08
        static let stateAnimationBlendDuration: TimeInterval = 0
        static let fallbackStrokeOpacity = 0.26
        static let fallbackShadowOpacity = 0.35
        static let fallbackShadowRadius: CGFloat = 24
        static let fallbackShadowY: CGFloat = 14
        static let markShadowOpacity = 0.3
        static let markShadowRadius: CGFloat = 10
        static let markShadowY: CGFloat = 5
        static let badgeStrokeOpacity = 0.22
        static let badgeStrokeWidth: CGFloat = 0.8
        static let titleForegroundOpacity = 1.0
        static let subtitleForegroundOpacity = 0.9
        static let titleKeyShadowOpacity = 0.34
        static let subtitleKeyShadowOpacity = 0.28
        static let textKeyShadowRadius: CGFloat = 0.8
        static let textShadowY: CGFloat = 1
    }

    let title: String
    let subtitle: String
    let logoImage: NSImage?
    let visualState: LooperContinuationPanelVisualState

    var body: some View {
        ZStack {
            Color.clear
            if #available(macOS 26.0, *) {
                liquidGlassPanel
            } else {
                fallbackPanel
            }
        }
        .frame(width: Layout.canvasSize.width, height: Layout.canvasSize.height)
    }

    @available(macOS 26.0, *)
    private var liquidGlassPanel: some View {
        ZStack {
            GlassEffectContainer(spacing: Layout.glassLayerMergeSpacing) {
                clearGlassShell
                    .scaleEffect(surfaceScale, anchor: .center)
                    .position(panelCenter)
            }

            panelContent
                .frame(width: Layout.glassSize.width, height: Layout.glassSize.height)
                .opacity(contentOpacity)
                .scaleEffect(surfaceScale, anchor: .center)
                .position(panelCenter)
        }
        .frame(width: Layout.canvasSize.width, height: Layout.canvasSize.height)
        .animation(stateAnimation, value: visualState)
    }

    @available(macOS 26.0, *)
    private var clearGlassShell: some View {
        ZStack {
            clearGlassLayer(size: outerGlassSize, shape: outerPanelShape)
            clearGlassLayer(size: Layout.glassSize, shape: panelShape, glass: panelGlass)
            clearGlassLayer(size: innerGlassSize, shape: innerPanelShape)
        }
        .frame(width: Layout.glassSize.width, height: Layout.glassSize.height)
        .compositingGroup()
    }

    @available(macOS 26.0, *)
    private func clearGlassLayer<S: Shape>(size: CGSize, shape: S, glass: Glass = .clear) -> some View {
        Color.clear
            .frame(width: size.width, height: size.height)
            .glassEffect(glass, in: shape)
    }

    @available(macOS 26.0, *)
    private var panelGlass: Glass {
        .clear
            .interactive(visualState.isFocused)
    }

    private var fallbackPanel: some View {
        panelContent
            .frame(width: Layout.glassSize.width, height: Layout.glassSize.height)
            .background(.regularMaterial, in: panelShape)
            .overlay {
                panelShape.stroke(.white.opacity(Layout.fallbackStrokeOpacity), lineWidth: 1)
            }
            .position(panelCenter)
            .frame(width: Layout.canvasSize.width, height: Layout.canvasSize.height)
            .shadow(
                color: .black.opacity(Layout.fallbackShadowOpacity),
                radius: Layout.fallbackShadowRadius,
                y: Layout.fallbackShadowY
            )
    }

    private var panelContent: some View {
        HStack(alignment: .center, spacing: 0) {
            textContent

            Spacer(minLength: 0)

            handoffMark
        }
        .padding(.leading, Layout.contentLeadingPadding)
        .padding(.trailing, Layout.contentTrailingPadding)
    }

    private var textContent: some View {
        VStack(alignment: .leading, spacing: Layout.textRowSpacing) {
            Text(title)
                .font(titleFont)
                .lineLimit(1)
                .truncationMode(.tail)
                .foregroundStyle(.white.opacity(Layout.titleForegroundOpacity))
                .shadow(
                    color: .black.opacity(Layout.titleKeyShadowOpacity),
                    radius: Layout.textKeyShadowRadius,
                    y: Layout.textShadowY
                )

            Text(subtitle)
                .font(subtitleFont)
                .lineLimit(1)
                .truncationMode(.tail)
                .foregroundStyle(.white.opacity(Layout.subtitleForegroundOpacity))
                .shadow(
                    color: .black.opacity(Layout.subtitleKeyShadowOpacity),
                    radius: Layout.textKeyShadowRadius,
                    y: Layout.textShadowY
                )
        }
        .frame(width: Layout.textColumnMaxWidth, alignment: .leading)
    }

    private var titleFont: Font {
        .system(
            size: Layout.titleFontSize,
            weight: Layout.titleFontWeight,
            design: Layout.brandFontDesign
        )
    }

    private var subtitleFont: Font {
        .system(
            size: Layout.subtitleFontSize,
            weight: Layout.subtitleFontWeight,
            design: Layout.brandFontDesign
        )
    }

    private var handoffMark: some View {
        ZStack {
            logoContent
                .frame(width: Layout.logoSize, height: Layout.logoSize)
                .shadow(
                    color: .black.opacity(Layout.markShadowOpacity),
                    radius: Layout.markShadowRadius,
                    y: Layout.markShadowY
                )
                .position(x: Layout.logoCenterX, y: Layout.logoCenterY)
                .zIndex(0)

            phoneBadge
                .position(x: Layout.badgeCenterX, y: Layout.badgeCenterY)
                .zIndex(1)
        }
        .frame(width: Layout.markColumnWidth, height: Layout.markColumnHeight)
    }

    private var logoContent: some View {
        ZStack {
            if let logoImage {
                Image(nsImage: logoImage)
                    .resizable()
                    .scaledToFit()
                    .frame(width: Layout.logoSize, height: Layout.logoSize)
            } else {
                Image(systemName: "infinity")
                    .font(.system(size: Layout.logoSize * 0.55, weight: .semibold))
                    .symbolRenderingMode(.hierarchical)
            }
        }
    }

    private var phoneBadge: some View {
        Group {
            if #available(macOS 26.0, *) {
                badgeContent
                    .frame(width: Layout.badgeSize, height: Layout.badgeSize)
                    .glassEffect(.clear, in: Circle())
            } else {
                badgeContent
                    .frame(width: Layout.badgeSize, height: Layout.badgeSize)
                    .background(.thinMaterial, in: Circle())
            }
        }
        .overlay {
            Circle()
                .stroke(.white.opacity(Layout.badgeStrokeOpacity), lineWidth: Layout.badgeStrokeWidth)
        }
    }

    private var badgeContent: some View {
        Image(systemName: "iphone")
            .font(.system(size: Layout.badgeIconSize, weight: .semibold))
            .symbolRenderingMode(.hierarchical)
            .foregroundStyle(.primary)
    }

    private var panelShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: Layout.glassCornerRadius, style: .continuous)
    }

    private var outerPanelShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: Layout.outerGlassCornerRadius, style: .continuous)
    }

    private var innerPanelShape: RoundedRectangle {
        RoundedRectangle(cornerRadius: Layout.innerGlassCornerRadius, style: .continuous)
    }

    private var outerGlassSize: CGSize {
        CGSize(
            width: Layout.glassSize.width + Layout.outerGlassOutset * 2,
            height: Layout.glassSize.height + Layout.outerGlassOutset * 2
        )
    }

    private var innerGlassSize: CGSize {
        CGSize(
            width: Layout.glassSize.width - Layout.innerGlassInset * 2,
            height: Layout.glassSize.height - Layout.innerGlassInset * 2
        )
    }

    private var surfaceScale: CGFloat {
        visualState.isFocused ? Layout.focusedSurfaceScale : Layout.restingSurfaceScale
    }

    private var contentOpacity: Double {
        visualState.isFocused ? Layout.focusedContentOpacity : Layout.restingContentOpacity
    }

    private var stateAnimation: Animation {
        .spring(
            duration: Layout.stateAnimationDuration,
            bounce: Layout.stateAnimationBounce,
            blendDuration: Layout.stateAnimationBlendDuration
        )
    }

    private var panelCenter: CGPoint {
        CGPoint(
            x: Layout.glassOrigin.x + Layout.glassSize.width / 2,
            y: Layout.glassOrigin.y + Layout.glassSize.height / 2
        )
    }
}

private extension String {
    var nilIfEmpty: String? {
        trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : self
    }
}

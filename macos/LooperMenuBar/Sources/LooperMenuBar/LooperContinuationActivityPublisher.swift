import AppKit
import CoreSpotlight
import LooperMenuBarCore

@MainActor
final class LooperContinuationActivityPublisher {
    fileprivate enum Layout {
        static let windowSize = NSSize(width: 440, height: 168)
    }

    private enum Timing {
        static let currentActivityRefreshInterval: Duration = .seconds(3)
    }

    private let activityPanel = LooperContinuationActivityPanel()
    private var currentActivity: NSUserActivity?
    private var currentDescriptor: LooperContinuationActivityDescriptor?
    private var currentActivityRefreshTask: Task<Void, Never>?

    func publish(_ descriptor: LooperContinuationActivityDescriptor) {
        guard descriptor != currentDescriptor else {
            activityPanel.refreshCurrentActivity()
            return
        }

        let isNewActivity = currentActivity == nil
        let activity = currentActivity ?? NSUserActivity(activityType: LooperContinuationActivity.activityType)
        configure(activity, with: descriptor)
        activityPanel.attach(activity, descriptor: descriptor, shouldOrderFront: isNewActivity)
        activity.needsSave = true
        activity.becomeCurrent()

        currentActivity = activity
        currentDescriptor = descriptor
        startCurrentActivityRefreshLoop()
    }

    func invalidate() {
        currentActivityRefreshTask?.cancel()
        currentActivity?.invalidate()
        activityPanel.detach()
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
                try? await Task.sleep(for: Timing.currentActivityRefreshInterval)
                self?.refreshCurrentActivity()
            }
        }
    }

    private func refreshCurrentActivity() {
        activityPanel.refreshCurrentActivity()
    }

    private func configure(
        _ activity: NSUserActivity,
        with descriptor: LooperContinuationActivityDescriptor
    ) {
        activity.title = activityTitle(for: descriptor)
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.persistentIdentifier = NSUserActivityPersistentIdentifier(
            descriptor.targetContentIdentifier
        )
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
        activity.keywords = activityKeywords(for: descriptor)
        activity.contentAttributeSet = contentAttributeSet(for: descriptor)
        activity.userInfo = descriptor.userInfo
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
private final class LooperContinuationActivityPanel {
    private let viewController = LooperContinuationActivityPanelViewController()
    private lazy var window: NSWindow = {
        let window = NSWindow(
            contentRect: NSRect(
                origin: .zero,
                size: LooperContinuationActivityPublisher.Layout.windowSize
            ),
            styleMask: [.titled, .closable, .miniaturizable],
            backing: .buffered,
            defer: false
        )
        window.backgroundColor = .windowBackgroundColor
        window.collectionBehavior = [.canJoinAllSpaces, .managed]
        window.isReleasedWhenClosed = false
        window.level = .normal
        window.title = LooperContinuationActivityPanelViewController.Content.appName
        window.contentViewController = viewController
        window.setContentSize(LooperContinuationActivityPublisher.Layout.windowSize)
        positionWindow(window)
        return window
    }()

    func attach(
        _ activity: NSUserActivity,
        descriptor: LooperContinuationActivityDescriptor,
        shouldOrderFront: Bool
    ) {
        _ = window
        viewController.descriptor = descriptor
        viewController.userActivity = activity
        if shouldOrderFront {
            window.makeKeyAndOrderFront(nil)
        }
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

    private func positionWindow(_ window: NSWindow) {
        guard let visibleFrame = NSScreen.main?.visibleFrame else {
            window.center()
            return
        }

        let x = visibleFrame.midX - (LooperContinuationActivityPublisher.Layout.windowSize.width / 2)
        let y = visibleFrame.maxY - LooperContinuationActivityPublisher.Layout.windowSize.height - Spacing.windowTopInset
        window.setFrameOrigin(NSPoint(x: x, y: y))
    }

    private enum Spacing {
        static let windowTopInset: CGFloat = 72
    }
}

@MainActor
private final class LooperContinuationActivityPanelViewController: NSViewController {
    enum Content {
        static let appName = "looper"
        static let fallbackSubtitle = "Mac session"
        static let fallbackPreview = "Running on this Mac"
    }

    private enum Layout {
        static let horizontalInset: CGFloat = 22
        static let topInset: CGFloat = 20
        static let verticalGap: CGFloat = 8
        static let titleHeight: CGFloat = 28
        static let subtitleHeight: CGFloat = 22
        static let previewHeight: CGFloat = 54
        static let cornerRadius: CGFloat = 12
    }

    var descriptor: LooperContinuationActivityDescriptor? {
        didSet {
            renderDescriptor()
        }
    }

    private let titleLabel = NSTextField(labelWithString: Content.appName)
    private let subtitleLabel = NSTextField(labelWithString: Content.fallbackSubtitle)
    private let previewLabel = NSTextField(wrappingLabelWithString: Content.fallbackPreview)

    override func loadView() {
        let rootView = NSView(
            frame: NSRect(
                origin: .zero,
                size: LooperContinuationActivityPublisher.Layout.windowSize
            )
        )
        rootView.wantsLayer = true
        rootView.layer?.cornerRadius = Layout.cornerRadius
        rootView.layer?.cornerCurve = .continuous

        configureLabels()
        rootView.addSubview(titleLabel)
        rootView.addSubview(subtitleLabel)
        rootView.addSubview(previewLabel)
        view = rootView
        renderDescriptor()
    }

    override func viewDidLayout() {
        super.viewDidLayout()
        let contentWidth = max(0, view.bounds.width - (Layout.horizontalInset * 2))
        var y = view.bounds.height - Layout.topInset - Layout.titleHeight
        titleLabel.frame = NSRect(
            x: Layout.horizontalInset,
            y: y,
            width: contentWidth,
            height: Layout.titleHeight
        )

        y -= Layout.verticalGap + Layout.subtitleHeight
        subtitleLabel.frame = NSRect(
            x: Layout.horizontalInset,
            y: y,
            width: contentWidth,
            height: Layout.subtitleHeight
        )

        y -= Layout.verticalGap + Layout.previewHeight
        previewLabel.frame = NSRect(
            x: Layout.horizontalInset,
            y: y,
            width: contentWidth,
            height: Layout.previewHeight
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
        activity.targetContentIdentifier = descriptor.targetContentIdentifier
        activity.addUserInfoEntries(from: descriptor.userInfo)
        activity.requiredUserInfoKeys = Set(descriptor.userInfo.keys)
        activity.isEligibleForHandoff = true
        activity.isEligibleForSearch = false
        activity.isEligibleForPublicIndexing = false
    }

    private func configureLabels() {
        titleLabel.font = .systemFont(ofSize: FontSize.title, weight: .semibold)
        titleLabel.lineBreakMode = .byTruncatingTail
        titleLabel.textColor = .labelColor

        subtitleLabel.font = .systemFont(ofSize: FontSize.subtitle, weight: .regular)
        subtitleLabel.lineBreakMode = .byTruncatingTail
        subtitleLabel.textColor = .secondaryLabelColor

        previewLabel.font = .systemFont(ofSize: FontSize.preview, weight: .regular)
        previewLabel.lineBreakMode = .byTruncatingTail
        previewLabel.maximumNumberOfLines = Preview.maximumLineCount
        previewLabel.textColor = .tertiaryLabelColor
    }

    private func renderDescriptor() {
        guard isViewLoaded else {
            return
        }

        titleLabel.stringValue = descriptor?.title ?? Content.appName
        subtitleLabel.stringValue = descriptor?.userInfo[LooperContinuationActivity.UserInfoKey.sessionSubtitle]
            ?? Content.fallbackSubtitle
        previewLabel.stringValue = descriptor?.userInfo[LooperContinuationActivity.UserInfoKey.sessionPreview]
            ?? Content.fallbackPreview
    }

    private enum FontSize {
        static let title: CGFloat = 17
        static let subtitle: CGFloat = 13
        static let preview: CGFloat = 12
    }

    private enum Preview {
        static let maximumLineCount = 3
    }
}

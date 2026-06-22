import AppKit
import Foundation

@MainActor
final class LooperDiagnosticsWindowController: NSObject, NSWindowDelegate {
    private enum Layout {
        static let title = "Looper Diagnostics"
        static let accessibilityIdentifier = "looper.diagnostics.window"
        static let textAccessibilityIdentifier = "looper.diagnostics.report"
        static let loadingText = "Looper Diagnostics\nState: loading"
        static let contentInsets = NSEdgeInsets(top: 16, left: 16, bottom: 16, right: 16)
        static let windowWidth: CGFloat = 860
        static let windowHeight: CGFloat = 680
        static let minimumWindowWidth: CGFloat = 640
        static let minimumWindowHeight: CGFloat = 420
        static let fallbackScreenWidth: CGFloat = 1440
        static let fallbackScreenHeight: CGFloat = 900
    }

    private var window: NSWindow?
    private var textView: NSTextView?

    func showLoading() {
        show(report: Layout.loadingText)
    }

    func show(report: String) {
        let window = existingOrCreateWindow()
        textView?.string = report
        NSApp.setActivationPolicy(.regular)
        NSApp.unhide(nil)
        window.makeKeyAndOrderFront(nil)
        window.orderFrontRegardless()
        NSApp.activate(ignoringOtherApps: true)
    }

    func windowWillClose(_ notification: Notification) {
        NSApp.setActivationPolicy(.accessory)
    }

    private func existingOrCreateWindow() -> NSWindow {
        if let window {
            return window
        }

        let window = NSWindow(
            contentRect: centeredFrame(),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.title = Layout.title
        window.identifier = NSUserInterfaceItemIdentifier(Layout.accessibilityIdentifier)
        window.minSize = NSSize(width: Layout.minimumWindowWidth, height: Layout.minimumWindowHeight)
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.contentView = makeContentView()
        self.window = window
        return window
    }

    private func makeContentView() -> NSView {
        let scrollView = NSScrollView()
        scrollView.drawsBackground = false
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = true
        scrollView.autoresizingMask = [.width, .height]

        let textView = NSTextView()
        textView.identifier = NSUserInterfaceItemIdentifier(Layout.textAccessibilityIdentifier)
        textView.isEditable = false
        textView.isSelectable = true
        textView.drawsBackground = false
        textView.textContainerInset = NSSize(
            width: Layout.contentInsets.left,
            height: Layout.contentInsets.top
        )
        textView.font = .monospacedSystemFont(ofSize: NSFont.systemFontSize, weight: .regular)
        textView.textColor = .labelColor
        textView.autoresizingMask = [.width]
        textView.isHorizontallyResizable = true
        textView.isVerticallyResizable = true
        textView.textContainer?.containerSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.textContainer?.widthTracksTextView = false

        scrollView.documentView = textView
        self.textView = textView
        return scrollView
    }

    private func centeredFrame() -> NSRect {
        let visibleFrame = NSScreen.main?.visibleFrame ?? NSRect(
            x: 0,
            y: 0,
            width: Layout.fallbackScreenWidth,
            height: Layout.fallbackScreenHeight
        )
        let size = NSSize(width: Layout.windowWidth, height: Layout.windowHeight)
        return NSRect(
            x: visibleFrame.midX - (size.width / 2),
            y: visibleFrame.midY - (size.height / 2),
            width: size.width,
            height: size.height
        )
    }
}

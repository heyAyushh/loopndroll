import AppKit
import Foundation
import LooperMenuBarCore

@MainActor
private final class LooperMenuBarAppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private enum Layout {
        static let statusIconSize = NSSize(width: 18, height: 18)
        static let visibleThreadLimitPerSection = 5
        static let statusIconResourceName = "looper-status-icon"
        static let appDisplayName = "looper"
        static let threadMenuTitleCharacterLimit = 38
        static let detachServerOnQuitKey = "detachServerOnQuit"
        static let continuationRefreshInterval: Duration = .seconds(20)
        static let activationPolicy: NSApplication.ActivationPolicy = .accessory
        static let handoffFocusAssistMenuTitle = "Handoff Focus Assist"
        static let detailsMenuTitle = "Details"
        static let settingsMenuTitle = "Settings"
    }

    private let client: HTTPControlPlaneClient
    private let lifecycle: LooperLifecycleCoordinator
    private let continuationPublisher = LooperContinuationActivityPublisher()
    private var statusItem: NSStatusItem?
    private var menu: NSMenu?
    private var continuationRefreshTask: Task<Void, Never>?
    private var mobileHealth: MobileHealthResponse?
    private var devinProbe: DevinAcpBridgeProbe?

    override init() {
        let endpointStore = ControlPlaneEndpointStore()
        let client = HTTPControlPlaneClient(endpointStore: endpointStore)
        self.client = client
        self.lifecycle = LooperLifecycleCoordinator(
            client: client,
            service: BundledControlPlaneService(endpointStore: endpointStore)
        )
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(Layout.activationPolicy)
        continuationPublisher.focusAssist = handoffFocusAssist
        installStatusItem()
        continuationPublisher.publish(LooperContinuationActivityBuilder.genericDescriptor())
        startContinuationRefreshLoop()
        Task {
            _ = await lifecycle.registerOnLaunch()
            await refreshMenu()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        continuationRefreshTask?.cancel()
        continuationPublisher.invalidate()
        if !detachServerOnQuit {
            _ = lifecycle.unregisterBeforeQuit()
        }
    }

    func application(
        _: NSApplication,
        willContinueUserActivityWithType userActivityType: String
    ) -> Bool {
        LooperContinuationActivity.isSupportedActivityType(userActivityType)
    }

    func application(
        _: NSApplication,
        continue userActivity: NSUserActivity,
        restorationHandler: @escaping ([any NSUserActivityRestoring]) -> Void
    ) -> Bool {
        guard LooperContinuationActivity.isSupportedActivityType(userActivity.activityType) else {
            return false
        }

        restorationHandler([])
        Task {
            await openContinuationActivity(userActivity)
        }
        return true
    }

    func menuWillOpen(_ menu: NSMenu) {
        Task {
            await refreshMenu()
        }
    }

    private func installStatusItem() {
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
        applyHumanStatus(.starting(detachOnQuit: detachServerOnQuit), to: item)
        continuationPublisher.attachHost(item.button)

        let menu = makeMenu(snapshot: nil, error: nil)
        item.menu = menu
        self.menu = menu
        statusItem = item
    }

    private func refreshMenu() async {
        do {
            let snapshot = try await client.fetchDesktopSnapshot()
            mobileHealth = try? await client.fetchMobileHealth()
            publishContinuationActivity(from: snapshot)
            replaceMenu(snapshot: snapshot, error: nil)
        } catch {
            mobileHealth = nil
            continuationPublisher.publishFallbackIfIdle(LooperContinuationActivityBuilder.genericDescriptor())
            replaceMenu(snapshot: nil, error: error)
        }
    }

    private func startContinuationRefreshLoop() {
        continuationRefreshTask?.cancel()
        continuationRefreshTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refreshContinuationActivity()
                try? await Task.sleep(for: Layout.continuationRefreshInterval)
            }
        }
    }

    private func refreshContinuationActivity() async {
        do {
            let snapshot = try await client.fetchDesktopSnapshot()
            mobileHealth = try? await client.fetchMobileHealth()
            publishContinuationActivity(from: snapshot)
        } catch {
            continuationPublisher.publishFallbackIfIdle(LooperContinuationActivityBuilder.genericDescriptor())
        }
    }

    private func publishContinuationActivity(from snapshot: DesktopSnapshotResponse) {
        continuationPublisher.publish(
            LooperContinuationActivityBuilder.descriptor(
                from: snapshot,
                handoffBaseURL: mobileHealth?.preferredHandoffBaseURL
            )
        )
    }

    private func openContinuationActivity(_ activity: NSUserActivity) async {
        guard let target = await continuationOpenTarget(for: activity) else {
            return
        }

        _ = openThread(target)
    }

    private func continuationOpenTarget(for activity: NSUserActivity) async -> LooperThreadOpenTarget? {
        guard let threadID = LooperContinuationActivity.sessionID(from: activity) else {
            return nil
        }

        if let snapshot = try? await client.fetchDesktopSnapshot(),
           let thread = snapshot.threads.first(where: { $0.threadId == threadID })
        {
            return LooperThreadOpenTarget(
                threadId: thread.threadId,
                transcriptPath: thread.transcriptPath,
                workingDirectory: thread.cwd,
                agentPath: thread.capabilities.agentPath
            )
        }

        return LooperThreadOpenTarget(
            threadId: threadID,
            transcriptPath: nil,
            workingDirectory: nil
        )
    }

    private func replaceMenu(snapshot: DesktopSnapshotResponse?, error: Error?) {
        updateStatusItem(snapshot: snapshot, error: error)
        let menu = makeMenu(snapshot: snapshot, error: error)
        statusItem?.menu = menu
        self.menu = menu
    }

    private func makeMenu(snapshot: DesktopSnapshotResponse?, error: Error?) -> NSMenu {
        let menu = NSMenu()
        menu.delegate = self
        addDisabledItem(Layout.appDisplayName, to: menu)
        menu.addItem(NSMenuItem.separator())
        addDetailsItem(snapshot: snapshot, error: error, to: menu)

        if let snapshot {
            addSnapshotThreadSections(snapshot, to: menu)
        }

        menu.addItem(NSMenuItem.separator())
        addActionItem("Refresh", action: #selector(refreshMenuAction(_:)), keyEquivalent: "r", to: menu)
        addSettingsItem(to: menu)
        addActionItem("Stop Server", action: #selector(stopServerAction(_:)), keyEquivalent: "", to: menu)
        menu.addItem(NSMenuItem.separator())
        menu.addItem(NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        return menu
    }

    private func addDetailsItem(snapshot: DesktopSnapshotResponse?, error: Error?, to menu: NSMenu) {
        let item = NSMenuItem(title: Layout.detailsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.detailsMenuTitle)
        submenu.autoenablesItems = false

        if let snapshot {
            addSnapshotDetails(snapshot, to: submenu)
        } else if error != nil {
            addUnavailableDetails(to: submenu)
        } else {
            addStartingDetails(to: submenu)
        }

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addSnapshotDetails(_ snapshot: DesktopSnapshotResponse, to menu: NSMenu) {
        let status = LooperHumanStatus.from(
            snapshot: snapshot,
            mobileHealth: mobileHealth,
            detachOnQuit: detachServerOnQuit
        )
        addDisabledItem("Status: \(status.title)", to: menu)
        addDisabledItem("Codex hooks: \(snapshot.controlPlane.hooks.health)", to: menu)
        addDisabledItem("iPhone: \(mobileStatusTitle())", to: menu)
        addDisabledItem("Lifecycle: \(status.lifecycle)", to: menu)
        addDisabledItem("Chats: \(snapshot.activeThreadCount) active, \(snapshot.archivedThreadCount) archived", to: menu)
        addDisabledItem("Codex: \(snapshot.controlPlane.codexServers.count) local servers", to: menu)
        addDisabledItem("Grok Build: \(snapshot.grokBuildStatusTitle)", to: menu)
        addDisabledItem("Grok hooks: \(snapshot.grokBuildHooksTitle)", to: menu)
        if let grokBuild = snapshot.grokBuild {
            addDisabledItem(
                "Grok sessions: \(grokBuild.activeSessionCount) active / \(grokBuild.sessionCount) total",
                to: menu
            )
        }
        addDisabledItem("Devin: \(devinStatusTitle(snapshot.devinDesktop.acpBridge))", to: menu)
        addDisabledItem("Automations: \(coveredAutomationCount(snapshot))/\(snapshot.automations.count) covered", to: menu)
        addDisabledItem("Goals: \(snapshot.goals.count)", to: menu)
        addDevinBridgeItems(snapshot.devinDesktop.acpBridge, to: menu)
    }

    private func addSnapshotThreadSections(_ snapshot: DesktopSnapshotResponse, to menu: NSMenu) {
        let sections = LooperMenuContent.buildThreadSections(from: snapshot.threads)
        guard !sections.isEmpty else {
            return
        }

        addThreadSections(sections, to: menu)
    }

    private func addUnavailableDetails(to menu: NSMenu) {
        addDisabledItem("Status: Unavailable", to: menu)
        addDisabledItem("Rust service unavailable", to: menu)
        addDisabledItem("Lifecycle: \(LooperHumanStatus.unavailable(detachOnQuit: detachServerOnQuit).lifecycle)", to: menu)
    }

    private func addStartingDetails(to menu: NSMenu) {
        addDisabledItem("Status: Starting", to: menu)
        addDisabledItem("Starting local service...", to: menu)
        addDisabledItem("Lifecycle: \(LooperHumanStatus.starting(detachOnQuit: detachServerOnQuit).lifecycle)", to: menu)
    }

    private func mobileStatusTitle() -> String {
        guard let mobileHealth else {
            return "Unknown"
        }
        return mobileHealth.ok && mobileHealth.requiresAuthentication ? "Ready" : "Needs attention"
    }

    private func devinStatusTitle(_ bridge: DevinAcpBridgeStatus) -> String {
        if let devinProbe {
            return devinProbe.ready ? "\(devinProbe.name ?? devinProbe.agentId ?? "Agent") ready" : "Probe blocked"
        }

        guard bridge.available else {
            return "Unavailable"
        }

        guard let agent = bridge.defaultProbeAgent else {
            return "No agent"
        }

        return agent.probeCapable ? "\(agent.name) visible" : "\(agent.name) needs attention"
    }

    private func addDevinBridgeItems(_ bridge: DevinAcpBridgeStatus, to menu: NSMenu) {
        guard bridge.available || devinProbe != nil else {
            return
        }

        menu.addItem(NSMenuItem.separator())
        addDisabledItem("Devin Desktop", to: menu)
        addDisabledItem(bridge.summary, to: menu)

        if let devinProbe {
            addDisabledItem(devinProbe.detail, to: menu)
            if !devinProbe.blockers.isEmpty {
                addDisabledItem("Blocked: \(devinProbe.blockers.joined(separator: ", "))", to: menu)
            }
        }

        let probeItem = NSMenuItem(
            title: "Probe Devin Agent",
            action: #selector(probeDevinAcpAction(_:)),
            keyEquivalent: ""
        )
        probeItem.target = self
        probeItem.representedObject = bridge.defaultProbeAgent?.id
        probeItem.isEnabled = bridge.defaultProbeAgent != nil
        menu.addItem(probeItem)
    }

    private func coveredAutomationCount(_ snapshot: DesktopSnapshotResponse) -> Int {
        snapshot.automations.filter(\.controlPlaneCovered).count
    }

    private func addThreadSections(_ sections: [LooperMenuSection], to menu: NSMenu) {
        for (sectionIndex, section) in sections.enumerated() {
            if sectionIndex > 0 {
                menu.addItem(NSMenuItem.separator())
            }

            addDisabledItem(section.title, to: menu)

            for row in section.rows.prefix(Layout.visibleThreadLimitPerSection) {
                menu.addItem(makeThreadItem(row))
            }

            let overflowRows = Array(section.rows.dropFirst(Layout.visibleThreadLimitPerSection))
            if !overflowRows.isEmpty {
                menu.addItem(makeMoreThreadsItem(rows: overflowRows))
            }
        }
    }

    private func makeThreadItem(_ row: LooperMenuRow) -> NSMenuItem {
        let item = NSMenuItem(
            title: truncatedThreadTitle(row.title),
            action: #selector(openThreadAction(_:)),
            keyEquivalent: ""
        )
        item.target = self
        item.representedObject = row.openTarget
        setSubtitle(row.subtitle, on: item)
        return item
    }

    private func makeMoreThreadsItem(rows: [LooperMenuRow]) -> NSMenuItem {
        let item = NSMenuItem(title: "More", action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: "More")
        submenu.autoenablesItems = false
        for row in rows {
            submenu.addItem(makeThreadItem(row))
        }
        item.submenu = submenu
        return item
    }

    private func truncatedThreadTitle(_ title: String) -> String {
        let characters = Array(title)
        guard characters.count > Layout.threadMenuTitleCharacterLimit else {
            return title
        }

        let prefix = characters
            .prefix(Layout.threadMenuTitleCharacterLimit - 1)
            .map(String.init)
            .joined()
            .trimmingCharacters(in: .whitespacesAndNewlines)
        return "\(prefix)…"
    }

    private func setSubtitle(_ subtitle: String, on item: NSMenuItem) {
        if #available(macOS 14.4, *) {
            item.subtitle = subtitle
        } else {
            item.toolTip = subtitle
        }
    }

    private func addDisabledItem(_ title: String, to menu: NSMenu) {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        menu.addItem(item)
    }

    private func addActionItem(
        _ title: String,
        action: Selector,
        keyEquivalent: String,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: keyEquivalent)
        item.target = self
        menu.addItem(item)
    }

    private func addDetachServerItem(to menu: NSMenu) {
        let item = NSMenuItem(
            title: "Detach Server on Quit",
            action: #selector(toggleDetachServerOnQuitAction(_:)),
            keyEquivalent: ""
        )
        item.target = self
        item.state = detachServerOnQuit ? .on : .off
        menu.addItem(item)
    }

    private func addSettingsItem(to menu: NSMenu) {
        let item = NSMenuItem(title: Layout.settingsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.settingsMenuTitle)
        submenu.autoenablesItems = false
        addHandoffFocusAssistItem(to: submenu)
        addDetachServerItem(to: submenu)
        submenu.addItem(NSMenuItem.separator())
        addActionItem("Repair Codex & Grok Hooks", action: #selector(repairHooksAction(_:)), keyEquivalent: "", to: submenu)
        addActionItem("Clear Live Codex & Grok Hooks", action: #selector(clearLiveHooksAction(_:)), keyEquivalent: "", to: submenu)
        addActionItem("Copy Terminal Command", action: #selector(copyTerminalCommandAction(_:)), keyEquivalent: "c", to: submenu)
        item.submenu = submenu
        menu.addItem(item)
    }

    private func addHandoffFocusAssistItem(to menu: NSMenu) {
        let item = NSMenuItem(
            title: "\(Layout.handoffFocusAssistMenuTitle): \(handoffFocusAssist.statusTitle)",
            action: nil,
            keyEquivalent: ""
        )
        let submenu = NSMenu(title: Layout.handoffFocusAssistMenuTitle)
        submenu.autoenablesItems = false

        for option in LooperHandoffFocusAssist.allCases {
            let optionItem = NSMenuItem(
                title: option.menuTitle,
                action: #selector(setHandoffFocusAssistAction(_:)),
                keyEquivalent: ""
            )
            optionItem.target = self
            optionItem.representedObject = option.rawValue
            optionItem.state = handoffFocusAssist == option ? .on : .off
            submenu.addItem(optionItem)
        }

        item.submenu = submenu
        menu.addItem(item)
    }

    private var detachServerOnQuit: Bool {
        get {
            UserDefaults.standard.bool(forKey: Layout.detachServerOnQuitKey)
        }
        set {
            UserDefaults.standard.set(newValue, forKey: Layout.detachServerOnQuitKey)
        }
    }

    private var handoffFocusAssist: LooperHandoffFocusAssist {
        get {
            LooperHandoffFocusAssist.stored()
        }
        set {
            newValue.save()
            continuationPublisher.focusAssist = newValue
        }
    }

    @objc private func refreshMenuAction(_ sender: Any?) {
        Task {
            await refreshMenu()
        }
    }

    @objc private func repairHooksAction(_ sender: Any?) {
        Task {
            do {
                try await client.registerHooks()
            } catch {
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu()
        }
    }

    @objc private func clearLiveHooksAction(_ sender: Any?) {
        Task {
            do {
                try client.unregisterLiveHooks(timeout: LooperLifecycleDefaults.requestTimeoutSeconds)
            } catch {
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu()
        }
    }

    @objc private func copyTerminalCommandAction(_ sender: Any?) {
        let pasteboard = NSPasteboard.general
        pasteboard.clearContents()
        pasteboard.setString("looper", forType: .string)
    }

    @objc private func toggleDetachServerOnQuitAction(_ sender: Any?) {
        detachServerOnQuit.toggle()
        Task {
            await refreshMenu()
        }
    }

    @objc private func setHandoffFocusAssistAction(_ sender: NSMenuItem) {
        guard let rawValue = sender.representedObject as? String,
              let option = LooperHandoffFocusAssist(rawValue: rawValue)
        else {
            return
        }

        handoffFocusAssist = option
        Task {
            await refreshMenu()
        }
    }

    @objc private func stopServerAction(_ sender: Any?) {
        lifecycle.shutdownServer()
        Task {
            await refreshMenu()
        }
    }

    @objc private func probeDevinAcpAction(_ sender: NSMenuItem) {
        let agentId = sender.representedObject as? String
        Task {
            do {
                let response = try await client.probeDevinAcpBridge(agentId: agentId)
                devinProbe = response.probe
            } catch {
                devinProbe = nil
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu()
        }
    }

    @objc private func openThreadAction(_ sender: NSMenuItem) {
        guard let target = sender.representedObject as? LooperThreadOpenTarget else {
            return
        }
        openThread(target)
    }

    @discardableResult
    private func openThread(_ target: LooperThreadOpenTarget) -> Bool {
        let workspace = NSWorkspace.shared
        if let codexURL = target.codexURL,
           workspace.open(codexURL) {
            return true
        }

        guard let fallbackURL = target.firstLocalFallbackURL else {
            return false
        }
        return workspace.open(fallbackURL)
    }

    private func updateStatusItem(snapshot: DesktopSnapshotResponse?, error: Error?) {
        let status: LooperHumanStatus
        if let snapshot {
            status = .from(
                snapshot: snapshot,
                mobileHealth: mobileHealth,
                detachOnQuit: detachServerOnQuit
            )
        } else if error != nil {
            status = .unavailable(detachOnQuit: detachServerOnQuit)
        } else {
            status = .starting(detachOnQuit: detachServerOnQuit)
        }
        guard let statusItem else {
            return
        }
        applyHumanStatus(status, to: statusItem)
    }

    private func applyHumanStatus(_ status: LooperHumanStatus, to item: NSStatusItem) {
        item.button?.image = Self.statusIconImage()
        item.button?.imagePosition = .imageOnly
        item.button?.toolTip = "\(Layout.appDisplayName): \(status.title). \(status.lifecycle). \(status.detail)"
    }

    private static func statusIconImage() -> NSImage? {
        guard let image = statusIconImage(named: Layout.statusIconResourceName)
            ?? statusIconImage(named: FallbackResource.statusIconResourceName)
        else {
            return nil
        }
        image.isTemplate = false
        image.accessibilityDescription = Layout.appDisplayName
        image.size = Layout.statusIconSize
        return image
    }

    private static func statusIconImage(named resourceName: String) -> NSImage? {
        guard let url = Bundle.main.url(forResource: resourceName, withExtension: "png") else {
            return nil
        }
        return NSImage(contentsOf: url)
    }

    private enum FallbackResource {
        static let statusIconResourceName = "notification-orb"
    }
}

private let app = NSApplication.shared
private let delegate = LooperMenuBarAppDelegate()
app.delegate = delegate
app.run()

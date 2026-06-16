import AppKit
import Carbon.HIToolbox
import Foundation
import LooperMenuBarCore
import OSLog

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
        static let handoffHotkeySubMenuTitle = "Handoff Hotkey"
        static let handoffHoldSubMenuTitle = "Handoff Hold"
        static let detailsMenuTitle = "Details"
        static let settingsMenuTitle = "Settings"
        static let acpTargetsMenuTitle = "ACP Targets"
    }

    private let client: HTTPControlPlaneClient
    private let lifecycle: LooperLifecycleCoordinator
    private let continuationPublisher = LooperContinuationActivityPublisher()
    private var statusItem: NSStatusItem?
    private var menu: NSMenu?
    private var continuationRefreshTask: Task<Void, Never>?
    private var mobileHealth: MobileHealthResponse?
    private var devinProbe: DevinAcpBridgeProbe?
    private let handoffHotkeyController = HandoffHotkeyController()
    private lazy var desktopEventStream = DesktopEventStreamCoordinator(client: client) { [weak self] in
        await self?.refreshMenu()
    }

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
        LooperHandoffHotkeyOption.migrateStoredPreference()
        continuationPublisher.focusAssist = handoffFocusAssist
        continuationPublisher.focusAssistHoldDuration = handoffHoldDuration
        installStatusItem()
        continuationPublisher.publish(LooperContinuationActivityBuilder.genericDescriptor())
        startContinuationRefreshLoop()
        installHandoffHotkey()
        desktopEventStream.start()
        Task {
            _ = await lifecycle.registerOnLaunch()
            await refreshMenu()
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        continuationRefreshTask?.cancel()
        desktopEventStream.stop()
        continuationPublisher.invalidate()
        if !detachServerOnQuit {
            _ = lifecycle.unregisterBeforeQuit()
        }
        handoffHotkeyController.stop()
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
            updateMobileHealth(try? await client.fetchMobileHealth())
            publishContinuationActivity(from: snapshot)
            replaceMenu(snapshot: snapshot, error: nil)
        } catch {
            updateMobileHealth(nil)
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
            updateMobileHealth(try? await client.fetchMobileHealth())
            publishContinuationActivity(from: snapshot)
        } catch {
            updateMobileHealth(nil)
            continuationPublisher.publishFallbackIfIdle(LooperContinuationActivityBuilder.genericDescriptor())
        }
    }

    private func publishContinuationActivity(from snapshot: DesktopSnapshotResponse) {
        continuationPublisher.publish(
            LooperContinuationActivityBuilder.descriptor(
                from: snapshot,
                handoffBaseURL: mobileHealth?.preferredReachableHandoffBaseURL
            )
        )
    }

    private func updateMobileHealth(_ health: MobileHealthResponse?) {
        mobileHealth = health
        continuationPublisher.isHandoffSupported = health?.supportsNativeHandoff == true
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

        if let snapshot {
            addSnapshotThreadSections(snapshot, to: menu)
        }

        menu.addItem(NSMenuItem.separator())
        addActionItem("Refresh", action: #selector(refreshMenuAction(_:)), keyEquivalent: "r", to: menu)
        addDetailsItem(snapshot: snapshot, error: error, to: menu)
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
        addDisabledItem("Zed: \(snapshot.zedStatusTitle)", to: menu)
        addDisabledItem(
            "ACP targets: \(LooperMenuContent.acpTargetStatusTitle(from: snapshot.acpTargets))",
            to: menu
        )
        addDisabledItem("Automations: \(coveredAutomationCount(snapshot))/\(snapshot.automations.count) covered", to: menu)
        addDisabledItem("Goals: \(runningGoalCount(snapshot))/\(snapshot.goals.count) running", to: menu)
        addDevinBridgeItems(snapshot.devinDesktop.acpBridge, to: menu)
        addAcpTargetsItem(snapshot.acpTargets, to: menu)
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

    private func addAcpTargetsItem(_ targets: [AcpTargetSummary], to menu: NSMenu) {
        guard !targets.isEmpty else {
            return
        }

        menu.addItem(NSMenuItem.separator())
        let item = NSMenuItem(title: Layout.acpTargetsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.acpTargetsMenuTitle)
        submenu.autoenablesItems = false
        addDisabledItem("Targets: \(LooperMenuContent.acpTargetStatusTitle(from: targets))", to: submenu)
        submenu.addItem(NSMenuItem.separator())

        for row in LooperMenuContent.buildAcpTargetRows(from: targets) {
            submenu.addItem(makeAcpTargetItem(row))
        }

        item.submenu = submenu
        menu.addItem(item)
    }

    private func makeAcpTargetItem(_ row: LooperAcpTargetRow) -> NSMenuItem {
        let item = NSMenuItem(title: row.title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        setSubtitle(row.subtitle, on: item)
        if !row.detail.isEmpty {
            item.toolTip = row.detail
        }
        return item
    }

    private func coveredAutomationCount(_ snapshot: DesktopSnapshotResponse) -> Int {
        snapshot.automations.filter(\.controlPlaneCovered).count
    }

    private func runningGoalCount(_ snapshot: DesktopSnapshotResponse) -> Int {
        snapshot.goals.filter(\.running).count
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
        addHandoffHotkeyItem(to: submenu)
        addHandoffHoldItem(to: submenu)
        addDetachServerItem(to: submenu)
        submenu.addItem(NSMenuItem.separator())
        addRepairHooksItem(to: submenu)
        addClearLiveHooksItem(to: submenu)
        addDevinAcpBridgeSettingsItem(to: submenu)
        addActionItem("Copy Terminal Command", action: #selector(copyTerminalCommandAction(_:)), keyEquivalent: "c", to: submenu)
        item.submenu = submenu
        menu.addItem(item)
    }

    private func addRepairHooksItem(to menu: NSMenu) {
        let item = NSMenuItem(title: "Repair Hooks", action: nil, keyEquivalent: "")
        item.submenu = hookTargetMenu(title: "Repair Hooks", action: #selector(repairHooksAction(_:)))
        menu.addItem(item)
    }

    private func addClearLiveHooksItem(to menu: NSMenu) {
        let item = NSMenuItem(title: "Clear Live Hooks", action: nil, keyEquivalent: "")
        item.submenu = hookTargetMenu(title: "Clear Live Hooks", action: #selector(clearLiveHooksAction(_:)))
        menu.addItem(item)
    }

    private func hookTargetMenu(title: String, action: Selector) -> NSMenu {
        let submenu = NSMenu(title: title)
        submenu.autoenablesItems = false
        addActionItem("All Hook Sources", action: action, keyEquivalent: "", to: submenu)
        submenu.addItem(NSMenuItem.separator())
        for target in HookRepairTarget.allCases {
            let item = NSMenuItem(title: target.displayTitle, action: action, keyEquivalent: "")
            item.target = self
            item.representedObject = target
            submenu.addItem(item)
        }
        return submenu
    }

    private func addDevinAcpBridgeSettingsItem(to menu: NSMenu) {
        let item = NSMenuItem(title: "Devin ACP Bridge", action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: "Devin ACP Bridge")
        submenu.autoenablesItems = false
        addActionItem(
            "Install or Repair Bridge",
            action: #selector(installDevinAcpBridgeAction(_:)),
            keyEquivalent: "",
            to: submenu
        )
        addActionItem("Probe Devin Agent", action: #selector(probeDevinAcpAction(_:)), keyEquivalent: "", to: submenu)
        item.submenu = submenu
        menu.addItem(item)
    }

    private func addHandoffFocusAssistItem(to menu: NSMenu) {
        let item = NSMenuItem(
            title: Layout.handoffFocusAssistMenuTitle,
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

    private func addHandoffHotkeyItem(to menu: NSMenu) {
        let item = NSMenuItem(
            title: Layout.handoffHotkeySubMenuTitle,
            action: nil,
            keyEquivalent: ""
        )
        let submenu = NSMenu(title: Layout.handoffHotkeySubMenuTitle)
        submenu.autoenablesItems = false

        for option in LooperHandoffHotkeyOption.allOptions {
            let optionItem = NSMenuItem(
                title: option.menuTitle,
                action: #selector(setHandoffHotkeyAction(_:)),
                keyEquivalent: ""
            )
            optionItem.target = self
            optionItem.representedObject = option.rawValue
            optionItem.state = handoffHotkeyOption == option ? .on : .off
            submenu.addItem(optionItem)
        }

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addHandoffHoldItem(to menu: NSMenu) {
        let item = NSMenuItem(
            title: Layout.handoffHoldSubMenuTitle,
            action: nil,
            keyEquivalent: ""
        )
        let submenu = NSMenu(title: Layout.handoffHoldSubMenuTitle)
        submenu.autoenablesItems = false

        for option in LooperHandoffHoldDuration.allOptions {
            let optionItem = NSMenuItem(
                title: option.menuTitle,
                action: #selector(setHandoffHoldDurationAction(_:)),
                keyEquivalent: ""
            )
            optionItem.target = self
            optionItem.representedObject = option.rawValue
            optionItem.state = handoffHoldDuration == option ? .on : .off
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

    private var handoffHoldDuration: LooperHandoffHoldDuration {
        get {
            LooperHandoffHoldDuration.stored()
        }
        set {
            newValue.save()
            continuationPublisher.focusAssistHoldDuration = newValue
        }
    }

    private var handoffHotkeyOption: LooperHandoffHotkeyOption {
        get {
            LooperHandoffHotkeyOption.stored()
        }
        set {
            newValue.save()
            handoffHotkeyController.update(option: newValue)
        }
    }

    private func installHandoffHotkey() {
        handoffHotkeyController.start(option: handoffHotkeyOption) { [weak self] in
            self?.handleHandoffHotkey()
        }
    }

    private func handleHandoffHotkey() {
        guard !continuationPublisher.requestFocusAssistedActivation() else {
            Task {
                await refreshContinuationActivity()
            }
            return
        }

        Task {
            await refreshContinuationActivity()
            continuationPublisher.requestFocusAssistedActivation()
        }
    }

    @objc private func refreshMenuAction(_ sender: Any?) {
        Task {
            await refreshMenu()
        }
    }

    @objc private func repairHooksAction(_ sender: Any?) {
        let target = (sender as? NSMenuItem)?.representedObject as? HookRepairTarget
        Task {
            do {
                if let target {
                    try await client.registerHooks(target: target)
                } else {
                    try await client.registerHooks()
                }
            } catch {
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu()
        }
    }

    @objc private func clearLiveHooksAction(_ sender: Any?) {
        let target = (sender as? NSMenuItem)?.representedObject as? HookRepairTarget
        Task {
            do {
                if let target {
                    try client.unregisterLiveHooks(
                        target: target,
                        timeout: LooperLifecycleDefaults.requestTimeoutSeconds
                    )
                } else {
                    try client.unregisterLiveHooks(timeout: LooperLifecycleDefaults.requestTimeoutSeconds)
                }
            } catch {
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu()
        }
    }

    @objc private func installDevinAcpBridgeAction(_ sender: Any?) {
        Task {
            do {
                _ = try await client.installDevinAcpBridge()
                devinProbe = nil
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

    @objc private func setHandoffHotkeyAction(_ sender: NSMenuItem) {
        guard let rawValue = sender.representedObject as? String,
              let option = LooperHandoffHotkeyOption(rawValue: rawValue)
        else {
            return
        }

        handoffHotkeyOption = option
        Task {
            await refreshMenu()
        }
    }

    @objc private func setHandoffHoldDurationAction(_ sender: NSMenuItem) {
        guard let rawValue = sender.representedObject as? String,
              let option = LooperHandoffHoldDuration(rawValue: rawValue)
        else {
            return
        }

        handoffHoldDuration = option
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

@MainActor
private final class HandoffHotkeyController {
    private enum Layout {
        static let loggingSubsystem = "dev.looper.app.ios"
        static let loggingCategory = "handoff-hotkey"
        static let hotkeySignature = FourCharacterCode.make("LHky")
        static let hotkeyIdentifier: UInt32 = 1
        static let handledEventCount = 1
        static let registrationOptions: OptionBits = 0
    }

    private let logger = Logger(subsystem: Layout.loggingSubsystem, category: Layout.loggingCategory)
    private let eventHandler: EventHandlerUPP = { _, _, userData in
        guard let userData else {
            return noErr
        }

        let controller = Unmanaged<HandoffHotkeyController>.fromOpaque(userData).takeUnretainedValue()
        Task { @MainActor in
            controller.invokeHandler()
        }
        return noErr
    }

    private var eventHandlerReference: EventHandlerRef?
    private var hotkeyReference: EventHotKeyRef?
    private var registeredOption: LooperHandoffHotkeyOption?
    private var onHotkey: (@MainActor () -> Void)?

    func start(option: LooperHandoffHotkeyOption, onHotkey: @escaping @MainActor () -> Void) {
        self.onHotkey = onHotkey
        update(option: option)
    }

    func update(option: LooperHandoffHotkeyOption) {
        unregisterCurrentHotkey()
        registeredOption = nil

        guard option.isEnabled else {
            logger.info("handoff hotkey disabled")
            return
        }

        guard installEventHandlerIfNeeded() else {
            return
        }

        var hotkeyReference: EventHotKeyRef?
        let hotkeyID = EventHotKeyID(signature: Layout.hotkeySignature, id: Layout.hotkeyIdentifier)
        let status = RegisterEventHotKey(
            option.carbonKeyCode,
            option.carbonModifierFlags,
            hotkeyID,
            GetApplicationEventTarget(),
            Layout.registrationOptions,
            &hotkeyReference
        )

        guard status == noErr, let hotkeyReference else {
            logger.error(
                "handoff hotkey registration failed option=\(option.menuTitle, privacy: .public) status=\(status, privacy: .public)"
            )
            return
        }

        self.hotkeyReference = hotkeyReference
        registeredOption = option
        logger.info("handoff hotkey registered option=\(option.menuTitle, privacy: .public)")
    }

    func stop() {
        unregisterCurrentHotkey()
        removeEventHandler()
        onHotkey = nil
    }

    private func installEventHandlerIfNeeded() -> Bool {
        if eventHandlerReference != nil {
            return true
        }

        var eventType = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )
        let userData = Unmanaged.passUnretained(self).toOpaque()
        let status = InstallEventHandler(
            GetApplicationEventTarget(),
            eventHandler,
            Layout.handledEventCount,
            &eventType,
            userData,
            &eventHandlerReference
        )

        guard status == noErr else {
            logger.error("handoff hotkey event handler installation failed status=\(status, privacy: .public)")
            return false
        }

        return true
    }

    private func unregisterCurrentHotkey() {
        guard let hotkeyReference else {
            return
        }

        UnregisterEventHotKey(hotkeyReference)
        self.hotkeyReference = nil
    }

    private func removeEventHandler() {
        guard let eventHandlerReference else {
            return
        }

        RemoveEventHandler(eventHandlerReference)
        self.eventHandlerReference = nil
    }

    private func invokeHandler() {
        logger.info("handoff hotkey pressed option=\(self.registeredOption?.menuTitle ?? "unknown", privacy: .public)")
        onHotkey?()
    }
}

private enum FourCharacterCode {
    private static let expectedByteCount = 4
    private static let bitsPerByte: UInt32 = 8

    static func make(_ characters: String) -> OSType {
        precondition(characters.utf8.count == expectedByteCount)
        return characters.utf8.reduce(OSType(0)) { partialResult, character in
            (partialResult << bitsPerByte) + OSType(character)
        }
    }
}

private extension LooperHandoffHotkeyOption {
    var carbonKeyCode: UInt32 {
        UInt32(kVK_ANSI_L)
    }

    var carbonModifierFlags: UInt32 {
        switch self {
        case .commandL:
            Self.carbonModifierFlags(cmdKey)
        case .commandShiftL:
            Self.carbonModifierFlags(cmdKey, shiftKey)
        case .commandOptionL:
            Self.carbonModifierFlags(cmdKey, optionKey)
        case .controlL:
            Self.carbonModifierFlags(controlKey)
        case .disabled:
            0
        }
    }

    private static func carbonModifierFlags(_ flags: Int...) -> UInt32 {
        flags.reduce(UInt32(0)) { partialResult, flag in
            partialResult | UInt32(flag)
        }
    }
}

private let app = NSApplication.shared
private let delegate = LooperMenuBarAppDelegate()
app.delegate = delegate
app.run()

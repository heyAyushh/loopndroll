import AppKit
import Carbon.HIToolbox
import Foundation
import LooperMenuBarCore
import OSLog

@MainActor
private final class LooperMenuBarAppDelegate: NSObject, NSApplicationDelegate, NSMenuDelegate {
    private enum Layout {
        static let acpHostsMenuTitle = "Assistant Hosts (ACP)"
        static let agentsDetailsMenuTitle = "Agents & Assistant Hosts"
        static let coverageDetailsMenuTitle = "Coverage"
        static let statusItemTitle = "looper"
        static let visibleThreadLimitPerSection = 5
        static let statusIconResourceName = "looper-status-icon"
        static let appDisplayName = "looper"
        static let devinAcpHostID = "devin"
        static let devinAcpHostTitle = "Devin Desktop"
        static let zedAcpHostID = "zed"
        static let zedAcpHostTitle = "Zed"
        static let threadMenuTitleCharacterLimit = 38
        static let detachServerOnQuitKey = "detachServerOnQuit"
        static let continuationRefreshInterval: Duration = .seconds(20)
        static let activationPolicy: NSApplication.ActivationPolicy = .accessory
        static let handoffFocusAssistMenuTitle = "Handoff Focus Assist"
        static let handoffHotkeySubMenuTitle = "Handoff Hotkey"
        static let handoffHoldSubMenuTitle = "Handoff Hold"
        static let detailsMenuTitle = "Details"
        static let mobileRouteMenuTitle = "Mobile Route"
        static let settingsMenuTitle = "Settings"
        static let acpTargetsMenuTitle = "Configured ACP Targets"
        static let acpHostLimitationsTitle = "Limitations"
        static let acpHostLimitationItemTitle = "Limited capability"
    }

    private let client: HTTPControlPlaneClient
    private let lifecycle: LooperLifecycleCoordinator
    private let continuationPublisher = LooperContinuationActivityPublisher()
    private lazy var menuRefreshCoordinator = MenuRefreshCoordinator(client: client)
    private var statusItem: NSStatusItem?
    private var menu: NSMenu?
    private var continuationRefreshTask: Task<Void, Never>?
    private var mobileHealth: MobileHealthResponse?
    private var devinProbe: DevinAcpBridgeProbe?
    private let handoffHotkeyController = HandoffHotkeyController()
    private lazy var desktopEventStream = DesktopEventStreamCoordinator(client: client) { [weak self] in
        await self?.refreshMenu(force: true)
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
        let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.variableLength)
        applyHumanStatus(.starting(detachOnQuit: detachServerOnQuit), to: item)
        continuationPublisher.attachHost(item.button)

        let menu = makeMenu(snapshot: nil, error: nil)
        item.menu = menu
        self.menu = menu
        statusItem = item
    }

    private func refreshMenu(force: Bool = false) async {
        let result = await menuRefreshCoordinator.refresh(force: force)
        if let snapshot = result.snapshot {
            updateMobileHealth(result.mobileHealth)
            publishContinuationActivity(from: snapshot)
            replaceMenu(
                snapshot: snapshot,
                connections: result.connections,
                acpClientHosts: result.acpClientHosts,
                error: nil
            )
        } else {
            updateMobileHealth(nil)
            continuationPublisher.publishFallbackIfIdle(LooperContinuationActivityBuilder.genericDescriptor())
            replaceMenu(snapshot: nil, connections: nil, acpClientHosts: nil, error: result.error)
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
        let result = await menuRefreshCoordinator.refresh()
        if let snapshot = result.snapshot {
            updateMobileHealth(result.mobileHealth)
            publishContinuationActivity(from: snapshot)
        } else {
            updateMobileHealth(nil)
            continuationPublisher.publishFallbackIfIdle(LooperContinuationActivityBuilder.genericDescriptor())
        }
    }

    private func publishContinuationActivity(from snapshot: DesktopSnapshotResponse) {
        continuationPublisher.publish(
            LooperContinuationActivityBuilder.descriptor(
                from: snapshot,
                handoffBaseURL: mobileHealth?.preferredReachableHandoffBaseURL(preference: mobileRoutePreference)
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

        let result = await menuRefreshCoordinator.refresh()
        if let snapshot = result.snapshot,
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

    private func replaceMenu(
        snapshot: DesktopSnapshotResponse?,
        connections: DesktopConnectionsResponse? = nil,
        acpClientHosts: AcpClientHostsResponse? = nil,
        error: Error?
    ) {
        updateStatusItem(snapshot: snapshot, error: error)
        let menu = makeMenu(
            snapshot: snapshot,
            connections: connections,
            acpClientHosts: acpClientHosts,
            error: error
        )
        statusItem?.menu = menu
        self.menu = menu
    }

    private func makeMenu(
        snapshot: DesktopSnapshotResponse?,
        connections: DesktopConnectionsResponse? = nil,
        acpClientHosts: AcpClientHostsResponse? = nil,
        error: Error?
    ) -> NSMenu {
        let menu = NSMenu()
        menu.delegate = self
        addDisabledItem(Layout.appDisplayName, to: menu)
        menu.addItem(NSMenuItem.separator())

        if let snapshot {
            addSnapshotThreadSections(snapshot, to: menu)
        }

        menu.addItem(NSMenuItem.separator())
        addActionItem("Refresh", action: #selector(refreshMenuAction(_:)), keyEquivalent: "r", to: menu)
        addDetailsItem(snapshot: snapshot, connections: connections, error: error, to: menu)
        addSettingsItem(snapshot: snapshot, acpClientHosts: acpClientHosts, to: menu)
        addActionItem("Stop Server", action: #selector(stopServerAction(_:)), keyEquivalent: "", to: menu)
        menu.addItem(NSMenuItem.separator())
        menu.addItem(NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
        return menu
    }

    private func addDetailsItem(
        snapshot: DesktopSnapshotResponse?,
        connections: DesktopConnectionsResponse? = nil,
        error: Error?,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: Layout.detailsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.detailsMenuTitle)
        submenu.autoenablesItems = false

        if let snapshot {
            addSnapshotDetails(snapshot, connections: connections, to: submenu)
        } else if error != nil {
            addUnavailableDetails(to: submenu)
        } else {
            addStartingDetails(to: submenu)
        }

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addSnapshotDetails(
        _ snapshot: DesktopSnapshotResponse,
        connections: DesktopConnectionsResponse?,
        to menu: NSMenu
    ) {
        let status = LooperHumanStatus.from(
            snapshot: snapshot,
            mobileHealth: mobileHealth,
            detachOnQuit: detachServerOnQuit
        )
        addDisabledItem("Status: \(status.title)", to: menu)
        addDisabledItem("Lifecycle: \(status.lifecycle)", to: menu)
        addDisabledItem("iPhone: \(mobileStatusTitle())", to: menu)
        addMobileRouteDetails(to: menu)
        addDisabledItem("Chats: \(snapshot.activeThreadCount) active, \(snapshot.archivedThreadCount) archived", to: menu)
        menu.addItem(NSMenuItem.separator())
        addAgentDetails(snapshot, connections: connections, to: menu)
        addCoverageDetails(snapshot, to: menu)
    }

    private func addMobileRouteDetails(to menu: NSMenu) {
        guard let mobileHealth else {
            return
        }

        addDisabledItem("Route: \(mobileHealth.routeSummaryTitle(preference: mobileRoutePreference))", to: menu)

        guard let tailscale = mobileHealth.tailscale else {
            return
        }

        let detail = tailscale.routeDetailTitle
        addDisabledItem(
            "Tailscale: \(tailscale.statusTitle)",
            subtitle: detail.isEmpty ? nil : detail,
            to: menu
        )
    }

    private func addAgentDetails(
        _ snapshot: DesktopSnapshotResponse,
        connections: DesktopConnectionsResponse?,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: Layout.agentsDetailsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.agentsDetailsMenuTitle)
        submenu.autoenablesItems = false
        addDisabledItem("Devin: \(devinStatusTitle(snapshot.devinDesktop.acpBridge))", to: submenu)
        addDisabledItem("Zed: \(snapshot.zedStatusTitle)", to: submenu)
        addDisabledItem(
            "ACP targets: \(LooperMenuContent.acpTargetStatusTitle(from: snapshot.acpTargets))",
            to: submenu
        )

        let agentRows = snapshot.agentDetailMenuRows(connections: connections)
        if agentRows.isEmpty {
            submenu.addItem(NSMenuItem.separator())
            addDisabledItem("No agent connections", to: submenu)
        } else {
            submenu.addItem(NSMenuItem.separator())
            for row in agentRows {
                addDisabledItem(row.title, subtitle: row.detail, to: submenu)
            }
        }
        addDevinBridgeItems(snapshot.devinDesktop.acpBridge, to: submenu)
        addAcpTargetsItem(snapshot.acpTargets, to: submenu)

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addCoverageDetails(_ snapshot: DesktopSnapshotResponse, to menu: NSMenu) {
        let item = NSMenuItem(title: Layout.coverageDetailsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.coverageDetailsMenuTitle)
        submenu.autoenablesItems = false
        addDisabledItem("Automations: \(coveredAutomationCount(snapshot))/\(snapshot.automations.count) covered", to: submenu)
        addDisabledItem("Goals: \(runningGoalCount(snapshot))/\(snapshot.goals.count) running", to: submenu)
        item.submenu = submenu
        menu.addItem(item)
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
            title: "Probe Default Devin Agent",
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

    private func addDisabledItem(_ title: String, subtitle: String? = nil, to menu: NSMenu) {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        if let subtitle {
            setSubtitle(subtitle, on: item)
        }
        menu.addItem(item)
    }

    @discardableResult
    private func addActionItem(
        _ title: String,
        action: Selector,
        keyEquivalent: String,
        to menu: NSMenu
    ) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: action, keyEquivalent: keyEquivalent)
        item.target = self
        menu.addItem(item)
        return item
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

    private func addSettingsItem(
        snapshot: DesktopSnapshotResponse?,
        acpClientHosts: AcpClientHostsResponse?,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: Layout.settingsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.settingsMenuTitle)
        submenu.autoenablesItems = false
        addMobileRouteSettingsItem(to: submenu)
        addAcpHostsSettingsItem(snapshot: snapshot, acpClientHosts: acpClientHosts, to: submenu)
        submenu.addItem(NSMenuItem.separator())
        addHandoffFocusAssistItem(to: submenu)
        addHandoffHotkeyItem(to: submenu)
        addHandoffHoldItem(to: submenu)
        addDetachServerItem(to: submenu)
        submenu.addItem(NSMenuItem.separator())
        addRepairHooksItem(to: submenu)
        addClearLiveHooksItem(to: submenu)
        submenu.addItem(NSMenuItem.separator())
        addActionItem("Copy Terminal Command", action: #selector(copyTerminalCommandAction(_:)), keyEquivalent: "c", to: submenu)
        item.submenu = submenu
        menu.addItem(item)
    }

    private func addMobileRouteSettingsItem(to menu: NSMenu) {
        let item = NSMenuItem(
            title: Layout.mobileRouteMenuTitle,
            action: nil,
            keyEquivalent: ""
        )
        let submenu = NSMenu(title: Layout.mobileRouteMenuTitle)
        submenu.autoenablesItems = false

        for preference in MobileRoutePreference.allOptions {
            let optionItem = NSMenuItem(
                title: preference.menuTitle,
                action: #selector(setMobileRoutePreferenceAction(_:)),
                keyEquivalent: ""
            )
            optionItem.target = self
            optionItem.representedObject = preference.rawValue
            optionItem.state = mobileRoutePreference == preference ? .on : .off
            submenu.addItem(optionItem)
        }

        submenu.addItem(NSMenuItem.separator())
        addMobileRouteStatusItems(to: submenu)

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addMobileRouteStatusItems(to menu: NSMenu) {
        guard let mobileHealth else {
            addDisabledItem("Status: Loading", to: menu)
            return
        }

        addDisabledItem("Preference: \(mobileRoutePreference.menuTitle)", to: menu)
        addDisabledItem("Current route: \(mobileHealth.routeSummaryTitle(preference: mobileRoutePreference))", to: menu)

        guard let tailscale = mobileHealth.tailscale else {
            addDisabledItem("Tailscale: Unknown", to: menu)
            return
        }

        let detail = tailscale.routeDetailTitle
        addDisabledItem(
            "Tailscale: \(tailscale.statusTitle)",
            subtitle: detail.isEmpty ? nil : detail,
            to: menu
        )
    }

    private func addAcpHostsSettingsItem(
        snapshot: DesktopSnapshotResponse?,
        acpClientHosts: AcpClientHostsResponse?,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: Layout.acpHostsMenuTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.acpHostsMenuTitle)
        submenu.autoenablesItems = false
        addDevinAcpHostSettingsItem(snapshot: snapshot, acpClientHosts: acpClientHosts, to: submenu)
        addZedAcpHostSettingsItem(snapshot: snapshot, acpClientHosts: acpClientHosts, to: submenu)
        item.submenu = submenu
        menu.addItem(item)
    }

    private func addDevinAcpHostSettingsItem(
        snapshot: DesktopSnapshotResponse?,
        acpClientHosts: AcpClientHostsResponse?,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: Layout.devinAcpHostTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.devinAcpHostTitle)
        submenu.autoenablesItems = false

        if let snapshot {
            addDisabledItem("Status: \(devinStatusTitle(snapshot.devinDesktop.acpBridge))", to: submenu)
        } else {
            addDisabledItem("Status: Loading", to: submenu)
        }

        if let devinHost = acpClientHosts?.hosts.first(where: { $0.id == Layout.devinAcpHostID }) {
            addDisabledItem(
                "Agents: \(devinHost.enabledAgentCount)/\(devinHost.agents.count) enabled",
                to: submenu
            )
            if let defaultAgent = devinHost.defaultProbeAgent {
                addDisabledItem("Default agent: \(defaultAgent.name)", subtitle: defaultAgent.id, to: submenu)
            }
            addAcpHostLimitations(devinHost, to: submenu)
        }

        submenu.addItem(NSMenuItem.separator())
        addActionItem(
            "Install or Repair Bridge",
            action: #selector(installDevinAcpBridgeAction(_:)),
            keyEquivalent: "",
            to: submenu
        )

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addZedAcpHostSettingsItem(
        snapshot: DesktopSnapshotResponse?,
        acpClientHosts: AcpClientHostsResponse?,
        to menu: NSMenu
    ) {
        let item = NSMenuItem(title: Layout.zedAcpHostTitle, action: nil, keyEquivalent: "")
        let submenu = NSMenu(title: Layout.zedAcpHostTitle)
        submenu.autoenablesItems = false

        guard let snapshot else {
            addDisabledItem("Status: Loading", to: submenu)
            item.submenu = submenu
            menu.addItem(item)
            return
        }

        let zedHost = acpClientHosts?.hosts.first { $0.id == Layout.zedAcpHostID }
        let zedTargets = snapshot.acpTargets.filter { $0.client == Layout.zedAcpHostID }
        addDisabledItem("Status: \(snapshot.zedStatusTitle)", to: submenu)
        addDisabledItem("Targets: \(LooperMenuContent.acpTargetStatusTitle(from: zedTargets))", to: submenu)
        addAcpHostLimitations(zedHost, to: submenu)

        if zedTargets.isEmpty {
            addDisabledItem("No configured External Agents", to: submenu)
        } else {
            submenu.addItem(NSMenuItem.separator())
            for row in LooperMenuContent.buildAcpTargetRows(from: zedTargets) {
                submenu.addItem(makeAcpTargetItem(row))
            }
        }

        item.submenu = submenu
        menu.addItem(item)
    }

    private func addAcpHostLimitations(_ host: AcpClientHost?, to menu: NSMenu) {
        guard let host, !host.limitations.isEmpty else {
            return
        }

        menu.addItem(NSMenuItem.separator())
        addDisabledItem(Layout.acpHostLimitationsTitle, to: menu)
        for limitation in host.limitations {
            addDisabledItem(Layout.acpHostLimitationItemTitle, subtitle: limitation, to: menu)
        }
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

    private var mobileRoutePreference: MobileRoutePreference {
        get {
            MobileRoutePreference.stored()
        }
        set {
            newValue.save()
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
            await refreshMenu(force: true)
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
                await menuRefreshCoordinator.clearCache()
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu(force: true)
        }
    }

    @objc private func clearLiveHooksAction(_ sender: Any?) {
        let target = (sender as? NSMenuItem)?.representedObject as? HookRepairTarget
        Task {
            do {
                if let target {
                    try await client.unregisterLiveHooksWithoutBlockingUI(
                        target: target,
                        timeout: LooperLifecycleDefaults.requestTimeoutSeconds
                    )
                } else {
                    try await client.unregisterLiveHooksWithoutBlockingUI(
                        timeout: LooperLifecycleDefaults.requestTimeoutSeconds
                    )
                }
            } catch {
                await menuRefreshCoordinator.clearCache()
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu(force: true)
        }
    }

    @objc private func installDevinAcpBridgeAction(_ sender: Any?) {
        Task {
            do {
                _ = try await client.installAcpClientHost(id: Layout.devinAcpHostID)
            } catch {
                await menuRefreshCoordinator.clearCache()
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu(force: true)
        }
    }

    @objc private func probeDevinAcpAction(_ sender: NSMenuItem) {
        let agentID = sender.representedObject as? String
        Task {
            do {
                let response = try await client.probeDevinAcpBridge(agentId: agentID)
                devinProbe = response.probe
            } catch {
                devinProbe = nil
                await menuRefreshCoordinator.clearCache()
                replaceMenu(snapshot: nil, error: error)
                return
            }
            await refreshMenu(force: true)
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
            await refreshMenu(force: true)
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
            await refreshMenu(force: true)
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
            await refreshMenu(force: true)
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
            await refreshMenu(force: true)
        }
    }

    @objc private func setMobileRoutePreferenceAction(_ sender: NSMenuItem) {
        guard let rawValue = sender.representedObject as? String,
              let preference = MobileRoutePreference(rawValue: rawValue)
        else {
            return
        }

        mobileRoutePreference = preference
        Task {
            await refreshMenu(force: true)
        }
    }

    @objc private func stopServerAction(_ sender: Any?) {
        lifecycle.shutdownServer()
        Task {
            await refreshMenu(force: true)
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
        item.button?.title = Layout.statusItemTitle
        item.button?.image = nil
        item.button?.imagePosition = .noImage
        item.button?.toolTip = "\(Layout.appDisplayName): \(status.title). \(status.lifecycle). \(status.detail)"
    }
}

@MainActor
private final class HandoffHotkeyController {
    private enum Layout {
        static let loggingSubsystem = "dev.looper.app.menubar"
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

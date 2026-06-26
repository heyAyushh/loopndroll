import AppKit
import Carbon.HIToolbox
import Foundation
import LooperClientCore
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
    static let notificationsMenuTitle = "Notifications"
    static let settingsMenuTitle = "Settings"
    static let diagnosticsMenuTitle = "Diagnostics"
    static let diagnosticsLaunchArgument = "--looper-diagnostics"
    static let acpTargetsMenuTitle = "Configured ACP Targets"
    static let acpHostLimitationsTitle = "Limitations"
    static let acpHostLimitationItemTitle = "Limited capability"
    static let preserveSettingsMenuTitle = "Preserve Settings"
    static let openSettingsMenuTitle = "Open Settings"
    static let openSettingsUnavailableTitle = "No editor settings files found"
    static let notificationTargetMacOS = "macos"
  }

  private let client: HTTPControlPlaneClient
  private let lifecycle: LooperLifecycleCoordinator
  private let continuationPublisher = LooperContinuationActivityPublisher()
  private let sessionClientCore: LooperClientCore
  private let sessionMiniLocalStore: MenuBarSessionMiniLocalStore?
  private lazy var menuRefreshCoordinator = MenuRefreshCoordinator(client: client)
  private lazy var sessionCommandCenter = MenuBarSessionCommandCenter(
    localStore: sessionMiniLocalStore,
    clientCore: sessionClientCore
  )
  private var statusItem: NSStatusItem?
  private var menu: NSMenu?
  private var cachedSessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?
  private var continuationRefreshTask: Task<Void, Never>?
  private var sessionMiniSyncTask: Task<Void, Never>?
  private var mobileHealth: MobileHealthResponse?
  private var mobileState: DesktopMobileStateResponse?
  private var pushDevices: DesktopPushDevicesResponse?
  private var devinProbe: DevinAcpBridgeProbe?
  private let handoffHotkeyController = HandoffHotkeyController()
  private let diagnosticsWindowController = LooperDiagnosticsWindowController()
  private lazy var desktopNotifications = LooperDesktopNotificationCenter(
    openSession: { [weak self] threadID in
      await self?.openThreadFromNotification(threadID)
    },
    replyToSession: { [weak self] notificationID, threadID, prompt in
      await self?.replyToThreadFromNotification(
        notificationID: notificationID,
        threadID: threadID,
        prompt: prompt
      )
    }
  )

  override init() {
    let endpointStore = ControlPlaneEndpointStore()
    let client = HTTPControlPlaneClient(endpointStore: endpointStore)
    let sessionClientCore = LooperClientCore()
    self.client = client
    self.sessionClientCore = sessionClientCore
    self.sessionMiniLocalStore = MenuBarSessionMiniLocalStore.liveDefault(
      clientCore: sessionClientCore
    )
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
    desktopNotifications.start()
    startSessionMiniSync()
    if diagnosticsRequestedFromLaunchArguments {
      diagnosticsWindowController.showLoading()
    }
    Task {
      _ = await lifecycle.registerOnLaunch()
      await refreshMenu()
      if diagnosticsRequestedFromLaunchArguments {
        await showDiagnosticsWindow(force: true)
      }
    }
  }

  func applicationWillTerminate(_ notification: Notification) {
    continuationRefreshTask?.cancel()
    stopSessionMiniSync()
    _ = try? sessionClientCore.stop()
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

    let cachedMinis = restoreCachedSessionMiniSnapshot()
    let menu = makeMenu(snapshot: nil, sessionMiniSnapshot: cachedMinis, error: nil)
    item.menu = menu
    self.menu = menu
    statusItem = item
  }

  private func refreshMenu(force: Bool = false) async {
    let sessionMiniSnapshot = cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot()
    if !force, let sessionMiniSnapshot {
      replaceMenu(snapshot: nil, sessionMiniSnapshot: sessionMiniSnapshot, error: nil)
    }

    let result = await menuRefreshCoordinator.refresh(force: force)
    if let snapshot = result.snapshot {
      updateMobileState(
        result.mobileState, pushDevices: result.pushDevices, health: result.mobileHealth)
      publishContinuationActivity(
        sessionMiniSnapshot: sessionMiniSnapshot ?? cachedSessionMiniSnapshot,
        snapshot: snapshot
      )
      replaceMenu(
        snapshot: snapshot,
        sessionMiniSnapshot: sessionMiniSnapshot ?? cachedSessionMiniSnapshot,
        connections: result.connections,
        acpClientHosts: result.acpClientHosts,
        error: nil
      )
    } else {
      updateMobileState(nil, pushDevices: nil, health: nil)
      continuationPublisher.publishFallbackIfIdle(
        LooperContinuationActivityBuilder.genericDescriptor())
      replaceMenu(
        snapshot: nil,
        sessionMiniSnapshot: cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot(),
        connections: nil,
        acpClientHosts: nil,
        error: result.error
      )
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
    let sessionMiniSnapshot = cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot()
    if let sessionMiniSnapshot {
      publishContinuationActivity(from: sessionMiniSnapshot)
    }

    let result = await menuRefreshCoordinator.refresh()
    if let snapshot = result.snapshot {
      updateMobileState(
        result.mobileState, pushDevices: result.pushDevices, health: result.mobileHealth)
      publishContinuationActivity(
        sessionMiniSnapshot: sessionMiniSnapshot ?? cachedSessionMiniSnapshot,
        snapshot: snapshot
      )
    } else {
      updateMobileState(nil, pushDevices: nil, health: nil)
      if sessionMiniSnapshot == nil {
        continuationPublisher.publishFallbackIfIdle(
          LooperContinuationActivityBuilder.genericDescriptor())
      }
    }
  }

  private func publishContinuationActivity(
    sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?,
    snapshot: DesktopSnapshotResponse
  ) {
    if let sessionMiniSnapshot {
      publishContinuationActivity(from: sessionMiniSnapshot)
      return
    }

    publishContinuationActivity(from: snapshot)
  }

  private func publishContinuationActivity(from snapshot: MenuBarSessionMiniLocalSnapshot) {
    continuationPublisher.publish(
      LooperContinuationActivityBuilder.descriptor(
        from: snapshot,
        handoffBaseURL: mobileHealth?.preferredReachableHandoffBaseURL(
          preference: mobileRoutePreference)
      )
    )
  }

  private func publishContinuationActivity(from snapshot: DesktopSnapshotResponse) {
    continuationPublisher.publish(
      LooperContinuationActivityBuilder.descriptor(
        from: snapshot,
        handoffBaseURL: mobileHealth?.preferredReachableHandoffBaseURL(
          preference: mobileRoutePreference)
      )
    )
  }

  private func updateMobileHealth(_ health: MobileHealthResponse?) {
    mobileHealth = health
    continuationPublisher.isHandoffSupported = health?.supportsNativeHandoff == true
  }

  private func updateMobileState(
    _ state: DesktopMobileStateResponse?,
    pushDevices: DesktopPushDevicesResponse?,
    health: MobileHealthResponse?
  ) {
    mobileState = state
    self.pushDevices = pushDevices
    updateMobileHealth(health)
  }

  private func openContinuationActivity(_ activity: NSUserActivity) async {
    guard let target = await continuationOpenTarget(for: activity) else {
      return
    }

    _ = openThread(target)
  }

  private func continuationOpenTarget(for activity: NSUserActivity) async -> LooperThreadOpenTarget?
  {
    guard let threadID = LooperContinuationActivity.sessionID(from: activity) else {
      return nil
    }

    let sessionMiniSnapshot = cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot()
    if let target = openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot) {
      return target
    }

    let result = await menuRefreshCoordinator.refresh()
    if let snapshot = result.snapshot {
      return openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot, snapshot: snapshot)
    }

    return openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot, snapshot: nil)
  }

  private func shouldDeliverMacOSNotification(state: DesktopMobileStateResponse?) -> Bool {
    (state?.defaultNotificationTargetIDs ?? [Layout.notificationTargetMacOS])
      .contains(Layout.notificationTargetMacOS)
  }

  private func shouldDeliverMacOSNotification(for session: MenuBarSessionMini) -> Bool {
    guard let notificationStatus = session.notificationStatus else {
      return shouldDeliverMacOSNotification(state: mobileState)
    }
    guard notificationStatus.enabled else {
      return false
    }
    if notificationStatus.usesDefault {
      return shouldDeliverMacOSNotification(state: mobileState)
    }
    return notificationStatus.targetIds.contains(Layout.notificationTargetMacOS)
  }

  private func openThreadFromNotification(_ threadID: String) async {
    let sessionMiniSnapshot = cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot()
    if let target = openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot) {
      _ = openThread(target)
      return
    }

    let result = await menuRefreshCoordinator.refresh(force: true)
    if let snapshot = result.snapshot {
      updateMobileState(
        result.mobileState, pushDevices: result.pushDevices, health: result.mobileHealth)
      _ = openThread(
        openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot, snapshot: snapshot)
      )
      return
    }

    _ = openThread(openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot, snapshot: nil))
  }

  private func replyToThreadFromNotification(
    notificationID: String,
    threadID: String,
    prompt: String
  ) async {
    do {
      try await configureSessionClientCoreRuntimeIfNeeded()
      _ = try await sessionCommandCenter.submitNotificationReply(
        notificationID: notificationID,
        threadID: threadID,
        prompt: prompt,
        assistantSurface: nil,
        clientMutationID: "notification-reply:\(notificationID)"
      )
      replaceMenu(
        snapshot: nil,
        sessionMiniSnapshot: cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot(),
        error: nil
      )
    } catch {
      replaceMenu(
        snapshot: nil,
        sessionMiniSnapshot: cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot(),
        error: error
      )
    }
  }

  private func openTarget(
    for threadID: String,
    sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?,
    snapshot: DesktopSnapshotResponse?
  ) -> LooperThreadOpenTarget {
    if let target = openTarget(for: threadID, sessionMiniSnapshot: sessionMiniSnapshot) {
      return target
    }

    if let thread = snapshot?.threads.first(where: { $0.threadId == threadID }) {
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

  private func openTarget(
    for threadID: String,
    sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?
  ) -> LooperThreadOpenTarget? {
    guard let session = sessionMiniSnapshot?.sessions.first(where: { $0.sessionID == threadID }) else {
      return nil
    }

    return LooperThreadOpenTarget(
      threadId: session.sessionID,
      transcriptPath: nil,
      workingDirectory: session.projectPath
    )
  }

  private func replaceMenu(
    snapshot: DesktopSnapshotResponse?,
    sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot? = nil,
    connections: DesktopConnectionsResponse? = nil,
    acpClientHosts: AcpClientHostsResponse? = nil,
    error: Error?
  ) {
    let effectiveSessionMiniSnapshot = sessionMiniSnapshot ?? cachedSessionMiniSnapshot
    updateStatusItem(snapshot: snapshot, error: error)
    let menu = makeMenu(
      snapshot: snapshot,
      sessionMiniSnapshot: effectiveSessionMiniSnapshot,
      connections: connections,
      acpClientHosts: acpClientHosts,
      error: error
    )
    statusItem?.menu = menu
    self.menu = menu
  }

  private func makeMenu(
    snapshot: DesktopSnapshotResponse?,
    sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot? = nil,
    connections: DesktopConnectionsResponse? = nil,
    acpClientHosts: AcpClientHostsResponse? = nil,
    error: Error?
  ) -> NSMenu {
    let menu = NSMenu()
    menu.delegate = self
    addDisabledItem(Layout.appDisplayName, to: menu)
    menu.addItem(NSMenuItem.separator())

    if let sessionMiniSnapshot {
      addSessionMiniThreadSections(sessionMiniSnapshot, to: menu)
    } else if let snapshot {
      addSnapshotThreadSections(snapshot, to: menu)
    }

    menu.addItem(NSMenuItem.separator())
    addActionItem("Refresh", action: #selector(refreshMenuAction(_:)), keyEquivalent: "r", to: menu)
    addActionItem(
      Layout.diagnosticsMenuTitle, action: #selector(showDiagnosticsAction(_:)), keyEquivalent: "d",
      to: menu)
    addDetailsItem(
      snapshot: snapshot,
      sessionMiniSnapshot: sessionMiniSnapshot,
      connections: connections,
      error: error,
      to: menu
    )
    addSettingsItem(snapshot: snapshot, acpClientHosts: acpClientHosts, to: menu)
    addActionItem(
      "Stop Server", action: #selector(stopServerAction(_:)), keyEquivalent: "", to: menu)
    menu.addItem(NSMenuItem.separator())
    menu.addItem(
      NSMenuItem(title: "Quit", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q"))
    return menu
  }

  private func addDetailsItem(
    snapshot: DesktopSnapshotResponse?,
    sessionMiniSnapshot: MenuBarSessionMiniLocalSnapshot?,
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
    } else if let sessionMiniSnapshot {
      addSessionMiniDetails(sessionMiniSnapshot, to: submenu)
    } else {
      addStartingDetails(to: submenu)
    }

    item.submenu = submenu
    menu.addItem(item)
  }

  private func addSessionMiniDetails(
    _ snapshot: MenuBarSessionMiniLocalSnapshot,
    to menu: NSMenu
  ) {
    let activeCount = snapshot.sessions.filter { !$0.isArchived }.count
    let archivedCount = snapshot.sessions.count - activeCount
    addDisabledItem("Status: Realtime", to: menu)
    addDisabledItem("State: SessionMini seq \(snapshot.latestSeq)", to: menu)
    addDisabledItem("iPhone: \(mobileStatusTitle())", to: menu)
    addMobileRouteDetails(to: menu)
    addDisabledItem(
      "Chats: \(activeCount) active, \(archivedCount) archived",
      to: menu
    )

    let pendingCount = snapshot.pendingCommands.count
    guard pendingCount > 0 else {
      return
    }

    menu.addItem(NSMenuItem.separator())
    addDisabledItem("Pending commands: \(pendingCount)", to: menu)
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
    addDisabledItem(
      "Chats: \(snapshot.activeThreadCount) active, \(snapshot.archivedThreadCount) archived",
      to: menu)
    menu.addItem(NSMenuItem.separator())
    addAgentDetails(snapshot, connections: connections, to: menu)
    addCoverageDetails(snapshot, to: menu)
  }

  private func addMobileRouteDetails(to menu: NSMenu) {
    guard let mobileHealth else {
      return
    }

    addDisabledItem(
      "Route: \(mobileHealth.routeSummaryTitle(preference: mobileRoutePreference))", to: menu)

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
    addDisabledItem(
      "Automations: \(coveredAutomationCount(snapshot))/\(snapshot.automations.count) covered",
      to: submenu)
    addDisabledItem(
      "Goals: \(runningGoalCount(snapshot))/\(snapshot.goals.count) running", to: submenu)
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

  private func addSessionMiniThreadSections(
    _ snapshot: MenuBarSessionMiniLocalSnapshot,
    to menu: NSMenu
  ) {
    let sections = LooperMenuContent.buildThreadSections(from: snapshot.sessions)
    guard !sections.isEmpty else {
      return
    }

    addThreadSections(sections, to: menu)
  }

  private func restoreCachedSessionMiniSnapshot() -> MenuBarSessionMiniLocalSnapshot? {
    guard let snapshot = try? sessionMiniLocalStore?.cachedSnapshot() else {
      return nil
    }
    cachedSessionMiniSnapshot = snapshot
    return snapshot
  }

  @discardableResult
  private func restoreSessionMiniMenuIfAvailable() -> Bool {
    guard let snapshot = cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot() else {
      return false
    }

    replaceMenu(snapshot: nil, sessionMiniSnapshot: snapshot, connections: nil, acpClientHosts: nil, error: nil)
    return true
  }

  private func startSessionMiniSync() {
    guard sessionMiniSyncTask == nil,
      let sessionMiniLocalStore
    else {
      return
    }

    let clientCore = sessionClientCore
    sessionMiniSyncTask = Task { [weak self, sessionMiniLocalStore, clientCore] in
      guard let self else {
        return
      }
      do {
        try await self.configureSessionClientCoreRuntimeIfNeeded()
      } catch {
        os_log(.debug, log: .default, "session mini runtime failed: %{public}@", error.localizedDescription)
        return
      }
      await sessionMiniLocalStore.runClientCoreStateMiniSync(
        using: clientCore,
        onSnapshot: { [weak self] snapshot in
          self?.applySessionMiniSnapshot(snapshot)
        },
        onDebugMessage: { message in
          os_log(.debug, log: .default, "%{public}@", message)
        }
      )
    }
  }

  private func configureSessionClientCoreRuntimeIfNeeded() async throws {
    if (try? sessionClientCore.snapshot().endpointUrl.isEmpty) == false {
      return
    }
    let health = try await client.fetchMobileHealth()
    let endpoints = health.preferredRealtimeBaseURLs.map {
      ClientEndpoint(url: $0.absoluteString, lastGood: false)
    }
    guard !endpoints.isEmpty else {
      throw MenuBarSessionRuntimeError.noRealtimeEndpoint
    }
    _ = try sessionClientCore.configureSessionRuntime(
      endpoints: endpoints,
      bearerToken: "",
      mobileSessionHeader: ""
    )
  }

  private func stopSessionMiniSync() {
    sessionMiniSyncTask?.cancel()
    sessionMiniSyncTask = nil
  }

  private func applySessionMiniSnapshot(_ snapshot: MenuBarSessionMiniLocalSnapshot) {
    let previousSnapshot = cachedSessionMiniSnapshot
    cachedSessionMiniSnapshot = snapshot
    replaceMenu(snapshot: nil, sessionMiniSnapshot: snapshot, connections: nil, acpClientHosts: nil, error: nil)
    Task { @MainActor [weak self] in
      await self?.deliverSessionMiniStopNotifications(
        previousSnapshot: previousSnapshot,
        nextSnapshot: snapshot
      )
    }
  }

  private func deliverSessionMiniStopNotifications(
    previousSnapshot: MenuBarSessionMiniLocalSnapshot?,
    nextSnapshot: MenuBarSessionMiniLocalSnapshot
  ) async {
    var previousByID: [String: MenuBarSessionMini] = [:]
    for session in previousSnapshot?.sessions ?? [] {
      previousByID[session.sessionID] = session
    }
    for session in nextSnapshot.sessions where shouldNotifyForReplyableTransition(
      session,
      previous: previousByID[session.sessionID]
    ) {
      guard shouldDeliverMacOSNotification(for: session) else {
        continue
      }
      _ = await desktopNotifications.deliverSessionStop(
        notificationID: sessionStopNotificationID(for: session),
        threadID: session.sessionID,
        title: session.title,
        body: session.assistantPreview
      )
    }
  }

  private func shouldNotifyForReplyableTransition(
    _ session: MenuBarSessionMini,
    previous: MenuBarSessionMini?
  ) -> Bool {
    session.replyable && !session.isArchived && previous?.replyable != true
  }

  private func sessionStopNotificationID(for session: MenuBarSessionMini) -> String {
    [
      "looper-macos-stop",
      session.sessionID,
      session.revision,
    ]
    .joined(separator: "-")
  }

  private func addUnavailableDetails(to menu: NSMenu) {
    addDisabledItem("Status: Unavailable", to: menu)
    addDisabledItem("Rust service unavailable", to: menu)
    addDisabledItem(
      "Lifecycle: \(LooperHumanStatus.unavailable(detachOnQuit: detachServerOnQuit).lifecycle)",
      to: menu)
  }

  private func addStartingDetails(to menu: NSMenu) {
    addDisabledItem("Status: Starting", to: menu)
    addDisabledItem("Starting local service...", to: menu)
    addDisabledItem(
      "Lifecycle: \(LooperHumanStatus.starting(detachOnQuit: detachServerOnQuit).lifecycle)",
      to: menu)
  }

  private func mobileStatusTitle() -> String {
    guard let mobileHealth else {
      return "Unknown"
    }
    return mobileHealth.ok && mobileHealth.requiresAuthentication ? "Ready" : "Needs attention"
  }

  private func devinStatusTitle(_ bridge: DevinAcpBridgeStatus) -> String {
    if let devinProbe {
      return devinProbe.ready
        ? "\(devinProbe.name ?? devinProbe.agentId ?? "Agent") ready" : "Probe blocked"
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
    addDisabledItem(
      "Targets: \(LooperMenuContent.acpTargetStatusTitle(from: targets))", to: submenu)
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

    let prefix =
      characters
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

  private func menuSymbolImage(
    named systemImageName: String,
    accessibilityDescription: String
  ) -> NSImage? {
    let image = NSImage(
      systemSymbolName: systemImageName,
      accessibilityDescription: accessibilityDescription
    )
    image?.isTemplate = true
    return image
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
    addPreserveSettingsItem(to: submenu)
    addOpenSettingsItem(snapshot: snapshot, to: submenu)
    submenu.addItem(NSMenuItem.separator())
    addMobileRouteSettingsItem(to: submenu)
    addNotificationTargetsSettingsItem(to: submenu)
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
    addActionItem(
      "Copy Terminal Command", action: #selector(copyTerminalCommandAction(_:)), keyEquivalent: "c",
      to: submenu)
    item.submenu = submenu
    menu.addItem(item)
  }

  private func addPreserveSettingsItem(to menu: NSMenu) {
    let item = NSMenuItem(title: Layout.preserveSettingsMenuTitle, action: nil, keyEquivalent: "")
    let submenu = NSMenu(title: Layout.preserveSettingsMenuTitle)
    submenu.autoenablesItems = false

    let selectedCategories = settingsPreservationCategories
    for category in SettingsPreservationCategory.allOptions {
      let optionItem = NSMenuItem(
        title: category.menuTitle,
        action: #selector(toggleSettingsPreservationCategoryAction(_:)),
        keyEquivalent: ""
      )
      optionItem.target = self
      optionItem.representedObject = category.rawValue
      optionItem.state = selectedCategories.contains(category) ? .on : .off
      setSubtitle(category.detail, on: optionItem)
      submenu.addItem(optionItem)
    }

    submenu.addItem(NSMenuItem.separator())
    let resetItem = addActionItem(
      "Use Recommended Preserve Set",
      action: #selector(resetSettingsPreservationAction(_:)),
      keyEquivalent: "",
      to: submenu
    )
    setSubtitle("Preserves durable non-secret preferences by default", on: resetItem)

    item.submenu = submenu
    menu.addItem(item)
  }

  private func addOpenSettingsItem(snapshot: DesktopSnapshotResponse?, to menu: NSMenu) {
    let item = NSMenuItem(title: Layout.openSettingsMenuTitle, action: nil, keyEquivalent: "")
    let submenu = NSMenu(title: Layout.openSettingsMenuTitle)
    submenu.autoenablesItems = false

    addActionItem(
      "Diagnostics",
      action: #selector(showDiagnosticsAction(_:)),
      keyEquivalent: "",
      to: submenu
    )
    submenu.addItem(NSMenuItem.separator())

    for target in MacSystemSettingsTarget.allOptions {
      guard let url = target.url else {
        continue
      }
      let targetItem = addActionItem(
        target.menuTitle,
        action: #selector(openURLAction(_:)),
        keyEquivalent: "",
        to: submenu
      )
      targetItem.representedObject = url
      setSubtitle(target.detail, on: targetItem)
    }

    submenu.addItem(NSMenuItem.separator())
    addExternalSettingsFileItems(snapshot: snapshot, to: submenu)

    item.submenu = submenu
    menu.addItem(item)
  }

  private func addExternalSettingsFileItems(snapshot: DesktopSnapshotResponse?, to menu: NSMenu) {
    var addedFileTarget = false
    if let zedSettingsURL = localSettingsFileURL(
      path: snapshot?.zed.settingsPath,
      exists: snapshot?.zed.settingsExists == true
    ) {
      addOpenFileItem("Zed Settings", url: zedSettingsURL, to: menu)
      addedFileTarget = true
    }

    for installation in snapshot?.devinDesktop.installations ?? [] {
      guard
        let settingsURL = localSettingsFileURL(
          path: installation.settingsPath,
          exists: installation.settingsExists
        )
      else {
        continue
      }
      addOpenFileItem("\(installation.label) Settings", url: settingsURL, to: menu)
      addedFileTarget = true
    }

    if !addedFileTarget {
      addDisabledItem(Layout.openSettingsUnavailableTitle, to: menu)
    }
  }

  private func addOpenFileItem(_ title: String, url: URL, to menu: NSMenu) {
    let item = addActionItem(
      title, action: #selector(openURLAction(_:)), keyEquivalent: "", to: menu)
    item.representedObject = url
    setSubtitle(url.path, on: item)
  }

  private func localSettingsFileURL(path: String?, exists: Bool) -> URL? {
    guard exists,
      let trimmedPath = path?.trimmingCharacters(in: .whitespacesAndNewlines),
      !trimmedPath.isEmpty
    else {
      return nil
    }

    return URL(fileURLWithPath: trimmedPath, isDirectory: false).standardizedFileURL
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

    addDisabledItem(
      "Current route: \(mobileHealth.routeSummaryTitle(preference: mobileRoutePreference))",
      to: menu)

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

  private func addNotificationTargetsSettingsItem(to menu: NSMenu) {
    let item = NSMenuItem(
      title: Layout.notificationsMenuTitle,
      action: nil,
      keyEquivalent: ""
    )
    let submenu = NSMenu(title: Layout.notificationsMenuTitle)
    submenu.autoenablesItems = false

    let selectedTargetIDs = Set(mobileState?.defaultNotificationTargetIDs ?? ["macos"])
    let options = NotificationTargetOptions.build(
      mobileState: mobileState, pushDevices: pushDevices)
    for option in options {
      let optionItem = NSMenuItem(
        title: option.title,
        action: #selector(toggleNotificationTargetAction(_:)),
        keyEquivalent: ""
      )
      optionItem.target = self
      optionItem.representedObject = option.id
      optionItem.state = selectedTargetIDs.contains(option.id) ? .on : .off
      optionItem.isEnabled = option.available
      optionItem.image = menuSymbolImage(
        named: option.systemImageName,
        accessibilityDescription: option.title
      )
      if let detail = option.detail {
        setSubtitle(detail, on: optionItem)
      }
      submenu.addItem(optionItem)
    }

    if options.count == 1 {
      submenu.addItem(NSMenuItem.separator())
      addDisabledItem("Add iPhone or Telegram to show more targets", to: submenu)
    }

    item.submenu = submenu
    menu.addItem(item)
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
        addDisabledItem(
          "Default agent: \(defaultAgent.name)", subtitle: defaultAgent.id, to: submenu)
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
    addDisabledItem(
      "Targets: \(LooperMenuContent.acpTargetStatusTitle(from: zedTargets))", to: submenu)
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
    item.submenu = hookTargetMenu(
      title: "Clear Live Hooks", action: #selector(clearLiveHooksAction(_:)))
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

  private var settingsPreservationCategories: Set<SettingsPreservationCategory> {
    get {
      SettingsPreservationCategory.stored()
    }
    set {
      SettingsPreservationCategory.save(newValue)
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

  private var diagnosticsRequestedFromLaunchArguments: Bool {
    ProcessInfo.processInfo.arguments.contains(Layout.diagnosticsLaunchArgument)
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

  @objc private func showDiagnosticsAction(_ sender: Any?) {
    Task {
      await showDiagnosticsWindow(force: true)
    }
  }

  private func showDiagnosticsWindow(force: Bool) async {
    diagnosticsWindowController.showLoading()
    let result = await menuRefreshCoordinator.refresh(force: force)
    if let snapshot = result.snapshot {
      updateMobileState(
        result.mobileState, pushDevices: result.pushDevices, health: result.mobileHealth)
      publishContinuationActivity(
        sessionMiniSnapshot: cachedSessionMiniSnapshot ?? restoreCachedSessionMiniSnapshot(),
        snapshot: snapshot
      )
      replaceMenu(
        snapshot: snapshot,
        connections: result.connections,
        acpClientHosts: result.acpClientHosts,
        error: nil
      )
    } else {
      updateMobileState(nil, pushDevices: nil, health: nil)
      replaceMenu(snapshot: nil, connections: nil, acpClientHosts: nil, error: result.error)
    }
    diagnosticsWindowController.show(
      report: LooperDiagnosticsContent.report(
        from: result,
        mobileRoutePreference: mobileRoutePreference
      ))
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
    copyToPasteboard("looper")
  }

  @objc private func toggleDetachServerOnQuitAction(_ sender: Any?) {
    detachServerOnQuit.toggle()
    Task {
      await refreshMenu(force: true)
    }
  }

  @objc private func toggleNotificationTargetAction(_ sender: NSMenuItem) {
    guard let targetID = sender.representedObject as? String else {
      return
    }
    var selectedTargetIDs = mobileState?.defaultNotificationTargetIDs ?? ["macos"]
    if selectedTargetIDs.contains(targetID) {
      selectedTargetIDs.removeAll { $0 == targetID }
    } else {
      selectedTargetIDs.append(targetID)
    }
    if selectedTargetIDs.isEmpty {
      selectedTargetIDs = ["macos"]
    }

    Task {
      do {
        mobileState = try await client.setDefaultNotificationTargets(selectedTargetIDs)
        await menuRefreshCoordinator.clearCache()
      } catch {
        await menuRefreshCoordinator.clearCache()
        replaceMenu(snapshot: nil, error: error)
        return
      }
      await refreshMenu(force: true)
    }
  }

  @objc private func toggleSettingsPreservationCategoryAction(_ sender: NSMenuItem) {
    guard let rawValue = sender.representedObject as? String,
      let category = SettingsPreservationCategory(rawValue: rawValue)
    else {
      return
    }

    var selectedCategories = settingsPreservationCategories
    if selectedCategories.contains(category) {
      selectedCategories.remove(category)
    } else {
      selectedCategories.insert(category)
    }
    settingsPreservationCategories = selectedCategories

    Task {
      await refreshMenu(force: true)
    }
  }

  @objc private func resetSettingsPreservationAction(_ sender: Any?) {
    settingsPreservationCategories = SettingsPreservationCategory.defaultCategories
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

  @objc private func openURLAction(_ sender: NSMenuItem) {
    guard let url = sender.representedObject as? URL else {
      return
    }

    if !NSWorkspace.shared.open(url) {
      copyToPasteboard(url.absoluteString)
    }
  }

  @discardableResult
  private func openThread(_ target: LooperThreadOpenTarget) -> Bool {
    let workspace = NSWorkspace.shared
    if let codexURL = target.codexURL,
      workspace.open(codexURL)
    {
      return true
    }

    guard let fallbackURL = target.firstLocalFallbackURL else {
      return false
    }
    return workspace.open(fallbackURL)
  }

  private func copyToPasteboard(_ value: String) {
    let pasteboard = NSPasteboard.general
    pasteboard.clearContents()
    pasteboard.setString(value, forType: .string)
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
    item.button?.toolTip =
      "\(Layout.appDisplayName): \(status.title). \(status.lifecycle). \(status.detail)"
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
      logger.error(
        "handoff hotkey event handler installation failed status=\(status, privacy: .public)")
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
    logger.info(
      "handoff hotkey pressed option=\(self.registeredOption?.menuTitle ?? "unknown", privacy: .public)"
    )
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

extension LooperHandoffHotkeyOption {
  fileprivate var carbonKeyCode: UInt32 {
    UInt32(kVK_ANSI_L)
  }

  fileprivate var carbonModifierFlags: UInt32 {
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

private enum MenuBarSessionRuntimeError: LocalizedError {
  case noRealtimeEndpoint

  var errorDescription: String? {
    switch self {
    case .noRealtimeEndpoint:
      "No realtime endpoint available"
    }
  }
}

private let app = NSApplication.shared
private let delegate = LooperMenuBarAppDelegate()
private let diagnosticsLaunchRequested =
  ProcessInfo.processInfo.arguments.contains("--looper-diagnostics")
app.delegate = delegate
if diagnosticsLaunchRequested {
  NSApp.setActivationPolicy(.regular)
  app.finishLaunching()
}
app.run()

import Foundation
import LooperClientCore

public enum HookRepairTarget: String, CaseIterable, Equatable, Sendable {
  case codex
  case devin
  case grok = "grok-build"
  case claude = "claude-code"

  public var pathComponent: String {
    switch self {
    case .codex:
      "codex"
    case .devin:
      "devin"
    case .grok:
      "grok"
    case .claude:
      "claude"
    }
  }

  public var displayTitle: String {
    switch self {
    case .codex:
      "Codex"
    case .devin:
      "Devin"
    case .grok:
      "Grok Build"
    case .claude:
      "Claude Code"
    }
  }
}

public enum LooperLifecycleDefaults {
  public static let requestTimeoutSeconds: TimeInterval = 2
  public static let desktopSnapshotRequestTimeoutSeconds: TimeInterval = 6
  public static let quitCleanupTimeoutSeconds: TimeInterval = 2
}

public enum ControlPlaneEndpoint: Equatable {
  case acpClientHosts
  case acpClientHost(String)
  case acpClientHostProbe(String)
  case acpClientHostInstall(String)
  case desktopConnections
  case registerHooks
  case registerTargetHooks(HookRepairTarget)
  case unregisterHooks
  case unregisterTargetHooks(HookRepairTarget)
  case unregisterLiveHooks
  case unregisterLiveTargetHooks(HookRepairTarget)
  case shutdown
  case desktopMobileState
  case desktopPushDevices
  case mobileHealth
  case controlPlaneStatus
  case desktopSnapshot
  case devinAcpBridgeProbe
  case devinAcpBridgeInstall

  public var path: String {
    switch self {
    case .acpClientHosts:
      "/desktop/acp-client-hosts"
    case .acpClientHost(let clientHostID):
      "/desktop/acp-client-hosts/\(clientHostID)"
    case .acpClientHostProbe(let clientHostID):
      "/desktop/acp-client-hosts/\(clientHostID)/probe"
    case .acpClientHostInstall(let clientHostID):
      "/desktop/acp-client-hosts/\(clientHostID)/install"
    case .desktopConnections:
      "/desktop/connections"
    case .registerHooks:
      "/hooks/register"
    case .registerTargetHooks(let target):
      "/hooks/\(target.pathComponent)/register"
    case .unregisterHooks:
      "/hooks/unregister"
    case .unregisterTargetHooks(let target):
      "/hooks/\(target.pathComponent)/unregister"
    case .unregisterLiveHooks:
      "/hooks/unregister-live"
    case .unregisterLiveTargetHooks(let target):
      "/hooks/\(target.pathComponent)/unregister-live"
    case .shutdown:
      "/desktop/shutdown"
    case .desktopMobileState:
      "/desktop/mobile-state"
    case .desktopPushDevices:
      "/desktop/push/devices"
    case .mobileHealth:
      "/api/mobile/health"
    case .controlPlaneStatus:
      "/status/control-plane"
    case .desktopSnapshot:
      "/desktop/snapshot"
    case .devinAcpBridgeProbe:
      "/desktop/devin/acp-bridge/probe"
    case .devinAcpBridgeInstall:
      "/desktop/devin/acp-bridge/install"
    }
  }

  public var method: String {
    switch self {
    case .acpClientHostProbe, .acpClientHostInstall, .registerHooks, .registerTargetHooks,
      .unregisterHooks, .unregisterTargetHooks, .unregisterLiveHooks, .unregisterLiveTargetHooks,
      .shutdown, .devinAcpBridgeProbe,
      .devinAcpBridgeInstall:
      "POST"
    case .acpClientHosts, .acpClientHost, .desktopConnections, .desktopMobileState,
      .desktopPushDevices, .controlPlaneStatus, .desktopSnapshot, .mobileHealth:
      "GET"
    }
  }

  public var queryItems: [URLQueryItem] {
    switch self {
    case .desktopSnapshot:
      [URLQueryItem(name: "profile", value: "menu")]
    case .acpClientHosts, .acpClientHost, .acpClientHostProbe, .acpClientHostInstall,
      .desktopConnections,
      .registerHooks, .registerTargetHooks, .unregisterHooks, .unregisterTargetHooks,
      .unregisterLiveHooks, .unregisterLiveTargetHooks, .shutdown,
      .desktopMobileState, .desktopPushDevices, .mobileHealth,
      .controlPlaneStatus,
      .devinAcpBridgeProbe,
      .devinAcpBridgeInstall:
      []
    }
  }

  public var timeoutInterval: TimeInterval {
    switch self {
    case .desktopSnapshot:
      LooperLifecycleDefaults.desktopSnapshotRequestTimeoutSeconds
    case .acpClientHosts, .acpClientHost, .acpClientHostProbe, .acpClientHostInstall,
      .desktopConnections,
      .registerHooks, .registerTargetHooks, .unregisterHooks, .unregisterTargetHooks,
      .unregisterLiveHooks, .unregisterLiveTargetHooks, .shutdown,
      .desktopMobileState, .desktopPushDevices, .mobileHealth,
      .controlPlaneStatus, .devinAcpBridgeProbe,
      .devinAcpBridgeInstall:
      LooperLifecycleDefaults.requestTimeoutSeconds
    }
  }
}

public enum ControlPlaneClientError: Error, Equatable {
  case invalidResponse
  case requestFailed(statusCode: Int)
  case timeout
}

public protocol ControlPlaneClient: Sendable {
  func registerHooks() async throws
  func unregisterLiveHooks(timeout: TimeInterval) throws
  func shutdownServer(timeout: TimeInterval) throws
  func fetchControlPlaneStatus() async throws -> ControlPlaneStatusResponse
  func fetchDesktopSnapshot() async throws -> DesktopSnapshotResponse
  func fetchDesktopConnections() async throws -> DesktopConnectionsResponse
  func fetchAcpClientHosts() async throws -> AcpClientHostsResponse
  func fetchDesktopMobileState() async throws -> DesktopMobileStateResponse
  func fetchDesktopPushDevices() async throws -> DesktopPushDevicesResponse
  func fetchMobileHealth() async throws -> MobileHealthResponse
  func probeDevinAcpBridge(agentId: String?) async throws -> DevinAcpBridgeProbeResponse
}

public final class ControlPlaneEndpointStore: @unchecked Sendable {
  public static let defaultBaseURL = URL(string: "http://127.0.0.1:8765")!

  private let lock = NSLock()
  private var storedBaseURL: URL

  public init(baseURL: URL = ControlPlaneEndpointStore.defaultBaseURL) {
    self.storedBaseURL = baseURL
  }

  public var baseURL: URL {
    get {
      lock.withLock {
        storedBaseURL
      }
    }
    set {
      lock.withLock {
        storedBaseURL = newValue
      }
    }
  }
}

public final class HTTPControlPlaneClient: ControlPlaneClient, @unchecked Sendable {
  private let baseURLProvider: @Sendable () -> URL
  private let session: URLSession

  public init(
    baseURL: URL = ControlPlaneEndpointStore.defaultBaseURL,
    session: URLSession = .shared
  ) {
    self.baseURLProvider = { baseURL }
    self.session = session
  }

  public init(
    endpointStore: ControlPlaneEndpointStore,
    session: URLSession = .shared
  ) {
    self.baseURLProvider = { endpointStore.baseURL }
    self.session = session
  }

  public func registerHooks() async throws {
    let (_, response) = try await session.data(for: request(for: .registerHooks))
    try validate(response)
  }

  public func registerHooks(target: HookRepairTarget) async throws {
    let (_, response) = try await session.data(for: request(for: .registerTargetHooks(target)))
    try validate(response)
  }

  public func fetchControlPlaneStatus() async throws -> ControlPlaneStatusResponse {
    try await fetchJSON(ControlPlaneStatusResponse.self, from: .controlPlaneStatus)
  }

  public func fetchDesktopSnapshot() async throws -> DesktopSnapshotResponse {
    try await fetchJSON(DesktopSnapshotResponse.self, from: .desktopSnapshot)
  }

  public func fetchDesktopConnections() async throws -> DesktopConnectionsResponse {
    try await fetchJSON(DesktopConnectionsResponse.self, from: .desktopConnections)
  }

  public func fetchDesktopMobileState() async throws -> DesktopMobileStateResponse {
    try await fetchJSON(DesktopMobileStateResponse.self, from: .desktopMobileState)
  }

  public func fetchDesktopPushDevices() async throws -> DesktopPushDevicesResponse {
    try await fetchJSON(DesktopPushDevicesResponse.self, from: .desktopPushDevices)
  }

  public func fetchMobileHealth() async throws -> MobileHealthResponse {
    try await fetchJSON(MobileHealthResponse.self, from: .mobileHealth)
  }

  public func fetchAcpClientHosts() async throws -> AcpClientHostsResponse {
    try await fetchJSON(AcpClientHostsResponse.self, from: .acpClientHosts)
  }

  public func fetchAcpClientHost(id: String) async throws -> AcpClientHostResponse {
    try await fetchJSON(AcpClientHostResponse.self, from: .acpClientHost(id))
  }

  public func probeAcpClientHost(id: String, agentId: String? = nil) async throws
    -> AcpClientHostProbeResponse
  {
    try await postJSON(
      AcpClientHostProbeResponse.self,
      to: .acpClientHostProbe(id),
      body: DevinAcpBridgeProbeRequest(agentId: agentId)
    )
  }

  public func installAcpClientHost(id: String) async throws -> AcpClientHostInstallResponse {
    try await postJSON(
      AcpClientHostInstallResponse.self,
      to: .acpClientHostInstall(id),
      body: EmptyRequest()
    )
  }

  public func probeDevinAcpBridge(agentId: String? = nil) async throws
    -> DevinAcpBridgeProbeResponse
  {
    try await postJSON(
      DevinAcpBridgeProbeResponse.self,
      to: .devinAcpBridgeProbe,
      body: DevinAcpBridgeProbeRequest(agentId: agentId)
    )
  }

  public func installDevinAcpBridge() async throws -> DevinAcpInstallResponse {
    try await postJSON(
      DevinAcpInstallResponse.self,
      to: .devinAcpBridgeInstall,
      body: EmptyRequest()
    )
  }

  public func unregisterLiveHooks(
    timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds
  ) throws {
    try runBlockingRequest(for: .unregisterLiveHooks, timeout: timeout)
  }

  public func unregisterLiveHooksWithoutBlockingUI(
    timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds
  ) async throws {
    try await runRequest(for: .unregisterLiveHooks, timeout: timeout)
  }

  public func unregisterLiveHooks(
    target: HookRepairTarget,
    timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds
  ) throws {
    try runBlockingRequest(for: .unregisterLiveTargetHooks(target), timeout: timeout)
  }

  public func unregisterLiveHooksWithoutBlockingUI(
    target: HookRepairTarget,
    timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds
  ) async throws {
    try await runRequest(for: .unregisterLiveTargetHooks(target), timeout: timeout)
  }

  public func shutdownServer(
    timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds
  ) throws {
    try runBlockingRequest(for: .shutdown, timeout: timeout)
  }

  public func request(for endpoint: ControlPlaneEndpoint) -> URLRequest {
    let path = endpoint.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
    let url = baseURLProvider().appendingPathComponent(path)
    var components = URLComponents(url: url, resolvingAgainstBaseURL: false)
    components?.queryItems = endpoint.queryItems.isEmpty ? nil : endpoint.queryItems
    var request = URLRequest(url: components?.url ?? url)
    request.httpMethod = endpoint.method
    request.timeoutInterval = endpoint.timeoutInterval
    return request
  }

  private func fetchJSON<Response: Decodable>(
    _ responseType: Response.Type,
    from endpoint: ControlPlaneEndpoint
  ) async throws -> Response {
    let (data, response) = try await session.data(for: request(for: endpoint))
    try validate(response)
    return try JSONDecoder().decode(responseType, from: data)
  }

  private func postJSON<Request: Encodable, Response: Decodable>(
    _ responseType: Response.Type,
    to endpoint: ControlPlaneEndpoint,
    body: Request
  ) async throws -> Response {
    var request = request(for: endpoint)
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = try JSONEncoder().encode(body)
    let (data, response) = try await session.data(for: request)
    try validate(response)
    return try JSONDecoder().decode(responseType, from: data)
  }

  private func postStatus<Request: Encodable>(
    to endpoint: ControlPlaneEndpoint,
    body: Request
  ) async throws {
    var request = request(for: endpoint)
    request.setValue("application/json", forHTTPHeaderField: "Content-Type")
    request.httpBody = try JSONEncoder().encode(body)
    let (_, response) = try await session.data(for: request)
    try validate(response)
  }

  private func runRequest(
    for endpoint: ControlPlaneEndpoint,
    timeout: TimeInterval
  ) async throws {
    var request = request(for: endpoint)
    request.timeoutInterval = timeout
    let (_, response) = try await session.data(for: request)
    try validate(response)
  }

  private func runBlockingRequest(
    for endpoint: ControlPlaneEndpoint,
    timeout: TimeInterval
  ) throws {
    let resultBox = BlockingRequestResultBox()
    let semaphore = DispatchSemaphore(value: 0)
    let task = session.dataTask(with: request(for: endpoint)) { _, response, error in
      defer { semaphore.signal() }
      if let error {
        resultBox.set(.failure(error))
        return
      }

      do {
        try validate(response)
        resultBox.set(.success(()))
      } catch {
        resultBox.set(.failure(error))
      }
    }

    task.resume()
    if semaphore.wait(timeout: .now() + timeout) == .timedOut {
      task.cancel()
      throw ControlPlaneClientError.timeout
    }

    try resultBox.result?.get()
  }
}

private struct EmptyRequest: Encodable {}

private final class BlockingRequestResultBox: @unchecked Sendable {
  private let lock = NSLock()
  private var storedResult: Result<Void, Error>?

  var result: Result<Void, Error>? {
    lock.withLock {
      storedResult
    }
  }

  func set(_ result: Result<Void, Error>) {
    lock.withLock {
      storedResult = result
    }
  }
}

private func validate(_ response: URLResponse?) throws {
  guard let response = response as? HTTPURLResponse else {
    throw ControlPlaneClientError.invalidResponse
  }
  guard (200..<300).contains(response.statusCode) else {
    throw ControlPlaneClientError.requestFailed(statusCode: response.statusCode)
  }
}

public struct ControlPlaneStatusResponse: Codable, Equatable, Sendable {
  public let hooks: HookStatusSummary
  public let codexServers: [CodexServerSummary]
  public let source: SourceStatusSummary

  enum CodingKeys: String, CodingKey {
    case hooks
    case codexServers = "codex_servers"
    case source
  }
}

public struct HookStatusSummary: Codable, Equatable, Sendable {
  public let enabled: Bool
  public let registeredEvents: [String]
  public let activeCommand: String?
  public let owner: String
  public let health: String
  public let issues: [String]
  public let recentFailuresCount: UInt32

  enum CodingKeys: String, CodingKey {
    case enabled
    case registeredEvents = "registered_events"
    case activeCommand = "active_command"
    case owner
    case health
    case issues
    case recentFailuresCount = "recent_failures_count"
  }
}

public struct CodexServerSummary: Codable, Equatable, Sendable {
  public let pid: Int
  public let parentPid: Int?
  public let tty: String?
  public let executable: String
  public let command: String
  public let owner: String
  public let parentProcesses: [ProcessAncestorSummary]

  enum CodingKeys: String, CodingKey {
    case pid
    case parentPid = "parent_pid"
    case tty
    case executable
    case command
    case owner
    case parentProcesses = "parent_processes"
  }
}

public struct ProcessAncestorSummary: Codable, Equatable, Sendable {
  public let pid: Int
  public let parentPid: Int?
  public let executable: String
  public let command: String

  enum CodingKeys: String, CodingKey {
    case pid
    case parentPid = "parent_pid"
    case executable
    case command
  }
}

public struct SourceStatusSummary: Codable, Equatable, Sendable {
  public let codexHome: String
  public let stateDb: String?
  public let logsDb: String?
  public let sessionsRoot: String
  public let health: String
  public let degradedReason: String?

  enum CodingKeys: String, CodingKey {
    case codexHome = "codex_home"
    case stateDb = "state_db"
    case logsDb = "logs_db"
    case sessionsRoot = "sessions_root"
    case health
    case degradedReason = "degraded_reason"
  }
}

public struct AssistantAdapterCapability: Codable, Equatable, Sendable {
  public let assistantKind: String
  public let liveSessions: Bool
  public let toolInventory: Bool
  public let spawnGraph: Bool
  public let diffSummary: Bool
  public let authCapabilities: Bool
  public let runtimes: [AssistantRuntimeSummary]
  public let detail: String

  public var isVisibleInAgentMenu: Bool {
    liveSessions || runtimes.contains { $0.running || $0.installed }
  }

  public var displayTitle: String {
    assistantKind.identifierDisplayTitle
  }

  public var menuStatusTitle: String {
    let runningCount = runtimes.filter(\.running).count
    if runningCount == 1 {
      return "running"
    }
    if runningCount > 1 {
      return "\(runningCount) running"
    }

    let installedCount = runtimes.filter(\.installed).count
    if installedCount == 1 {
      return "installed"
    }
    if installedCount > 1 {
      return "\(installedCount) installed"
    }

    if liveSessions {
      return "sessions"
    }

    return "available"
  }

  enum CodingKeys: String, CodingKey {
    case assistantKind = "assistant_kind"
    case liveSessions = "live_sessions"
    case toolInventory = "tool_inventory"
    case spawnGraph = "spawn_graph"
    case diffSummary = "diff_summary"
    case authCapabilities = "auth_capabilities"
    case runtimes
    case detail
  }

  public init(
    assistantKind: String,
    liveSessions: Bool = false,
    toolInventory: Bool = false,
    spawnGraph: Bool = false,
    diffSummary: Bool = false,
    authCapabilities: Bool = false,
    runtimes: [AssistantRuntimeSummary],
    detail: String
  ) {
    self.assistantKind = assistantKind
    self.liveSessions = liveSessions
    self.toolInventory = toolInventory
    self.spawnGraph = spawnGraph
    self.diffSummary = diffSummary
    self.authCapabilities = authCapabilities
    self.runtimes = runtimes
    self.detail = detail
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.init(
      assistantKind: try container.decode(String.self, forKey: .assistantKind),
      liveSessions: try container.decodeIfPresent(Bool.self, forKey: .liveSessions) ?? false,
      toolInventory: try container.decodeIfPresent(Bool.self, forKey: .toolInventory) ?? false,
      spawnGraph: try container.decodeIfPresent(Bool.self, forKey: .spawnGraph) ?? false,
      diffSummary: try container.decodeIfPresent(Bool.self, forKey: .diffSummary) ?? false,
      authCapabilities: try container.decodeIfPresent(Bool.self, forKey: .authCapabilities)
        ?? false,
      runtimes: try container.decodeIfPresent([AssistantRuntimeSummary].self, forKey: .runtimes)
        ?? [],
      detail: try container.decodeIfPresent(String.self, forKey: .detail) ?? ""
    )
  }
}

public struct AssistantRuntimeSummary: Codable, Equatable, Sendable {
  public let kind: String
  public let running: Bool
  public let installed: Bool
  public let label: String
  public let executable: String?

  public init(
    kind: String,
    running: Bool,
    installed: Bool,
    label: String,
    executable: String? = nil
  ) {
    self.kind = kind
    self.running = running
    self.installed = installed
    self.label = label
    self.executable = executable
  }
}

public struct AgentDetailMenuRow: Equatable, Sendable {
  public let title: String
  public let detail: String?

  public init(title: String, detail: String? = nil) {
    self.title = title
    self.detail = detail?.nilIfBlank
  }
}

public struct DesktopConnectionsResponse: Codable, Equatable, Sendable {
  public let connections: [DesktopConnectionSummary]

  public var agentDetailMenuRows: [AgentDetailMenuRow] {
    connections
      .filter(\.isAgentConnection)
      .sorted { lhs, rhs in
        lhs.sortKey.lexicographicallyPrecedes(rhs.sortKey)
      }
      .map { connection in
        AgentDetailMenuRow(
          title: "\(connection.label): \(connection.status.displayStatusTitle)",
          detail: connection.detail ?? connection.subtitle
        )
      }
  }
}

public struct DesktopMobileStateResponse: Codable, Equatable, Sendable {
  public let globalNotificationID: String?
  public let defaultNotificationTargetIDs: [String]
  public let notifications: [DesktopNotificationRouteSummary]

  enum CodingKeys: String, CodingKey {
    case globalNotificationID = "globalNotificationId"
    case defaultNotificationTargetIDs = "defaultNotificationTargetIds"
    case notifications
  }

  public init(
    globalNotificationID: String? = nil,
    defaultNotificationTargetIDs: [String] = [],
    notifications: [DesktopNotificationRouteSummary] = []
  ) {
    self.globalNotificationID = globalNotificationID
    self.defaultNotificationTargetIDs = defaultNotificationTargetIDs
    self.notifications = notifications
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    let globalNotificationID = try container.decodeIfPresent(
      String.self, forKey: .globalNotificationID)
    let defaultNotificationTargetIDs =
      try container
      .decodeIfPresent([String].self, forKey: .defaultNotificationTargetIDs)
      ?? []
    self.init(
      globalNotificationID: globalNotificationID,
      defaultNotificationTargetIDs: defaultNotificationTargetIDs,
      notifications: try container.decodeIfPresent(
        [DesktopNotificationRouteSummary].self,
        forKey: .notifications
      ) ?? []
    )
  }
}

public struct DesktopNotificationRouteSummary: Codable, Equatable, Sendable, Identifiable {
  public let id: String
  public let label: String
  public let channel: String

  public init(id: String, label: String, channel: String) {
    self.id = id
    self.label = label
    self.channel = channel
  }
}

public struct DesktopPushDevicesResponse: Codable, Equatable, Sendable {
  public let devices: [DesktopPushDeviceSummary]

  public init(devices: [DesktopPushDeviceSummary] = []) {
    self.devices = devices
  }
}

public struct DesktopPushDeviceSummary: Codable, Equatable, Sendable, Identifiable {
  public let installationID: String
  public let state: String
  public let deviceName: String?
  public let canTest: Bool

  public var id: String {
    installationID
  }

  enum CodingKeys: String, CodingKey {
    case installationID = "installationId"
    case state
    case deviceName
    case canTest
  }

  public init(
    installationID: String, state: String, deviceName: String? = nil, canTest: Bool = false
  ) {
    self.installationID = installationID
    self.state = state
    self.deviceName = deviceName
    self.canTest = canTest
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    installationID = try container.decode(String.self, forKey: .installationID)
    state = try container.decodeIfPresent(String.self, forKey: .state) ?? ""
    deviceName = try container.decodeIfPresent(String.self, forKey: .deviceName)
    canTest = try container.decodeIfPresent(Bool.self, forKey: .canTest) ?? (state == "enabled")
  }
}

public struct NotificationTargetOption: Equatable, Sendable, Identifiable {
  public let id: String
  public let title: String
  public let detail: String?
  public let systemImageName: String
  public let available: Bool

  public init(
    id: String,
    title: String,
    detail: String? = nil,
    systemImageName: String,
    available: Bool = true
  ) {
    self.id = id
    self.title = title
    self.detail = detail?.nilIfBlank
    self.systemImageName = systemImageName
    self.available = available
  }
}

public enum NotificationTargetOptions {
  private enum BuiltInID {
    static let iphone = "iphone"
    static let macOS = "macos"
  }

  fileprivate enum SystemImageName {
    static let bell = "bell.badge"
    static let iphone = "iphone"
    static let macOS = "macbook"
    static let slack = "number"
    static let telegram = "paperplane.fill"
  }

  public static func build(
    mobileState: DesktopMobileStateResponse?,
    pushDevices: DesktopPushDevicesResponse?
  ) -> [NotificationTargetOption] {
    var options = [
      NotificationTargetOption(
        id: BuiltInID.macOS,
        title: "macOS",
        detail: "Local menu bar alerts",
        systemImageName: SystemImageName.macOS
      )
    ]

    if let pushDevices, !pushDevices.devices.isEmpty {
      let pushReadyDevices = pushDevices.devices.filter(\.canTest)
      let available = !pushReadyDevices.isEmpty
      options.append(
        NotificationTargetOption(
          id: BuiltInID.iphone,
          title: "iPhone",
          detail: iPhoneDetail(
            devices: available ? pushReadyDevices : pushDevices.devices,
            available: available
          ),
          systemImageName: SystemImageName.iphone,
          available: available
        )
      )
    }

    options.append(contentsOf: (mobileState?.notifications ?? []).map(routeOption))
    return uniqueOptions(options)
  }

  private static func iPhoneDetail(devices: [DesktopPushDeviceSummary], available: Bool) -> String {
    if devices.count == 1 {
      let deviceName = devices[0].deviceName ?? "1 registered iPhone"
      return available ? deviceName : "\(deviceName) - push not ready"
    }
    return available
      ? "\(devices.count) push-ready iPhones"
      : "\(devices.count) registered iPhones - push not ready"
  }

  private static func routeOption(_ notification: DesktopNotificationRouteSummary)
    -> NotificationTargetOption
  {
    NotificationTargetOption(
      id: notification.id,
      title: notification.label,
      detail: notification.channel.identifierDisplayTitle,
      systemImageName: notification.channel.notificationTargetSystemImageName
    )
  }

  private static func uniqueOptions(_ options: [NotificationTargetOption])
    -> [NotificationTargetOption]
  {
    var seen = Set<String>()
    return options.filter { option in
      seen.insert(option.id).inserted
    }
  }
}

public enum SettingsPreservationCategory: String, CaseIterable, Identifiable, Sendable {
  case mobileRoute
  case notificationTargets
  case handoff
  case assistantDefaults
  case siriDefaults
  case companionPreferences
  case connectionPairing

  public static let userDefaultsKey = "settingsPreservationCategories"
  public static let allOptions: [SettingsPreservationCategory] = [
    .mobileRoute,
    .notificationTargets,
    .handoff,
    .assistantDefaults,
    .siriDefaults,
    .companionPreferences,
    .connectionPairing,
  ]
  public static let defaultCategories: Set<SettingsPreservationCategory> = [
    .mobileRoute,
    .notificationTargets,
    .handoff,
    .assistantDefaults,
    .siriDefaults,
    .companionPreferences,
  ]

  public var id: String {
    rawValue
  }

  public var menuTitle: String {
    switch self {
    case .mobileRoute:
      "Mobile route"
    case .notificationTargets:
      "Notification targets"
    case .handoff:
      "Handoff preferences"
    case .assistantDefaults:
      "Assistant defaults"
    case .siriDefaults:
      "Siri defaults"
    case .companionPreferences:
      "iPhone UI preferences"
    case .connectionPairing:
      "Connection pairing"
    }
  }

  public var detail: String {
    switch self {
    case .mobileRoute:
      "Remote, Tailscale, or LAN preference"
    case .notificationTargets:
      "macOS, iPhone, Telegram, and route choices"
    case .handoff:
      "Focus assist, hotkey, and hold duration"
    case .assistantDefaults:
      "Prompt, scope, preset, and assistant surface"
    case .siriDefaults:
      "Pinned and current Siri session targets"
    case .companionPreferences:
      "Appearance, quick actions, onboarding, and search"
    case .connectionPairing:
      "Pairing URLs and tokens; opt in because credentials are involved"
    }
  }

  public static func stored(
    in userDefaults: UserDefaults = .standard,
    key: String = userDefaultsKey
  ) -> Set<SettingsPreservationCategory> {
    guard userDefaults.object(forKey: key) != nil else {
      return defaultCategories
    }

    let rawValues = userDefaults.stringArray(forKey: key) ?? []
    return Set(rawValues.compactMap(SettingsPreservationCategory.init(rawValue:)))
  }

  public static func save(
    _ categories: Set<SettingsPreservationCategory>,
    in userDefaults: UserDefaults = .standard,
    key: String = userDefaultsKey
  ) {
    let rawValues =
      allOptions
      .filter { categories.contains($0) }
      .map(\.rawValue)
    userDefaults.set(rawValues, forKey: key)
  }
}

public enum MacSystemSettingsTarget: String, CaseIterable, Identifiable, Sendable {
  case notifications
  case accessibility
  case screenRecording
  case automation

  public static let allOptions: [MacSystemSettingsTarget] = [
    .notifications,
    .accessibility,
    .screenRecording,
    .automation,
  ]

  private enum URLString {
    static let notifications =
      "x-apple.systempreferences:com.apple.Notifications-Settings.extension"
    static let accessibility =
      "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
    static let screenRecording =
      "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
    static let automation =
      "x-apple.systempreferences:com.apple.preference.security?Privacy_Automation"
  }

  public var id: String {
    rawValue
  }

  public var menuTitle: String {
    switch self {
    case .notifications:
      "Notifications"
    case .accessibility:
      "Accessibility"
    case .screenRecording:
      "Screen & System Audio Recording"
    case .automation:
      "Automation"
    }
  }

  public var detail: String {
    switch self {
    case .notifications:
      "macOS alert delivery for looper stop notices"
    case .accessibility:
      "Desktop UI control for supported clients"
    case .screenRecording:
      "Desktop capture for visual inspection and control"
    case .automation:
      "Apple Events access when controlling desktop apps"
    }
  }

  public var url: URL? {
    switch self {
    case .notifications:
      URL(string: URLString.notifications)
    case .accessibility:
      URL(string: URLString.accessibility)
    case .screenRecording:
      URL(string: URLString.screenRecording)
    case .automation:
      URL(string: URLString.automation)
    }
  }
}

public struct DesktopConnectionSummary: Codable, Equatable, Sendable {
  public let id: String
  public let kind: String
  public let label: String
  public let status: String
  public let subtitle: String?
  public let detail: String?

  public var isAgentConnection: Bool {
    kind != "mobile"
  }

  fileprivate var sortKey: String {
    "\(kind)\u{0}\(label)\u{0}\(id)"
  }
}

extension String {
  fileprivate var nilIfBlank: String? {
    let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
    return trimmed.isEmpty ? nil : trimmed
  }

  fileprivate var identifierDisplayTitle: String {
    splitIdentifierWords().map(\.displayTitleWord).joined(separator: " ")
  }

  fileprivate var notificationTargetSystemImageName: String {
    switch trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
    case "slack":
      return NotificationTargetOptions.SystemImageName.slack
    case "telegram":
      return NotificationTargetOptions.SystemImageName.telegram
    default:
      return NotificationTargetOptions.SystemImageName.bell
    }
  }

  fileprivate var displayStatusTitle: String {
    splitIdentifierWords().map(\.displayTitleWord).joined(separator: " ")
  }

  private func splitIdentifierWords() -> [String] {
    split { character in
      character == "-" || character == "_" || character == " "
    }
    .map(String.init)
  }

  private var displayTitleWord: String {
    switch lowercased() {
    case "acp":
      return "ACP"
    case "ai":
      return "AI"
    case "api":
      return "API"
    case "cli":
      return "CLI"
    case "id":
      return "ID"
    case "ios":
      return "iOS"
    case "macos":
      return "macOS"
    case "ui":
      return "UI"
    default:
      return prefix(1).uppercased() + dropFirst()
    }
  }
}

public struct AcpLaunchMetadataSummary: Codable, Equatable, Sendable {
  public let configured: Bool
  public let methods: [String]
}

public struct AcpTargetSummary: Codable, Equatable, Sendable {
  public let id: String
  public let client: String
  public let clientName: String
  public let agentId: String
  public let name: String
  public let source: String
  public let sourcePath: String?
  public let enabled: Bool
  public let preferred: Bool
  public let launchConfigured: Bool
  public let launch: AcpLaunchMetadataSummary
  public let ready: Bool
  public let status: String
  public let detail: String

  enum CodingKeys: String, CodingKey {
    case id
    case client
    case clientName = "client_name"
    case agentId = "agent_id"
    case name
    case source
    case sourcePath = "source_path"
    case enabled
    case preferred
    case launchConfigured = "launch_configured"
    case launch
    case ready
    case status
    case detail
  }
}

public struct ZedStatus: Codable, Equatable, Sendable {
  public static let unavailable = ZedStatus(
    settingsPath: "",
    settingsExists: false,
    running: false,
    installed: false,
    summary: "Unavailable",
    acpTargetCount: 0,
    acpTargets: []
  )

  public let settingsPath: String
  public let settingsExists: Bool
  public let running: Bool
  public let installed: Bool
  public let summary: String
  public let acpTargetCount: Int
  public let acpTargets: [ZedAcpTargetSummary]

  enum CodingKeys: String, CodingKey {
    case settingsPath = "settings_path"
    case settingsExists = "settings_exists"
    case running
    case installed
    case summary
    case acpTargetCount = "acp_target_count"
    case acpTargets = "acp_targets"
  }
}

public struct ZedAcpTargetSummary: Codable, Equatable, Sendable {
  public let id: String
  public let name: String
  public let targetType: String?
  public let launchConfigured: Bool
  public let launch: AcpLaunchMetadataSummary

  enum CodingKeys: String, CodingKey {
    case id
    case name
    case targetType = "target_type"
    case launchConfigured = "launch_configured"
    case launch
  }
}

public struct GrokBuildStatus: Codable, Equatable, Sendable {
  public let hooks: GrokBuildHookStatus
  public let sessionCount: Int
  public let activeSessionCount: Int

  enum CodingKeys: String, CodingKey {
    case hooks
    case sessionCount = "session_count"
    case activeSessionCount = "active_session_count"
  }
}

public struct GrokBuildHookStatus: Codable, Equatable, Sendable {
  public let health: String
  public let owner: String
  public let registeredEvents: [String]

  enum CodingKeys: String, CodingKey {
    case health
    case owner
    case registeredEvents = "registered_events"
  }
}

public struct DesktopSnapshotResponse: Codable, Equatable, Sendable {
  public let revision: String?
  public let controlPlane: ControlPlaneStatusResponse
  public let devinDesktop: DevinDesktopStatus
  public let grokBuild: GrokBuildStatus?
  public let threadCount: Int
  public let activeThreadCount: Int
  public let archivedThreadCount: Int
  public let threads: [DesktopThreadSummary]
  public let automations: [DesktopAutomationSummary]
  public let goals: [GoalSummary]
  public let compactions: [CompactionEventSummary]
  public let assistantAdapters: [AssistantAdapterCapability]
  public let acpTargets: [AcpTargetSummary]
  public let zed: ZedStatus

  public var grokBuildAdapter: AssistantAdapterCapability? {
    assistantAdapters.first { $0.assistantKind == "grok-build" }
  }

  public var grokBuildStatusTitle: String {
    guard let adapter = grokBuildAdapter else {
      return "Unavailable"
    }

    let cliRuntime = adapter.runtimes.first { $0.label == "Grok Build CLI" }
    let sessionRuntime = adapter.runtimes.first { $0.label == "Grok Build session" }

    if sessionRuntime?.running == true {
      return "Session active"
    }
    if cliRuntime?.running == true {
      return "CLI running"
    }
    if cliRuntime?.installed == true {
      return "Installed"
    }
    return "Missing"
  }

  public var grokBuildHooksTitle: String {
    grokBuild?.hooks.health.capitalized ?? "Unavailable"
  }

  public var zedStatusTitle: String {
    zed.summary
  }

  public func agentDetailMenuRows(connections: DesktopConnectionsResponse?) -> [AgentDetailMenuRow]
  {
    if let connectionRows = connections?.agentDetailMenuRows, !connectionRows.isEmpty {
      return connectionRows
    }

    return
      assistantAdapters
      .filter(\.isVisibleInAgentMenu)
      .sorted { lhs, rhs in
        lhs.displayTitle.localizedStandardCompare(rhs.displayTitle) == .orderedAscending
      }
      .map { adapter in
        AgentDetailMenuRow(
          title: "\(adapter.displayTitle): \(adapter.menuStatusTitle)",
          detail: adapter.detail
        )
      }
  }

  enum CodingKeys: String, CodingKey {
    case revision
    case controlPlane = "control_plane"
    case devinDesktop = "devin_desktop"
    case grokBuild = "grok_build"
    case threadCount = "thread_count"
    case activeThreadCount = "active_thread_count"
    case archivedThreadCount = "archived_thread_count"
    case threads
    case automations
    case goals
    case compactions
    case assistantAdapters = "assistant_adapters"
    case acpTargets = "acp_targets"
    case zed
  }

  public init(
    revision: String? = nil,
    controlPlane: ControlPlaneStatusResponse,
    devinDesktop: DevinDesktopStatus,
    grokBuild: GrokBuildStatus? = nil,
    threadCount: Int,
    activeThreadCount: Int,
    archivedThreadCount: Int,
    threads: [DesktopThreadSummary],
    automations: [DesktopAutomationSummary],
    goals: [GoalSummary],
    compactions: [CompactionEventSummary],
    assistantAdapters: [AssistantAdapterCapability] = [],
    acpTargets: [AcpTargetSummary] = [],
    zed: ZedStatus = .unavailable
  ) {
    self.revision = revision
    self.controlPlane = controlPlane
    self.devinDesktop = devinDesktop
    self.grokBuild = grokBuild
    self.threadCount = threadCount
    self.activeThreadCount = activeThreadCount
    self.archivedThreadCount = archivedThreadCount
    self.threads = threads
    self.automations = automations
    self.goals = goals
    self.compactions = compactions
    self.assistantAdapters = assistantAdapters
    self.acpTargets = acpTargets
    self.zed = zed
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.init(
      revision: try container.decodeIfPresent(String.self, forKey: .revision),
      controlPlane: try container.decode(ControlPlaneStatusResponse.self, forKey: .controlPlane),
      devinDesktop: try container.decode(DevinDesktopStatus.self, forKey: .devinDesktop),
      grokBuild: try container.decodeIfPresent(GrokBuildStatus.self, forKey: .grokBuild),
      threadCount: try container.decode(Int.self, forKey: .threadCount),
      activeThreadCount: try container.decode(Int.self, forKey: .activeThreadCount),
      archivedThreadCount: try container.decode(Int.self, forKey: .archivedThreadCount),
      threads: try container.decode([DesktopThreadSummary].self, forKey: .threads),
      automations: try container.decode([DesktopAutomationSummary].self, forKey: .automations),
      goals: try container.decode([GoalSummary].self, forKey: .goals),
      compactions: try container.decode([CompactionEventSummary].self, forKey: .compactions),
      assistantAdapters: try container.decodeIfPresent(
        [AssistantAdapterCapability].self,
        forKey: .assistantAdapters
      ) ?? [],
      acpTargets: try container.decodeIfPresent([AcpTargetSummary].self, forKey: .acpTargets) ?? [],
      zed: try container.decodeIfPresent(ZedStatus.self, forKey: .zed) ?? .unavailable
    )
  }
}

public struct AcpClientHostsResponse: Codable, Equatable, Sendable {
  public let hosts: [AcpClientHost]
}

public struct AcpClientHostResponse: Codable, Equatable, Sendable {
  public let host: AcpClientHost
}

public struct AcpClientHostProbeResponse: Codable, Equatable, Sendable {
  public let host: AcpClientHost
  public let probe: AcpClientHostProbe
}

public struct AcpClientHostInstallResponse: Codable, Equatable, Sendable {
  public let host: AcpClientHost
  public let install: AcpClientHostInstall
}

public struct AcpClientHost: Codable, Equatable, Sendable {
  public let id: String
  public let label: String
  public let running: Bool
  public let installed: Bool
  public let registry: AcpClientHostRegistry
  public let agents: [AcpClientHostAgent]
  public let sessions: [AcpClientHostSession]
  public let actions: [AcpClientHostAction]
  public let limitations: [String]
  public let runtime: AcpClientHostRuntime?

  public var enabledAgentCount: Int {
    agents.filter(\.enabled).count
  }

  public var preferredAgent: AcpClientHostAgent? {
    agents.first(where: \.preferred)
  }

  public var defaultProbeAgent: AcpClientHostAgent? {
    guard let defaultAgentID = actions.compactMap(\.defaultAgentID).first else {
      return agents.first(where: \.probeCapable)
    }
    return agents.first { $0.id == defaultAgentID } ?? agents.first(where: \.probeCapable)
  }
}

public struct AcpClientHostRegistry: Codable, Equatable, Sendable {
  public let path: String
  public let exists: Bool
  public let version: String?
  public let agentCount: Int

  enum CodingKeys: String, CodingKey {
    case path
    case exists
    case version
    case agentCount = "agent_count"
  }
}

public struct AcpClientHostAgent: Codable, Equatable, Sendable {
  public let id: String
  public let name: String
  public let version: String?
  public let description: String?
  public let enabled: Bool
  public let preferred: Bool
  public let launchConfigured: Bool
  public let controlLevel: String
  public let supportsSessions: Bool
  public let supportsPrompt: Bool
  public let supportsCancel: Bool
  public let source: String

  public var probeCapable: Bool {
    enabled && launchConfigured && controlLevel == "agent-configured"
  }

  enum CodingKeys: String, CodingKey {
    case id
    case name
    case version
    case description
    case enabled
    case preferred
    case launchConfigured = "launch_configured"
    case controlLevel = "control_level"
    case supportsSessions = "supports_sessions"
    case supportsPrompt = "supports_prompt"
    case supportsCancel = "supports_cancel"
    case source
  }
}

public struct AcpClientHostSession: Codable, Equatable, Sendable {
  public let threadID: String
  public let sessionID: String
  public let providerID: String
  public let title: String?
  public let cwd: String?
  public let status: String
  public let archived: Bool
  public let updatedAtMs: Int64?

  enum CodingKeys: String, CodingKey {
    case threadID = "thread_id"
    case sessionID = "session_id"
    case providerID = "provider_id"
    case title
    case cwd
    case status
    case archived
    case updatedAtMs = "updated_at_ms"
  }
}

public struct AcpClientHostAction: Codable, Equatable, Sendable {
  public let id: String
  public let label: String
  public let method: String
  public let path: String
  public let defaultAgentID: String?

  enum CodingKeys: String, CodingKey {
    case id
    case label
    case method
    case path
    case defaultAgentID = "default_agent_id"
  }
}

public struct AcpClientHostRuntime: Codable, Equatable, Sendable {
  public let connected: Bool
  public let connectionCount: Int
  public let sessionCount: Int

  enum CodingKeys: String, CodingKey {
    case connected
    case connectionCount = "connection_count"
    case sessionCount = "session_count"
  }
}

public struct AcpClientHostProbe: Codable, Equatable, Sendable {
  public let ok: Bool
  public let status: String
  public let agentID: String?
  public let name: String?
  public let controlLevel: String
  public let ready: Bool
  public let probeKind: String
  public let launchConfigured: Bool
  public let launchMethods: [String]
  public let supportedMethods: [String]
  public let blockers: [String]
  public let detail: String

  enum CodingKeys: String, CodingKey {
    case ok
    case status
    case agentID = "agent_id"
    case name
    case controlLevel = "control_level"
    case ready
    case probeKind = "probe_kind"
    case launchConfigured = "launch_configured"
    case launchMethods = "launch_methods"
    case supportedMethods = "supported_methods"
    case blockers
    case detail
  }
}

public struct AcpClientHostInstall: Codable, Equatable, Sendable {
  public let clientID: String
  public let installedAgentID: String
  public let registryPath: String
  public let settingsPath: String
  public let transportURL: String
  public let preferredAgent: String

  enum CodingKeys: String, CodingKey {
    case clientID = "client_id"
    case installedAgentID = "installed_agent_id"
    case registryPath = "registry_path"
    case settingsPath = "settings_path"
    case transportURL = "transport_url"
    case preferredAgent = "preferred_agent"
  }
}

public struct DevinDesktopStatus: Codable, Equatable, Sendable {
  public let installations: [DevinInstallationStatus]
  public let acpBridge: DevinAcpBridgeStatus

  public var running: Bool {
    installations.contains(where: \.running)
  }

  public var installed: Bool {
    installations.contains(where: \.installed)
  }

  enum CodingKeys: String, CodingKey {
    case installations
    case acpBridge = "acp_bridge"
  }

  public init(
    installations: [DevinInstallationStatus] = [],
    acpBridge: DevinAcpBridgeStatus
  ) {
    self.installations = installations
    self.acpBridge = acpBridge
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    self.init(
      installations: try container.decodeIfPresent(
        [DevinInstallationStatus].self, forKey: .installations) ?? [],
      acpBridge: try container.decode(DevinAcpBridgeStatus.self, forKey: .acpBridge)
    )
  }
}

public struct DevinInstallationStatus: Codable, Equatable, Sendable {
  public let id: String
  public let label: String
  public let channel: String
  public let running: Bool
  public let installed: Bool
  public let appSupportPath: String
  public let settingsPath: String
  public let settingsExists: Bool
  public let acpEnabled: Bool?
  public let preferredAgent: String?
  public let enabledAgents: [String]

  enum CodingKeys: String, CodingKey {
    case id
    case label
    case channel
    case running
    case installed
    case appSupportPath = "app_support_path"
    case settingsPath = "settings_path"
    case settingsExists = "settings_exists"
    case acpEnabled = "acp_enabled"
    case preferredAgent = "preferred_agent"
    case enabledAgents = "enabled_agents"
  }
}

public struct DevinAcpBridgeStatus: Codable, Equatable, Sendable {
  public let available: Bool
  public let controlLevel: String
  public let summary: String
  public let actions: [DevinAcpBridgeAction]
  public let agents: [DevinAcpBridgeAgent]

  public var defaultProbeAgent: DevinAcpBridgeAgent? {
    guard let defaultAgentId = actions.compactMap(\.defaultAgentId).first else {
      return agents.first(where: \.probeCapable)
    }
    return agents.first { $0.id == defaultAgentId } ?? agents.first(where: \.probeCapable)
  }

  enum CodingKeys: String, CodingKey {
    case available
    case controlLevel = "control_level"
    case summary
    case actions
    case agents
  }
}

public struct DevinAcpBridgeAction: Codable, Equatable, Sendable {
  public let id: String
  public let label: String
  public let method: String
  public let path: String
  public let defaultAgentId: String?

  enum CodingKeys: String, CodingKey {
    case id
    case label
    case method
    case path
    case defaultAgentId = "default_agent_id"
  }
}

public struct DevinAcpBridgeAgent: Codable, Equatable, Sendable {
  public let id: String
  public let name: String
  public let enabled: Bool
  public let preferred: Bool
  public let launchConfigured: Bool
  public let controlLevel: String

  public var probeCapable: Bool {
    enabled && launchConfigured && controlLevel == "agent-configured"
  }

  enum CodingKeys: String, CodingKey {
    case id
    case name
    case enabled
    case preferred
    case launchConfigured = "launch_configured"
    case controlLevel = "control_level"
  }
}

public struct DevinAcpBridgeProbeRequest: Codable, Equatable, Sendable {
  public let agentId: String?

  public init(agentId: String?) {
    self.agentId = agentId
  }

  enum CodingKeys: String, CodingKey {
    case agentId = "agentId"
  }
}

public struct DevinAcpBridgeProbeResponse: Codable, Equatable, Sendable {
  public let probe: DevinAcpBridgeProbe
  public let bridge: DevinAcpBridgeStatus
}

public struct DevinAcpInstallResponse: Codable, Equatable, Sendable {
  public let install: DevinAcpInstallResult
  public let bridge: DevinAcpBridgeStatus
}

public struct DevinAcpInstallResult: Codable, Equatable, Sendable {
  public let installedAgentId: String
  public let registryPath: String
  public let settingsPath: String
  public let websocketURL: String
  public let acpEnabled: Bool
  public let preferredAgent: String

  enum CodingKeys: String, CodingKey {
    case installedAgentId = "installed_agent_id"
    case registryPath = "registry_path"
    case settingsPath = "settings_path"
    case websocketURL = "websocket_url"
    case acpEnabled = "acp_enabled"
    case preferredAgent = "preferred_agent"
  }
}

public struct DevinAcpBridgeProbe: Codable, Equatable, Sendable {
  public let ok: Bool
  public let status: String
  public let agentId: String?
  public let name: String?
  public let ready: Bool
  public let launchConfigured: Bool
  public let blockers: [String]
  public let detail: String

  enum CodingKeys: String, CodingKey {
    case ok
    case status
    case agentId = "agent_id"
    case name
    case ready
    case launchConfigured = "launch_configured"
    case blockers
    case detail
  }
}

public struct MobileHealthResponse: Codable, Equatable, Sendable {
  public let ok: Bool
  public let baseURL: String
  public let baseURLs: [String]
  public let grpcBaseURL: String
  public let grpcBaseURLs: [String]
  public let requiresAuthentication: Bool
  public let tailscale: MobileTailscaleStatus?

  public var preferredHandoffBaseURL: URL? {
    preferredReachableHandoffBaseURL()
      ?? URL(string: baseURL)
  }

  public var preferredReachableHandoffBaseURL: URL? {
    preferredReachableHandoffBaseURL()
  }

  public var supportsNativeHandoff: Bool {
    ok && requiresAuthentication && preferredReachableHandoffBaseURL() != nil
  }

  public var preferredRealtimeBaseURLs: [URL] {
    var seen = Set<String>()
    return ([grpcBaseURL] + grpcBaseURLs)
      .compactMap(URL.init(string:))
      .filter { url in
        !url.absoluteString.isEmpty && seen.insert(url.absoluteString).inserted
      }
  }

  public var routeSummaryTitle: String {
    routeSummaryTitle()
  }

  public func preferredReachableHandoffBaseURL(
    preference: MobileRoutePreference = .defaultOption
  ) -> URL? {
    rankedBaseURLs(preference: preference)
      .first(where: { !$0.isLoopbackHost })
  }

  public func routeSummaryTitle(
    preference: MobileRoutePreference = .defaultOption
  ) -> String {
    guard let reachableBaseURL = preferredReachableHandoffBaseURL(preference: preference) else {
      return "Loopback only"
    }

    return
      "\(reachableBaseURL.routeTitle): \(reachableBaseURL.host ?? reachableBaseURL.absoluteString)"
  }

  private func rankedBaseURLs(preference: MobileRoutePreference) -> [URL] {
    let advertisedTailscaleBaseURLs = [tailscale?.baseURL]
      .compactMap { $0 }
      .compactMap(URL.init(string:))
    let healthBaseURLs = baseURLs.compactMap(URL.init(string:))

    return MobileRouteURLPolicy.sortedUniqueURLs(
      advertisedTailscaleBaseURLs + healthBaseURLs,
      preference: preference
    )
  }

  enum CodingKeys: String, CodingKey {
    case ok
    case baseURL
    case baseURLs
    case grpcBaseURL
    case grpcBaseURLs
    case requiresAuthentication
    case tailscale
  }

  public init(
    ok: Bool,
    baseURL: String,
    baseURLs: [String],
    grpcBaseURL: String = "",
    grpcBaseURLs: [String] = [],
    requiresAuthentication: Bool,
    tailscale: MobileTailscaleStatus? = nil
  ) {
    self.ok = ok
    self.baseURL = baseURL
    self.baseURLs = baseURLs
    self.grpcBaseURL = grpcBaseURL
    self.grpcBaseURLs = grpcBaseURLs
    self.requiresAuthentication = requiresAuthentication
    self.tailscale = tailscale
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    ok = try container.decodeIfPresent(Bool.self, forKey: .ok) ?? false
    baseURL = try container.decodeIfPresent(String.self, forKey: .baseURL) ?? ""
    baseURLs = try container.decodeIfPresent([String].self, forKey: .baseURLs) ?? []
    grpcBaseURL = try container.decodeIfPresent(String.self, forKey: .grpcBaseURL) ?? ""
    grpcBaseURLs = try container.decodeIfPresent([String].self, forKey: .grpcBaseURLs) ?? []
    requiresAuthentication =
      try container.decodeIfPresent(Bool.self, forKey: .requiresAuthentication) ?? true
    tailscale = try container.decodeIfPresent(MobileTailscaleStatus.self, forKey: .tailscale)
  }
}

public struct MobileTailscaleStatus: Codable, Equatable, Sendable {
  public let available: Bool
  public let running: Bool
  public let backendState: String?
  public let baseURL: String?
  public let grpcBaseURL: String?
  public let dnsName: String?
  public let hostname: String?
  public let ipAddresses: [String]
  public let magicDNSEnabled: Bool
  public let magicDNSSuffix: String?
  public let source: String?
  public let tailnetName: String?
  public let version: String?
  public let health: [String]

  public var statusTitle: String {
    if running {
      return "Running"
    }

    if available {
      return backendState ?? "Available"
    }

    return "Unavailable"
  }

  public var routeDetailTitle: String {
    [
      nonEmpty(dnsName),
      nonEmpty(baseURL),
      nonEmpty(tailnetName).map { "tailnet \($0)" },
    ]
    .compactMap { $0 }
    .joined(separator: " - ")
  }

  enum CodingKeys: String, CodingKey {
    case available
    case running
    case backendState
    case baseURL
    case grpcBaseURL
    case dnsName
    case hostname
    case ipAddresses
    case magicDNSEnabled
    case magicDNSSuffix
    case source
    case tailnetName
    case version
    case health
  }

  public init(
    available: Bool,
    running: Bool,
    backendState: String? = nil,
    baseURL: String? = nil,
    grpcBaseURL: String? = nil,
    dnsName: String? = nil,
    hostname: String? = nil,
    ipAddresses: [String] = [],
    magicDNSEnabled: Bool = false,
    magicDNSSuffix: String? = nil,
    source: String? = nil,
    tailnetName: String? = nil,
    version: String? = nil,
    health: [String] = []
  ) {
    self.available = available
    self.running = running
    self.backendState = backendState
    self.baseURL = baseURL
    self.grpcBaseURL = grpcBaseURL
    self.dnsName = dnsName
    self.hostname = hostname
    self.ipAddresses = ipAddresses
    self.magicDNSEnabled = magicDNSEnabled
    self.magicDNSSuffix = magicDNSSuffix
    self.source = source
    self.tailnetName = tailnetName
    self.version = version
    self.health = health
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    available = try container.decodeIfPresent(Bool.self, forKey: .available) ?? false
    running = try container.decodeIfPresent(Bool.self, forKey: .running) ?? false
    backendState = try container.decodeIfPresent(String.self, forKey: .backendState)
    baseURL = try container.decodeIfPresent(String.self, forKey: .baseURL)
    grpcBaseURL = try container.decodeIfPresent(String.self, forKey: .grpcBaseURL)
    dnsName = try container.decodeIfPresent(String.self, forKey: .dnsName)
    hostname = try container.decodeIfPresent(String.self, forKey: .hostname)
    ipAddresses = try container.decodeIfPresent([String].self, forKey: .ipAddresses) ?? []
    magicDNSEnabled = try container.decodeIfPresent(Bool.self, forKey: .magicDNSEnabled) ?? false
    magicDNSSuffix = try container.decodeIfPresent(String.self, forKey: .magicDNSSuffix)
    source = try container.decodeIfPresent(String.self, forKey: .source)
    tailnetName = try container.decodeIfPresent(String.self, forKey: .tailnetName)
    version = try container.decodeIfPresent(String.self, forKey: .version)
    health = try container.decodeIfPresent([String].self, forKey: .health) ?? []
  }

  private func nonEmpty(_ value: String?) -> String? {
    guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines), !value.isEmpty else {
      return nil
    }

    return value
  }
}

public enum MobileRouteURLPolicy {
  public static let defaultHTTPAPIPort = 8765
  public static let defaultRealtimeGRPCPort = 8766

  public static func canonicalRealtimeGRPCBaseURL(for baseURL: URL) -> URL {
    baseURL.canonicalRealtimeGRPCBaseURL
  }

  public static func canonicalHTTPAPIBaseURL(for realtimeBaseURL: URL) -> URL {
    realtimeBaseURL.canonicalHTTPAPIBaseURL
  }

  public static func isLoopbackURL(_ url: URL) -> Bool {
    url.isLoopbackHost
  }

  public static func routeTitle(for url: URL) -> String {
    url.routeTitle
  }

  public static func sortedUniqueURLs(
    _ urls: [URL],
    preference: MobileRoutePreference
  ) -> [URL] {
    var seen = Set<String>()
    return urls
      .filter { url in
        seen.insert(url.absoluteString).inserted
      }
      .enumerated()
      .sorted { lhs, rhs in
        let lhsPriority = lhs.element.routePriority(preference: preference)
        let rhsPriority = rhs.element.routePriority(preference: preference)

        guard lhsPriority != rhsPriority else {
          return lhs.offset < rhs.offset
        }

        return lhsPriority < rhsPriority
      }
      .map(\.element)
  }
}

extension URL {
  fileprivate enum MobileRoute {
    case remote
    case tailscale
    case lan
    case loopback
  }

  fileprivate enum RoutePriority {
    static let first = 0
    static let second = 1
    static let third = 2
    static let fourth = 3
  }

  fileprivate enum LANNetwork {
    static let ipv4OctetCount = 4
    static let ipv4OctetRange = 0...255
    static let localHostnameSuffix = ".local"
    static let privateTenFirstOctet = 10
    static let privateOneSevenTwoFirstOctet = 172
    static let privateOneSevenTwoSecondOctetRange = 16...31
    static let privateOneNineTwoFirstOctet = 192
    static let privateOneNineTwoSecondOctet = 168
    static let linkLocalFirstOctet = 169
    static let linkLocalSecondOctet = 254
  }

  fileprivate var isLoopbackHost: Bool {
    guard let host else {
      return false
    }
    return host == "127.0.0.1" || host == "localhost" || host == "::1"
  }

  fileprivate var isTailscaleHost: Bool {
    guard let host = normalizedHost else {
      return false
    }

    return TailscaleNetworkPattern.isTailscaleHost(host)
  }

  fileprivate var routeTitle: String {
    switch mobileRoute {
    case .remote:
      "Remote"
    case .tailscale:
      "Tailscale"
    case .lan:
      "LAN"
    case .loopback:
      "Loopback"
    }
  }

  fileprivate func routePriority(preference: MobileRoutePreference) -> Int {
    switch preference {
    case .remote:
      tailscalePriority
    case .tailscale:
      tailscalePriority
    case .lan:
      lanPriority
    }
  }

  fileprivate var canonicalRealtimeGRPCBaseURL: URL {
    guard port == MobileRouteURLPolicy.defaultHTTPAPIPort,
      scheme?.lowercased() == "http",
      allowsRealtimePortRepair,
      var components = URLComponents(url: self, resolvingAgainstBaseURL: false)
    else {
      return self
    }

    components.port = MobileRouteURLPolicy.defaultRealtimeGRPCPort
    return components.url ?? self
  }

  fileprivate var canonicalHTTPAPIBaseURL: URL {
    guard port == MobileRouteURLPolicy.defaultRealtimeGRPCPort,
      scheme?.lowercased() == "http",
      allowsRealtimePortRepair,
      var components = URLComponents(url: self, resolvingAgainstBaseURL: false)
    else {
      return self
    }

    components.port = MobileRouteURLPolicy.defaultHTTPAPIPort
    return components.url ?? self
  }

  private var allowsRealtimePortRepair: Bool {
    switch mobileRoute {
    case .tailscale, .lan, .loopback:
      return true
    case .remote:
      return false
    }
  }

  private var mobileRoute: MobileRoute {
    guard !isLoopbackHost else {
      return .loopback
    }

    guard let host = normalizedHost else {
      return .remote
    }

    if isTailscaleHost {
      return .tailscale
    }

    if host.hasSuffix(LANNetwork.localHostnameSuffix) || isPrivateLANHost(host)
      || isLinkLocalHost(host)
    {
      return .lan
    }

    return .remote
  }

  private var tailscalePriority: Int {
    switch mobileRoute {
    case .tailscale:
      RoutePriority.first
    case .lan:
      RoutePriority.second
    case .remote:
      RoutePriority.third
    case .loopback:
      RoutePriority.fourth
    }
  }

  private var lanPriority: Int {
    switch mobileRoute {
    case .lan:
      RoutePriority.first
    case .tailscale:
      RoutePriority.second
    case .remote:
      RoutePriority.third
    case .loopback:
      RoutePriority.fourth
    }
  }

  private var normalizedHost: String? {
    host?
      .lowercased()
      .trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
  }

  private func isPrivateLANHost(_ host: String) -> Bool {
    guard let octets = ipv4Octets(from: host) else {
      return false
    }

    return octets[0] == LANNetwork.privateTenFirstOctet
      || (octets[0] == LANNetwork.privateOneSevenTwoFirstOctet
        && LANNetwork.privateOneSevenTwoSecondOctetRange.contains(octets[1]))
      || (octets[0] == LANNetwork.privateOneNineTwoFirstOctet
        && octets[1] == LANNetwork.privateOneNineTwoSecondOctet)
  }

  private func isLinkLocalHost(_ host: String) -> Bool {
    guard let octets = ipv4Octets(from: host) else {
      return false
    }

    return octets[0] == LANNetwork.linkLocalFirstOctet
      && octets[1] == LANNetwork.linkLocalSecondOctet
  }

  private func ipv4Octets(from host: String) -> [Int]? {
    let octets =
      host
      .split(separator: ".", omittingEmptySubsequences: false)
      .compactMap { Int($0) }

    guard octets.count == LANNetwork.ipv4OctetCount,
      octets.allSatisfy({ LANNetwork.ipv4OctetRange.contains($0) })
    else {
      return nil
    }

    return octets
  }
}

public struct DesktopThreadSummary: Codable, Equatable, Sendable {
  public let threadId: String
  public let title: String?
  public let cwd: String?
  public let transcriptPath: String?
  public let source: String?
  public let originator: String?
  public let model: String?
  public let reasoningEffort: String?
  public let createdAtMs: Int64?
  public let updatedAtMs: Int64?
  public let latestMessageAtMs: Int64?
  public let assistantPreview: String?
  public let archived: Bool
  public let capabilities: ThreadCapabilitiesSummary

  public init(
    threadId: String,
    title: String?,
    cwd: String?,
    transcriptPath: String?,
    source: String?,
    originator: String? = nil,
    model: String?,
    reasoningEffort: String?,
    createdAtMs: Int64? = nil,
    updatedAtMs: Int64?,
    latestMessageAtMs: Int64? = nil,
    assistantPreview: String?,
    archived: Bool,
    capabilities: ThreadCapabilitiesSummary
  ) {
    self.threadId = threadId
    self.title = title
    self.cwd = cwd
    self.transcriptPath = transcriptPath
    self.source = source
    self.originator = originator
    self.model = model
    self.reasoningEffort = reasoningEffort
    self.createdAtMs = createdAtMs
    self.updatedAtMs = updatedAtMs
    self.latestMessageAtMs = latestMessageAtMs
    self.assistantPreview = assistantPreview
    self.archived = archived
    self.capabilities = capabilities
  }

  enum CodingKeys: String, CodingKey {
    case threadId = "thread_id"
    case title
    case cwd
    case transcriptPath = "transcript_path"
    case source
    case originator
    case model
    case reasoningEffort = "reasoning_effort"
    case createdAtMs = "created_at_ms"
    case updatedAtMs = "updated_at_ms"
    case latestMessageAtMs = "latest_message_at_ms"
    case assistantPreview = "assistant_preview"
    case archived
    case capabilities
  }
}

public struct ThreadCapabilitiesSummary: Codable, Equatable, Sendable {
  public let threadId: String
  public let assistantKind: String?
  public let mcpTools: [String]
  public let appTools: [String]
  public let automationTools: [String]
  public let spawn: SpawnGraphSummary
  public let agentNickname: String?
  public let agentRole: String?
  public let agentPath: String?

  public init(
    threadId: String,
    assistantKind: String? = nil,
    mcpTools: [String],
    appTools: [String],
    automationTools: [String],
    spawn: SpawnGraphSummary,
    agentNickname: String?,
    agentRole: String?,
    agentPath: String?
  ) {
    self.threadId = threadId
    self.assistantKind = assistantKind
    self.mcpTools = mcpTools
    self.appTools = appTools
    self.automationTools = automationTools
    self.spawn = spawn
    self.agentNickname = agentNickname
    self.agentRole = agentRole
    self.agentPath = agentPath
  }

  enum CodingKeys: String, CodingKey {
    case threadId = "thread_id"
    case assistantKind = "assistant_kind"
    case mcpTools = "mcp_tools"
    case appTools = "app_tools"
    case automationTools = "automation_tools"
    case spawn
    case agentNickname = "agent_nickname"
    case agentRole = "agent_role"
    case agentPath = "agent_path"
  }
}

public struct SpawnGraphSummary: Codable, Equatable, Sendable {
  public let parentThreadId: String?
  public let rootThreadId: String
  public let children: [String]
  public let launchKind: String

  enum CodingKeys: String, CodingKey {
    case parentThreadId = "parent_thread_id"
    case rootThreadId = "root_thread_id"
    case children
    case launchKind = "launch_kind"
  }
}

public struct DesktopAutomationSummary: Codable, Equatable, Sendable {
  public let id: String
  public let kind: String
  public let name: String
  public let status: String
  public let rrule: String
  public let scheduleSummary: String
  public let targetThreadId: String?
  public let targetKnown: Bool
  public let controlPlaneCovered: Bool

  enum CodingKeys: String, CodingKey {
    case id
    case kind
    case name
    case status
    case rrule
    case scheduleSummary = "schedule_summary"
    case targetThreadId = "target_thread_id"
    case targetKnown = "target_known"
    case controlPlaneCovered = "control_plane_covered"
  }
}

public struct CompactionEventSummary: Codable, Equatable, Sendable {
  public let eventId: String
  public let eventType: String
  public let threadId: String
  public let occurredAt: String

  enum CodingKeys: String, CodingKey {
    case eventId = "event_id"
    case eventType = "event_type"
    case threadId = "thread_id"
    case occurredAt = "occurred_at"
  }
}

public struct GoalSummary: Codable, Equatable, Sendable {
  public let id: String
  public let title: String
  public let status: String
  public let lifecycle: String
  public let running: Bool
  public let priority: String?
  public let targetThreadId: String?
  public let targetKnown: Bool
  public let sourceKind: String
  public let sourcePath: String
  public let updatedAtMs: Int64?
  public let contentHash: String
  public let syncSafe: Bool

  enum CodingKeys: String, CodingKey {
    case id
    case title
    case status
    case lifecycle
    case running
    case priority
    case targetThreadId = "target_thread_id"
    case targetKnown = "target_known"
    case sourceKind = "source_kind"
    case sourcePath = "source_path"
    case updatedAtMs = "updated_at_ms"
    case contentHash = "content_hash"
    case syncSafe = "sync_safe"
  }

  public init(from decoder: Decoder) throws {
    let container = try decoder.container(keyedBy: CodingKeys.self)
    id = try container.decode(String.self, forKey: .id)
    title = try container.decode(String.self, forKey: .title)
    status = try container.decode(String.self, forKey: .status)
    lifecycle = try container.decode(String.self, forKey: .lifecycle)
    running =
      try container.decodeIfPresent(Bool.self, forKey: .running)
      ?? (status == "pursuing" || lifecycle == "pursuing")
    priority = try container.decodeIfPresent(String.self, forKey: .priority)
    targetThreadId = try container.decodeIfPresent(String.self, forKey: .targetThreadId)
    targetKnown = try container.decode(Bool.self, forKey: .targetKnown)
    sourceKind = try container.decode(String.self, forKey: .sourceKind)
    sourcePath = try container.decode(String.self, forKey: .sourcePath)
    updatedAtMs = try container.decodeIfPresent(Int64.self, forKey: .updatedAtMs)
    contentHash = try container.decode(String.self, forKey: .contentHash)
    syncSafe = try container.decode(Bool.self, forKey: .syncSafe)
  }
}

import Foundation

public enum HookRepairTarget: String, CaseIterable, Equatable, Sendable {
    case codex
    case grok = "grok-build"
    case claude = "claude-code"

    public var pathComponent: String {
        switch self {
        case .codex:
            "codex"
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
        case .grok:
            "Grok Build"
        case .claude:
            "Claude Code"
        }
    }
}

public enum LooperLifecycleDefaults {
    private enum Time {
        static let secondsPerMinute: TimeInterval = 60
        static let minutesPerHour: TimeInterval = 60
        static let hoursPerDay: TimeInterval = 24
    }

    public static let requestTimeoutSeconds: TimeInterval = 2
    public static let desktopSnapshotRequestTimeoutSeconds: TimeInterval = 6
    public static let desktopEventStreamRequestTimeoutSeconds: TimeInterval =
        Time.hoursPerDay * Time.minutesPerHour * Time.secondsPerMinute
    public static let quitCleanupTimeoutSeconds: TimeInterval = 2
}

public enum ControlPlaneEndpoint: Equatable {
    case registerHooks
    case registerTargetHooks(HookRepairTarget)
    case unregisterLiveHooks
    case unregisterLiveTargetHooks(HookRepairTarget)
    case shutdown
    case mobileHealth
    case controlPlaneStatus
    case desktopSnapshot
    case desktopEvents
    case devinAcpBridgeProbe
    case devinAcpBridgeInstall

    public var path: String {
        switch self {
        case .registerHooks:
            "/hooks/register"
        case let .registerTargetHooks(target):
            "/hooks/\(target.pathComponent)/register"
        case .unregisterLiveHooks:
            "/hooks/unregister-live"
        case let .unregisterLiveTargetHooks(target):
            "/hooks/\(target.pathComponent)/unregister-live"
        case .shutdown:
            "/desktop/shutdown"
        case .mobileHealth:
            "/api/mobile/health"
        case .controlPlaneStatus:
            "/status/control-plane"
        case .desktopSnapshot:
            "/desktop/snapshot"
        case .desktopEvents:
            "/desktop/events"
        case .devinAcpBridgeProbe:
            "/desktop/devin/acp-bridge/probe"
        case .devinAcpBridgeInstall:
            "/desktop/devin/acp-bridge/install"
        }
    }

    public var method: String {
        switch self {
        case .registerHooks, .registerTargetHooks, .unregisterLiveHooks, .unregisterLiveTargetHooks, .shutdown,
             .devinAcpBridgeProbe, .devinAcpBridgeInstall:
            "POST"
        case .controlPlaneStatus, .desktopSnapshot, .desktopEvents, .mobileHealth:
            "GET"
        }
    }

    public var queryItems: [URLQueryItem] {
        switch self {
        case .desktopSnapshot:
            [URLQueryItem(name: "profile", value: "menu")]
        case .registerHooks, .registerTargetHooks, .unregisterLiveHooks, .unregisterLiveTargetHooks, .shutdown,
             .mobileHealth, .controlPlaneStatus, .desktopEvents, .devinAcpBridgeProbe, .devinAcpBridgeInstall:
            []
        }
    }

    public var timeoutInterval: TimeInterval {
        switch self {
        case .desktopSnapshot:
            LooperLifecycleDefaults.desktopSnapshotRequestTimeoutSeconds
        case .desktopEvents:
            LooperLifecycleDefaults.desktopEventStreamRequestTimeoutSeconds
        case .registerHooks, .registerTargetHooks, .unregisterLiveHooks, .unregisterLiveTargetHooks, .shutdown,
             .mobileHealth, .controlPlaneStatus, .devinAcpBridgeProbe, .devinAcpBridgeInstall:
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

    public func fetchMobileHealth() async throws -> MobileHealthResponse {
        try await fetchJSON(MobileHealthResponse.self, from: .mobileHealth)
    }

    public func probeDevinAcpBridge(agentId: String? = nil) async throws -> DevinAcpBridgeProbeResponse {
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

    public func unregisterLiveHooks(
        target: HookRepairTarget,
        timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds
    ) throws {
        try runBlockingRequest(for: .unregisterLiveTargetHooks(target), timeout: timeout)
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
    public let runtimes: [AssistantRuntimeSummary]
    public let detail: String

    enum CodingKeys: String, CodingKey {
        case assistantKind = "assistant_kind"
        case runtimes
        case detail
    }
}

public struct AssistantRuntimeSummary: Codable, Equatable, Sendable {
    public let kind: String
    public let running: Bool
    public let installed: Bool
    public let label: String
    public let executable: String?
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

    enum CodingKeys: String, CodingKey {
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

public struct DevinDesktopStatus: Codable, Equatable, Sendable {
    public let acpBridge: DevinAcpBridgeStatus

    enum CodingKeys: String, CodingKey {
        case acpBridge = "acp_bridge"
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

    public var preferredHandoffBaseURL: URL? {
        preferredReachableHandoffBaseURL
            ?? URL(string: baseURL)
    }

    public var preferredReachableHandoffBaseURL: URL? {
        baseURLs
            .compactMap(URL.init(string:))
            .first(where: { !$0.isLoopbackHost })
    }

    public var supportsNativeHandoff: Bool {
        ok && requiresAuthentication && preferredReachableHandoffBaseURL != nil
    }

    public var preferredRealtimeBaseURLs: [URL] {
        var seen = Set<String>()
        return ([grpcBaseURL] + grpcBaseURLs)
            .compactMap(URL.init(string:))
            .filter { url in
                !url.absoluteString.isEmpty && seen.insert(url.absoluteString).inserted
            }
    }

    enum CodingKeys: String, CodingKey {
        case ok
        case baseURL
        case baseURLs
        case grpcBaseURL
        case grpcBaseURLs
        case requiresAuthentication
    }

    public init(
        ok: Bool,
        baseURL: String,
        baseURLs: [String],
        grpcBaseURL: String = "",
        grpcBaseURLs: [String] = [],
        requiresAuthentication: Bool
    ) {
        self.ok = ok
        self.baseURL = baseURL
        self.baseURLs = baseURLs
        self.grpcBaseURL = grpcBaseURL
        self.grpcBaseURLs = grpcBaseURLs
        self.requiresAuthentication = requiresAuthentication
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        ok = try container.decodeIfPresent(Bool.self, forKey: .ok) ?? false
        baseURL = try container.decodeIfPresent(String.self, forKey: .baseURL) ?? ""
        baseURLs = try container.decodeIfPresent([String].self, forKey: .baseURLs) ?? []
        grpcBaseURL = try container.decodeIfPresent(String.self, forKey: .grpcBaseURL) ?? ""
        grpcBaseURLs = try container.decodeIfPresent([String].self, forKey: .grpcBaseURLs) ?? []
        requiresAuthentication = try container.decodeIfPresent(Bool.self, forKey: .requiresAuthentication) ?? true
    }
}

private extension URL {
    var isLoopbackHost: Bool {
        guard let host else {
            return false
        }
        return host == "127.0.0.1" || host == "localhost" || host == "::1"
    }
}

public struct DesktopThreadSummary: Codable, Equatable, Sendable {
    public let threadId: String
    public let title: String?
    public let cwd: String?
    public let transcriptPath: String?
    public let source: String?
    public let model: String?
    public let reasoningEffort: String?
    public let updatedAtMs: Int64?
    public let assistantPreview: String?
    public let archived: Bool
    public let capabilities: ThreadCapabilitiesSummary

    enum CodingKeys: String, CodingKey {
        case threadId = "thread_id"
        case title
        case cwd
        case transcriptPath = "transcript_path"
        case source
        case model
        case reasoningEffort = "reasoning_effort"
        case updatedAtMs = "updated_at_ms"
        case assistantPreview = "assistant_preview"
        case archived
        case capabilities
    }
}

public struct ThreadCapabilitiesSummary: Codable, Equatable, Sendable {
    public let threadId: String
    public let mcpTools: [String]
    public let appTools: [String]
    public let automationTools: [String]
    public let spawn: SpawnGraphSummary
    public let agentNickname: String?
    public let agentRole: String?
    public let agentPath: String?

    enum CodingKeys: String, CodingKey {
        case threadId = "thread_id"
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
        running = try container.decodeIfPresent(Bool.self, forKey: .running) ??
            (status == "pursuing" || lifecycle == "pursuing")
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

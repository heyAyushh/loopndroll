import Foundation

public enum ControlPlaneServiceError: Error, Equatable {
    case missingBundledExecutable(String)
    case failedToBecomeHealthy
}

public protocol ControlPlaneService: Sendable {
    func startIfNeeded() async throws
    func stop()
}

public struct NoopControlPlaneService: ControlPlaneService {
    public init() {}

    public func startIfNeeded() async throws {}

    public func stop() {}
}

public final class BundledControlPlaneService: ControlPlaneService, @unchecked Sendable {
    public static let defaultHealthURL = ControlPlaneEndpointStore.defaultBaseURL
        .appendingPathComponent(healthPath)

    private static let listenEnvironmentKey = "AGENT_CONTROL_PLANE_LISTEN"
    private static let bundledExecutableName = "looper-server"
    private static let loopbackHost = "127.0.0.1"
    private static let networkReachableHost = "0.0.0.0"
    private static let unspecifiedIPv4Host = "0.0.0.0"
    private static let unspecifiedIPv6Host = "::"
    private static let IPv6LoopbackHost = "::1"
    private static let healthPath = "health"
    private static let defaultListenPort: UInt16 = 8765
    private static let fallbackPortCount: UInt16 = 16
    private static let blockedInheritedEnvironmentPrefixes = ["CODEX_SANDBOX_"]
    private static let expectedServiceName = "looper"
    private static let expectedHealthOwner = "looper-rust"
    private static let healthServiceKey = "service"
    private static let healthOwnerKey = "owner"
    private static let healthHooksKey = "hooks"
    private static let healthPollCount = 30
    private static let healthPollDelayNanoseconds: UInt64 = 100_000_000
    private static let healthRequestTimeoutSeconds: TimeInterval = 0.35

    private let executableURL: URL?
    private let endpointStore: ControlPlaneEndpointStore
    private let environment: [String: String]
    private let session: URLSession
    private let lock = NSLock()
    private var process: Process?

    public init(
        executableURL: URL? = BundledControlPlaneService.defaultExecutableURL(),
        endpointStore: ControlPlaneEndpointStore = ControlPlaneEndpointStore(),
        environment: [String: String] = ProcessInfo.processInfo.environment,
        session: URLSession = .shared
    ) {
        self.executableURL = executableURL
        self.endpointStore = endpointStore
        self.environment = environment
        self.session = session
    }

    public func startIfNeeded() async throws {
        let candidates = Self.listenCandidates(environment: environment)

        for candidate in candidates where await isHealthy(at: candidate.healthURL) {
            endpointStore.baseURL = candidate.baseURL
            return
        }

        guard let executableURL else {
            throw ControlPlaneServiceError.missingBundledExecutable(Self.bundledExecutableName)
        }

        var lastError: Error = ControlPlaneServiceError.failedToBecomeHealthy
        for candidate in candidates {
            do {
                let process = try startProcess(executableURL: executableURL, candidate: candidate)
                try await waitUntilHealthy(at: candidate.healthURL, process: process)
                endpointStore.baseURL = candidate.baseURL
                return
            } catch {
                lastError = error
                stop()
            }
        }

        throw lastError
    }

    public func stop() {
        let runningProcess = lock.withLock {
            let runningProcess = process
            process = nil
            return runningProcess
        }
        if runningProcess?.isRunning == true {
            runningProcess?.terminate()
        }
    }

    public static func defaultExecutableURL(bundle: Bundle = .main) -> URL? {
        bundle.executableURL?
            .deletingLastPathComponent()
            .appendingPathComponent(bundledExecutableName)
    }

    static func listenCandidates(environment: [String: String]) -> [ControlPlaneListenCandidate] {
        if let configured = environment[listenEnvironmentKey],
           let candidate = ControlPlaneListenCandidate(listenAddress: configured)
        {
            return [candidate]
        }

        return fallbackListenPorts.compactMap { port in
            ControlPlaneListenCandidate(listenAddress: "\(networkReachableHost):\(port)")
        }
    }

    private static var fallbackListenPorts: [UInt16] {
        (0..<fallbackPortCount).map { defaultListenPort + $0 }
    }

    static func sanitizedLaunchEnvironment(_ environment: [String: String]) -> [String: String] {
        environment.filter { key, _ in
            !blockedInheritedEnvironmentPrefixes.contains { prefix in
                key.hasPrefix(prefix)
            }
        }
    }

    static func isLooperHealthResponse(data: Data, response: URLResponse?) -> Bool {
        guard let response = response as? HTTPURLResponse,
              (200..<300).contains(response.statusCode),
              let json = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return false
        }

        if json[healthServiceKey] as? String == expectedServiceName {
            return true
        }

        let hooks = json[healthHooksKey] as? [String: Any]
        return hooks?[healthOwnerKey] as? String == expectedHealthOwner
    }

    private func startProcess(
        executableURL: URL,
        candidate: ControlPlaneListenCandidate
    ) throws -> Process {
        let process = Process()
        process.executableURL = executableURL
        process.arguments = ["serve"]
        process.environment = Self.sanitizedLaunchEnvironment(environment).merging(
            [Self.listenEnvironmentKey: candidate.listenAddress],
            uniquingKeysWith: { _, candidateValue in candidateValue }
        )
        try process.run()
        lock.withLock {
            self.process = process
        }
        return process
    }

    private func waitUntilHealthy(at healthURL: URL, process: Process) async throws {
        for _ in 0..<Self.healthPollCount {
            if await isHealthy(at: healthURL) {
                return
            }
            if !process.isRunning {
                throw ControlPlaneServiceError.failedToBecomeHealthy
            }
            try await Task.sleep(nanoseconds: Self.healthPollDelayNanoseconds)
        }
        throw ControlPlaneServiceError.failedToBecomeHealthy
    }

    private func isHealthy(at healthURL: URL) async -> Bool {
        var request = URLRequest(url: healthURL)
        request.timeoutInterval = Self.healthRequestTimeoutSeconds
        do {
            let (data, response) = try await session.data(for: request)
            return Self.isLooperHealthResponse(data: data, response: response)
        } catch {
            return false
        }
    }

    struct ControlPlaneListenCandidate: Equatable, Sendable {
        let listenAddress: String
        let baseURL: URL

        var healthURL: URL {
            baseURL.appendingPathComponent(healthPath)
        }

        init?(listenAddress: String) {
            let trimmedAddress = listenAddress.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmedAddress.isEmpty,
                  let baseURL = Self.baseURL(for: trimmedAddress)
            else {
                return nil
            }

            self.listenAddress = trimmedAddress
            self.baseURL = baseURL
        }

        private static func baseURL(for listenAddress: String) -> URL? {
            guard let components = URLComponents(string: "tcp://\(listenAddress)"),
                  let host = components.host,
                  let port = components.port
            else {
                return nil
            }

            let clientHost: String
            switch host {
            case unspecifiedIPv4Host:
                clientHost = loopbackHost
            case unspecifiedIPv6Host:
                clientHost = "[\(IPv6LoopbackHost)]"
            case let host where host.contains(":"):
                clientHost = "[\(host)]"
            default:
                clientHost = host
            }

            return URL(string: "http://\(clientHost):\(port)")
        }
    }
}

public struct LooperLifecycleCoordinator: Sendable {
    private let client: ControlPlaneClient
    private let service: ControlPlaneService

    public init(
        client: ControlPlaneClient,
        service: ControlPlaneService = NoopControlPlaneService()
    ) {
        self.client = client
        self.service = service
    }

    @discardableResult
    public func registerOnLaunch() async -> Result<Void, Error> {
        do {
            try await service.startIfNeeded()
            try await client.registerHooks()
            return .success(())
        } catch {
            return .failure(error)
        }
    }

    @discardableResult
    public func unregisterBeforeQuit(
        timeout: TimeInterval = LooperLifecycleDefaults.quitCleanupTimeoutSeconds,
        stopService: Bool = true
    ) -> Result<Void, Error> {
        defer {
            if stopService {
                service.stop()
            }
        }

        var firstError: Error?
        do {
            try client.unregisterLiveHooks(timeout: timeout)
        } catch {
            firstError = error
        }

        if stopService {
            do {
                try client.shutdownServer(timeout: timeout)
            } catch {
                firstError = firstError ?? error
            }
        }

        if let firstError {
            return .failure(firstError)
        }
        return .success(())
    }

    public func stopService() {
        service.stop()
    }

    public func shutdownServer(timeout: TimeInterval = LooperLifecycleDefaults.requestTimeoutSeconds) {
        try? client.shutdownServer(timeout: timeout)
        service.stop()
    }
}

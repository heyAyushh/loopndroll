import Foundation
import LooperCompanionCore
import LooperRealtime

enum RealtimeCompanionClientFactory {
    private static let healthPath = "/api/mobile/health"
    fileprivate static let requestTimeout: TimeInterval = 1.5
    fileprivate static let resourceTimeout: TimeInterval = 2
    private static let connectionManager = RealtimeCompanionConnectionManager()

    static func prepareClient(baseURLs: [URL], bearerToken: String?) async {
        guard let client = await makeClient(baseURLs: baseURLs, bearerToken: bearerToken) else {
            return
        }

        do {
            try await client.warmConnections()
            CompanionDiagnostics.record("realtime:warm-success")
        } catch {
            CompanionDiagnostics.record("realtime:warm-failed error=\(error.localizedDescription)")
        }
    }

    static func makeClient(baseURLs: [URL], bearerToken: String?) async -> LooperRealtimeClient? {
        let mobileSessionHeader = CompanionMobileSessionStore.loadValidHeaderValue()
        let endpoints = await connectionManager.endpoints(baseURLs: baseURLs)
        guard !endpoints.isEmpty else {
            return nil
        }

        return await connectionManager.client(
            endpoints: endpoints,
            credentials: LooperRealtimeCredentials(
                bearerToken: bearerToken,
                mobileSessionHeader: mobileSessionHeader
            )
        )
    }

    static func disconnectCachedClients() async {
        await connectionManager.disconnectClients()
    }

    static func invalidateCachedConnections() async {
        await connectionManager.invalidate()
    }

    fileprivate static func discoverRealtimeEndpoints(baseURLs: [URL]) async -> [LooperRealtimeEndpoint] {
        var seen = Set<String>()
        var endpoints: [LooperRealtimeEndpoint] = []

        let discoveredHealth = await discoverRealtimeHealth(baseURLs: baseURLs)
        for health in discoveredHealth {
            let grpcBaseURLs = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(
                health.grpcBaseURLsForConnection.compactMap(URL.init(string:))
            )
            for grpcBaseURL in grpcBaseURLs {
                guard seen.insert(grpcBaseURL.absoluteString).inserted else {
                    continue
                }
                endpoints.append(LooperRealtimeEndpoint(baseURL: grpcBaseURL))
            }
        }

        return endpoints
    }

    private static func discoverRealtimeHealth(baseURLs: [URL]) async -> [RealtimeHealthDiscovery] {
        let candidates = CompanionBaseURLRacePlan.candidates(for: baseURLs)

        return await withTaskGroup(
            of: RealtimeHealthDiscoveryResult?.self,
            returning: [RealtimeHealthDiscovery].self
        ) { group in
            for candidate in candidates {
                group.addTask {
                    do {
                        if candidate.delay != .zero {
                            try await Task.sleep(for: candidate.delay)
                        }
                        try Task.checkCancellation()
                    } catch {
                        return nil
                    }

                    guard let health = try? await loadHealth(baseURL: candidate.baseURL) else {
                        return nil
                    }
                    return RealtimeHealthDiscoveryResult(health: health)
                }
            }

            var firstResult: RealtimeHealthDiscoveryResult?
            for await result in group {
                guard let result else {
                    continue
                }
                firstResult = result
                group.cancelAll()
                break
            }

            return firstResult.map { [$0.health] } ?? []
        }
    }

    private static func loadHealth(baseURL: URL) async throws -> RealtimeHealthDiscovery {
        var request = URLRequest(url: baseURL.appending(path: healthPath))
        request.httpMethod = "GET"
        request.timeoutInterval = requestTimeout

        let (data, response) = try await RealtimeDiscoveryURLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse,
              (200 ..< 300).contains(httpResponse.statusCode)
        else {
            throw RealtimeDiscoveryError.invalidResponse
        }

        return try JSONDecoder().decode(RealtimeHealthDiscovery.self, from: data)
    }
}

private actor RealtimeCompanionConnectionManager {
    private let endpointCacheTimeToLive: TimeInterval = 300

    private var endpointCache: [RealtimeEndpointCacheKey: RealtimeEndpointCacheEntry] = [:]
    private var clientCache: [RealtimeClientCacheKey: LooperRealtimeClient] = [:]

    func endpoints(baseURLs: [URL]) async -> [LooperRealtimeEndpoint] {
        let candidateBaseURLs = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(baseURLs)
        let key = RealtimeEndpointCacheKey(baseURLs: candidateBaseURLs)
        let now = Date()
        if let cachedEntry = endpointCache[key],
           now.timeIntervalSince(cachedEntry.createdAt) < endpointCacheTimeToLive
        {
            return cachedEntry.endpoints
        }

        let endpoints = await RealtimeCompanionClientFactory.discoverRealtimeEndpoints(
            baseURLs: candidateBaseURLs
        )
        endpointCache[key] = RealtimeEndpointCacheEntry(endpoints: endpoints, createdAt: now)
        return endpoints
    }

    func client(
        endpoints: [LooperRealtimeEndpoint],
        credentials: LooperRealtimeCredentials
    ) -> LooperRealtimeClient {
        let key = RealtimeClientCacheKey(endpoints: endpoints, credentials: credentials)
        if let cachedClient = clientCache[key] {
            return cachedClient
        }

        let client = LooperRealtimeClient(endpoints: endpoints, credentials: credentials)
        clientCache[key] = client
        return client
    }

    func disconnectClients() {
        let clients = Array(clientCache.values)
        clientCache.removeAll(keepingCapacity: true)
        for client in clients {
            client.disconnect()
        }
    }

    func invalidate() {
        endpointCache.removeAll(keepingCapacity: true)
        disconnectClients()
    }
}

private struct RealtimeEndpointCacheKey: Hashable, Sendable {
    let baseURLStrings: [String]

    init(baseURLs: [URL]) {
        baseURLStrings = baseURLs.map(\.absoluteString)
    }
}

private struct RealtimeEndpointCacheEntry: Sendable {
    let endpoints: [LooperRealtimeEndpoint]
    let createdAt: Date
}

private struct RealtimeClientCacheKey: Hashable, Sendable {
    let endpointURLs: [String]
    let bearerToken: String?
    let mobileSessionHeader: String?

    init(endpoints: [LooperRealtimeEndpoint], credentials: LooperRealtimeCredentials) {
        endpointURLs = endpoints.map(\.baseURL.absoluteString)
        bearerToken = credentials.bearerToken
        mobileSessionHeader = credentials.mobileSessionHeader
    }
}

private struct RealtimeHealthDiscoveryResult: Sendable {
    let health: RealtimeHealthDiscovery
}

private enum RealtimeDiscoveryURLSession {
    static let shared: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = false
        configuration.timeoutIntervalForRequest = RealtimeCompanionClientFactory.requestTimeout
        configuration.timeoutIntervalForResource = RealtimeCompanionClientFactory.resourceTimeout
        return URLSession(configuration: configuration)
    }()
}

private struct RealtimeHealthDiscovery: Decodable, Sendable {
    let grpcBaseURL: String?
    let grpcBaseURLs: [String]

    var grpcBaseURLsForConnection: [String] {
        var seen = Set<String>()
        return ([grpcBaseURL].compactMap { $0 } + grpcBaseURLs).filter { url in
            !url.isEmpty && seen.insert(url).inserted
        }
    }

    private enum CodingKeys: String, CodingKey {
        case grpcBaseURL
        case grpcBaseURLs
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        grpcBaseURL = try container.decodeIfPresent(String.self, forKey: .grpcBaseURL)
        grpcBaseURLs = try container.decodeIfPresent([String].self, forKey: .grpcBaseURLs) ?? []
    }
}

private enum RealtimeDiscoveryError: Error {
    case invalidResponse
}

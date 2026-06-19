import Foundation
import LooperCompanionCore
import LooperRealtime

enum RealtimeCompanionClientFactory {
    private static let healthPath = "/api/mobile/health"
    private static let requestTimeout: TimeInterval = 4
    private static let connectionManager = RealtimeCompanionConnectionManager()

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
            for grpcBaseURL in health.grpcBaseURLsForConnection.compactMap(URL.init(string:)) {
                guard seen.insert(grpcBaseURL.absoluteString).inserted else {
                    continue
                }
                endpoints.append(LooperRealtimeEndpoint(baseURL: grpcBaseURL))
            }
        }

        return endpoints
    }

    private static func discoverRealtimeHealth(baseURLs: [URL]) async -> [RealtimeHealthDiscovery] {
        let candidateBaseURLs = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(baseURLs)

        return await withTaskGroup(
            of: RealtimeHealthDiscoveryResult?.self,
            returning: [RealtimeHealthDiscovery].self
        ) { group in
            for (index, baseURL) in candidateBaseURLs.enumerated() {
                group.addTask {
                    guard let health = try? await loadHealth(baseURL: baseURL) else {
                        return nil
                    }
                    return RealtimeHealthDiscoveryResult(index: index, health: health)
                }
            }

            var results: [RealtimeHealthDiscoveryResult] = []
            for await result in group {
                guard let result else {
                    continue
                }
                results.append(result)
            }

            return results
                .sorted { $0.index < $1.index }
                .map(\.health)
        }
    }

    private static func loadHealth(baseURL: URL) async throws -> RealtimeHealthDiscovery {
        var request = URLRequest(url: baseURL.appending(path: healthPath))
        request.httpMethod = "GET"
        request.timeoutInterval = requestTimeout

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse,
              (200 ..< 300).contains(httpResponse.statusCode)
        else {
            throw RealtimeDiscoveryError.invalidResponse
        }

        return try JSONDecoder().decode(RealtimeHealthDiscovery.self, from: data)
    }
}

private actor RealtimeCompanionConnectionManager {
    private let endpointCacheTimeToLive: TimeInterval = 30

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
    let index: Int
    let health: RealtimeHealthDiscovery
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

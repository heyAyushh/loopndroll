import Foundation
import LooperCompanionCore
import LooperRealtime

enum RealtimeCompanionClientFactory {
    private static let healthPath = "/api/mobile/health"
    private static let requestTimeout: TimeInterval = 4

    static func makeClient(baseURLs: [URL], bearerToken: String?) async -> LooperRealtimeClient? {
        let endpoints = await realtimeEndpoints(baseURLs: baseURLs)
        guard !endpoints.isEmpty else {
            return nil
        }

        return LooperRealtimeClient(
            endpoints: endpoints,
            credentials: LooperRealtimeCredentials(
                bearerToken: bearerToken,
                mobileSessionHeader: CompanionMobileSessionStore.loadValidHeaderValue()
            )
        )
    }

    private static func realtimeEndpoints(baseURLs: [URL]) async -> [LooperRealtimeEndpoint] {
        var seen = Set<String>()
        var endpoints: [LooperRealtimeEndpoint] = []

        for baseURL in CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(baseURLs) {
            guard let health = try? await loadHealth(baseURL: baseURL) else {
                continue
            }

            for grpcBaseURL in health.grpcBaseURLsForConnection.compactMap(URL.init(string:)) {
                guard seen.insert(grpcBaseURL.absoluteString).inserted else {
                    continue
                }
                endpoints.append(LooperRealtimeEndpoint(baseURL: grpcBaseURL))
            }
        }

        return endpoints
    }

    private static func loadHealth(baseURL: URL) async throws -> RealtimeHealthDiscovery {
        var request = URLRequest(url: baseURL.appending(path: healthPath))
        request.httpMethod = "GET"
        request.timeoutInterval = requestTimeout

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse,
              (200..<300).contains(httpResponse.statusCode)
        else {
            throw RealtimeDiscoveryError.invalidResponse
        }

        return try JSONDecoder().decode(RealtimeHealthDiscovery.self, from: data)
    }
}

private struct RealtimeHealthDiscovery: Decodable {
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

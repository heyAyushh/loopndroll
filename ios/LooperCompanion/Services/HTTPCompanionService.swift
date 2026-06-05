import Foundation

struct HTTPCompanionService: CompanionService {
    let baseURLs: [URL]
    let bearerToken: String?

    init(baseURL: URL) {
        self.baseURLs = [baseURL]
        bearerToken = nil
    }

    init(baseURLs: [URL], bearerToken: String? = nil) {
        self.baseURLs = baseURLs
        self.bearerToken = bearerToken
    }

    func loadServerHealth() async throws -> CompanionServerHealth {
        try await request(path: "/api/mobile/health", method: HTTPMethod.get)
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        try await request(path: "/api/mobile/snapshot", method: HTTPMethod.get)
    }

    func loadSessionDetail(id: String) async throws -> SessionDetail {
        try await request(path: "/api/mobile/sessions/\(id)", method: HTTPMethod.get)
    }

    func setSessionMode(id: String, preset: SessionMode?) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/sessions/\(id)/mode",
            method: HTTPMethod.post,
            body: ["preset": preset?.rawValue ?? NSNull()]
        )
    }

    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/sessions/\(id)/archive",
            method: HTTPMethod.post,
            body: ["archived": archived]
        )
    }

    func deleteSession(id: String) async throws -> MobileSnapshot {
        try await request(path: "/api/mobile/sessions/\(id)", method: HTTPMethod.delete)
    }

    func sendSessionPrompt(id: String, prompt: String) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/sessions/\(id)/prompt",
            method: HTTPMethod.post,
            body: ["prompt": prompt]
        )
    }

    func muteSession(id: String) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/sessions/\(id)/mute",
            method: HTTPMethod.post
        )
    }

    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/settings/default-prompt",
            method: HTTPMethod.post,
            body: ["defaultPrompt": prompt]
        )
    }

    func saveAssistantSurface(_ surface: CompanionAssistantSurface) async throws -> GlobalSettings {
        try await request(
            path: "/api/mobile/settings/assistant-surface",
            method: HTTPMethod.post,
            body: ["assistantSurface": surface.rawValue]
        )
    }

    func registerPushDevice(
        _ requestPayload: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        try await request(
            path: "/api/mobile/push/register",
            method: HTTPMethod.post,
            body: [
                "installationId": requestPayload.installationId,
                "deviceToken": requestPayload.deviceToken,
                "bundleId": requestPayload.bundleId,
                "environment": requestPayload.environment.rawValue,
                "deviceName": requestPayload.deviceName ?? NSNull()
            ]
        )
    }

    func sendTestPush(installationID: String) async throws -> RemotePushTestResponse {
        try await request(
            path: "/api/mobile/push/test",
            method: HTTPMethod.post,
            body: ["installationId": installationID]
        )
    }

    func resolveConnectionCode(orbID: String) async throws -> MobileConnectionCodeResponse {
        let normalizedOrbID = orbID.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalizedOrbID.isEmpty,
              let encodedOrbID = normalizedOrbID.addingPercentEncoding(
                withAllowedCharacters: .urlPathAllowed
              )
        else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        return try await request(
            path: "/api/mobile/connection-orbs/\(encodedOrbID)",
            method: HTTPMethod.get
        )
    }

    private func request<Response: Decodable>(
        path: String,
        method: HTTPMethod,
        body: [String: Any]? = nil
    ) async throws -> Response {
        let responseData: Data

        do {
            responseData = try await responseDataWithConfiguredURLs(
                path: path,
                method: method,
                body: body
            )
        } catch {
            let discoveredBaseURLs = await LocalCompanionServiceDiscovery.discoverBaseURLs()
            guard !discoveredBaseURLs.isEmpty else {
                throw error
            }

            responseData = try await responseDataWithResolvedURLs(
                baseURLs + discoveredBaseURLs,
                path: path,
                method: method,
                body: body
            )
        }

        let decoder = JSONDecoder()
        return try decoder.decode(Response.self, from: responseData)
    }

    private func responseDataWithConfiguredURLs(
        path: String,
        method: HTTPMethod,
        body: [String: Any]? = nil
    ) async throws -> Data {
        try await responseDataWithResolvedURLs(baseURLs, path: path, method: method, body: body)
    }

    private func responseDataWithResolvedURLs(
        _ resolvedBaseURLs: [URL],
        path: String,
        method: HTTPMethod,
        body: [String: Any]? = nil
    ) async throws -> Data {
        let candidateBaseURLs = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(resolvedBaseURLs)
        guard !candidateBaseURLs.isEmpty else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        if method == .get {
            return try await firstSuccessfulGetData(baseURLs: candidateBaseURLs, path: path)
        }

        var lastError: Error?

        for baseURL in candidateBaseURLs {
            do {
                return try await responseData(
                    baseURL: baseURL,
                    path: path,
                    method: method,
                    body: body
                )
            } catch {
                lastError = error
            }
        }

        throw lastError ?? HTTPCompanionServiceError.invalidResponse
    }

    private func responseData(
        baseURL: URL,
        path: String,
        method: HTTPMethod,
        body: [String: Any]? = nil
    ) async throws -> Data {
        var request = URLRequest(url: baseURL.appending(path: path))
        request.httpMethod = method.rawValue
        request.timeoutInterval = HTTPRequestTimeout.interval(path: path, method: method)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let hasBearerToken = bearerToken != nil
        let hasPasskeySession = CompanionMobileSessionStore.loadValidHeaderValue() != nil
        if let bearerToken {
            request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        }
        if let passkeySession = CompanionMobileSessionStore.loadValidHeaderValue() {
            request.setValue(passkeySession, forHTTPHeaderField: MobileAPIAuthenticationHeader.passkeySession)
        }

        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }

        CompanionDiagnostics.networking.info(
            "Request starting method=\(method.rawValue, privacy: .public) path=\(path, privacy: .public) baseURL=\(baseURL.absoluteString, privacy: .public) bearer=\(hasBearerToken, privacy: .public) passkeySession=\(hasPasskeySession, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "http:start method=\(method.rawValue) path=\(path) baseURL=\(baseURL.absoluteString) bearer=\(hasBearerToken) passkeySession=\(hasPasskeySession)"
        )

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await URLSession.shared.data(for: request)
        } catch {
            CompanionDiagnostics.networking.error(
                "Request transport failed path=\(path, privacy: .public) error=\(error.localizedDescription, privacy: .public)"
            )
            CompanionDiagnostics.record("http:transport-failed path=\(path) error=\(error.localizedDescription)")
            throw error
        }

        guard let httpResponse = response as? HTTPURLResponse else {
            CompanionDiagnostics.networking.error(
                "Request invalid response path=\(path, privacy: .public)"
            )
            CompanionDiagnostics.record("http:invalid-response path=\(path)")
            throw HTTPCompanionServiceError.invalidResponse
        }

        guard httpResponse.statusCode != HTTPStatus.unauthorized else {
            CompanionDiagnostics.networking.error(
                "Request unauthorized path=\(path, privacy: .public)"
            )
            CompanionDiagnostics.record("http:unauthorized path=\(path)")
            throw unauthorizedError(from: data)
        }

        guard (HTTPStatus.successLowerBound..<HTTPStatus.successUpperBound)
            .contains(httpResponse.statusCode)
        else {
            let serverMessage = String(data: data, encoding: .utf8) ?? "Request failed."
            CompanionDiagnostics.networking.error(
                "Request server error path=\(path, privacy: .public) status=\(httpResponse.statusCode, privacy: .public)"
            )
            CompanionDiagnostics.record(
                "http:server-error path=\(path) status=\(httpResponse.statusCode)"
            )
            throw HTTPCompanionServiceError.serverError(serverMessage)
        }

        CompanionDiagnostics.networking.info(
            "Request succeeded path=\(path, privacy: .public) status=\(httpResponse.statusCode, privacy: .public) bytes=\(data.count, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "http:success path=\(path) status=\(httpResponse.statusCode) bytes=\(data.count)"
        )
        return data
    }

    private func firstSuccessfulGetData(baseURLs: [URL], path: String) async throws -> Data {
        try await withThrowingTaskGroup(of: Data.self) { group in
            for baseURL in baseURLs {
                group.addTask {
                    try await responseData(
                        baseURL: baseURL,
                        path: path,
                        method: HTTPMethod.get
                    )
                }
            }

            var lastError: Error?
            while let result = await group.nextResult() {
                switch result {
                case let .success(response):
                    group.cancelAll()
                    return response
                case let .failure(error):
                    lastError = error
                }
            }

            throw lastError ?? HTTPCompanionServiceError.invalidResponse
        }
    }
}

private enum LocalCompanionServiceDiscovery {
    static let serviceType = "_looper._tcp."
    static let domain = "local."
    static let timeoutSeconds: TimeInterval = 8

    static func discoverBaseURLs() async -> [URL] {
        await BonjourServiceResolver().discoverBaseURLs()
    }
}

private final class BonjourServiceResolver: NSObject, NetServiceBrowserDelegate, NetServiceDelegate {
    private var browser: NetServiceBrowser?
    private var continuation: CheckedContinuation<[URL], Never>?
    private var resolvedBaseURLs: [URL] = []
    private var services: [NetService] = []

    func discoverBaseURLs() async -> [URL] {
        await withCheckedContinuation { continuation in
            self.continuation = continuation
            let browser = NetServiceBrowser()
            self.browser = browser
            browser.delegate = self
            browser.searchForServices(
                ofType: LocalCompanionServiceDiscovery.serviceType,
                inDomain: LocalCompanionServiceDiscovery.domain
            )

            perform(
                #selector(finishAfterTimeout),
                with: nil,
                afterDelay: LocalCompanionServiceDiscovery.timeoutSeconds
            )
        }
    }

    func netServiceBrowser(
        _ browser: NetServiceBrowser,
        didFind service: NetService,
        moreComing: Bool
    ) {
        services.append(service)
        service.delegate = self
        service.resolve(withTimeout: LocalCompanionServiceDiscovery.timeoutSeconds)
    }

    func netServiceDidResolveAddress(_ sender: NetService) {
        guard let url = baseURL(from: sender) else {
            return
        }

        resolvedBaseURLs.append(url)
        finish()
    }

    private func baseURL(from service: NetService) -> URL? {
        guard let hostName = service.hostName, service.port > 0 else {
            return nil
        }

        var components = URLComponents()
        components.scheme = "http"
        components.host = hostName.trimmingCharacters(in: CharacterSet(charactersIn: "."))
        components.port = service.port
        return components.url
    }

    private func finish() {
        guard let continuation else {
            return
        }

        self.continuation = nil
        NSObject.cancelPreviousPerformRequests(
            withTarget: self,
            selector: #selector(finishAfterTimeout),
            object: nil
        )
        browser?.stop()
        services.forEach { service in
            service.stop()
            service.delegate = nil
        }
        continuation.resume(
            returning: CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(resolvedBaseURLs)
        )
    }

    @objc private func finishAfterTimeout() {
        finish()
    }
}

private enum HTTPStatus {
    static let unauthorized = 401
    static let successLowerBound = 200
    static let successUpperBound = 300
}

private enum HTTPRequestTimeout {
    static let health: TimeInterval = 8
    static let sessionDetail: TimeInterval = 12
    static let snapshot: TimeInterval = 6
    static let mutation: TimeInterval = 25
    static let fallback: TimeInterval = 15

    static func interval(path: String, method: HTTPMethod) -> TimeInterval {
        if method != .get {
            return mutation
        }

        if path == "/api/mobile/health" {
            return health
        }

        if path == "/api/mobile/snapshot" {
            return snapshot
        }

        if path.hasPrefix("/api/mobile/sessions/") {
            return sessionDetail
        }

        return fallback
    }
}

private enum MobileAPIAuthenticationHeader {
    static let passkeySession = "X-Looper-Mobile-Session"
}

struct MobileConnectionCodeResponse: Decodable, Sendable {
    let code: String
    let orbID: String

    enum CodingKeys: String, CodingKey {
        case code
        case orbID = "orbId"
    }
}

private enum HTTPMethod: String {
    case delete = "DELETE"
    case get = "GET"
    case post = "POST"
}

enum HTTPCompanionServiceError: LocalizedError {
    case invalidResponse
    case unauthorized
    case passkeySessionRequired(String)
    case serverError(String)

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "The looper API returned an invalid response."
        case .unauthorized:
            return "This iPhone is not paired with the Mac."
        case let .passkeySessionRequired(message):
            return message
        case let .serverError(message):
            return message
        }
    }
}

private struct MobileAPIErrorEnvelope: Decodable {
    let code: String?
    let message: String?
}

private enum MobileAPIErrorCode {
    static let passkeySessionRequired = "passkey_session_required"
}

private enum MobileAPIErrorMessage {
    static let passkeySessionRequired = "Unlock looper with Face ID before using the mobile API."
}

private func unauthorizedError(from data: Data) -> HTTPCompanionServiceError {
    guard
        let envelope = try? JSONDecoder().decode(MobileAPIErrorEnvelope.self, from: data),
        envelope.code == MobileAPIErrorCode.passkeySessionRequired
    else {
        return .unauthorized
    }

    return .passkeySessionRequired(envelope.message ?? MobileAPIErrorMessage.passkeySessionRequired)
}

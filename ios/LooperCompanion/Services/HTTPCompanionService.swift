import Foundation
import LooperClientCore
import LooperCompanionCore

private struct HTTPCompanionResponseData: Sendable {
    let data: Data
    let baseURL: URL
}

// Race events stay internal to the task group so transport failures can be tried against the
// next route without throwing out of the structured-concurrency scope.
private enum HTTPCompanionBaseURLRaceEvent {
    case response(Result<HTTPCompanionResponseData, Error>)
    case fallbackTimer(generation: Int)
}

private struct HTTPCompanionBaseURLRaceCandidate: Equatable, Sendable {
    let baseURL: URL
    let delay: Duration
}

private struct HTTPCompanionBaseURLRaceState {
    private let candidates: [HTTPCompanionBaseURLRaceCandidate]
    private(set) var nextCandidateIndex = 0
    private(set) var inFlightRequestCount = 0
    private(set) var timerGeneration = 0

    init(candidates: [HTTPCompanionBaseURLRaceCandidate]) {
        self.candidates = candidates
    }

    var hasRemainingCandidates: Bool {
        nextCandidateIndex < candidates.count
    }

    var shouldKeepWaiting: Bool {
        inFlightRequestCount > 0 || hasRemainingCandidates
    }

    mutating func nextRequestCandidate() -> HTTPCompanionBaseURLRaceCandidate {
        let candidate = candidates[nextCandidateIndex]
        nextCandidateIndex += 1
        inFlightRequestCount += 1
        return candidate
    }

    mutating func nextFallbackTimer() -> (generation: Int, delay: Duration)? {
        guard hasRemainingCandidates else {
            return nil
        }

        timerGeneration += 1
        return (timerGeneration, candidates[nextCandidateIndex].delay)
    }

    mutating func ignorePendingFallbackTimer() {
        timerGeneration += 1
    }

    mutating func finishFailedRequest() {
        inFlightRequestCount -= 1
    }

    func acceptsFallbackTimer(generation: Int) -> Bool {
        generation == timerGeneration && hasRemainingCandidates
    }
}

struct HTTPCompanionService: CompanionService {
    private static let healthPath = "/api/mobile/health"
    private static let snapshotPath = "/api/mobile/snapshot"
    private static let sessionPathPrefix = "/api/mobile/sessions"
    private static let pathSeparator = "/"
    private static let assistantSurfaceQueryItemName = "assistantSurface"
    private static let pathSegmentReservedCharacters = CharacterSet(charactersIn: "/")
    private static let pathSegmentAllowedCharacters = CharacterSet.urlPathAllowed
        .subtracting(pathSegmentReservedCharacters)

    let baseURLs: [URL]
    let bearerToken: String?

    init(
        baseURL: URL
    ) {
        self.baseURLs = [baseURL]
        self.bearerToken = nil
    }

    init(
        baseURLs: [URL],
        bearerToken: String? = nil
    ) {
        self.baseURLs = baseURLs
        self.bearerToken = bearerToken
    }

    func loadServerHealth() async throws -> CompanionServerHealth {
        try await resolveServerHealth().health
    }

    func resolveServerHealth() async throws -> ResolvedCompanionServerHealth {
        let responseData = try await healthResponseDataWithConfiguredOrDiscoveredURLs()
        let health = try JSONDecoder().decode(CompanionServerHealth.self, from: responseData.data)
        return ResolvedCompanionServerHealth(
            health: health,
            reachedBaseURL: responseData.baseURL
        )
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        try await request(path: Self.snapshotPath, method: HTTPMethod.get)
    }

    func loadSessionDetail(
        id: String,
        surface: CompanionAssistantSurface?
    ) async throws -> SessionDetail {
        try await request(
            path: path(sessionPath(id: id), assistantSurface: surface),
            method: HTTPMethod.get
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
        let responseData = try await responseDataWithConfiguredURLs(path: path, method: method, body: body)
        let decoder = JSONDecoder()
        return try decoder.decode(Response.self, from: responseData.data)
    }

    private func healthResponseDataWithConfiguredOrDiscoveredURLs() async throws -> HTTPCompanionResponseData {
        let discoveredBaseURLsTask = Task { @MainActor in
            await LocalCompanionServiceDiscovery.discoverBaseURLs()
        }

        return try await withTaskCancellationHandler {
            do {
                let responseData = try await responseDataWithConfiguredURLs(
                    path: Self.healthPath,
                    method: .get,
                    includesAuthentication: false
                )
                discoveredBaseURLsTask.cancel()
                _ = await discoveredBaseURLsTask.value
                return responseData
            } catch {
                if isCancellationError(error) {
                    discoveredBaseURLsTask.cancel()
                    _ = await discoveredBaseURLsTask.value
                    throw error
                }

                let discoveredBaseURLs = await discoveredBaseURLsTask.value
                try Task.checkCancellation()
                guard !discoveredBaseURLs.isEmpty else {
                    throw error
                }

                return try await responseDataWithResolvedURLs(
                    CompanionBaseURLSelection.mergedCandidateBaseURLs(
                        configured: baseURLs,
                        discovered: discoveredBaseURLs
                    ),
                    path: Self.healthPath,
                    method: .get,
                    includesAuthentication: false
                )
            }
        } onCancel: {
            discoveredBaseURLsTask.cancel()
        }
    }

    private func responseDataWithConfiguredURLs(
        path: String,
        method: HTTPMethod,
        body: [String: Any]? = nil,
        includesAuthentication: Bool = true
    ) async throws -> HTTPCompanionResponseData {
        try await responseDataWithResolvedURLs(
            baseURLs,
            path: path,
            method: method,
            body: body,
            includesAuthentication: includesAuthentication
        )
    }

    private func responseDataWithResolvedURLs(
        _ resolvedBaseURLs: [URL],
        path: String,
        method: HTTPMethod,
        body: [String: Any]? = nil,
        includesAuthentication: Bool = true
    ) async throws -> HTTPCompanionResponseData {
        let candidateBaseURLs = CompanionConfiguration.uniqueAttemptableBaseURLs(resolvedBaseURLs)
        guard !candidateBaseURLs.isEmpty else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        let prioritizedBaseURLs = await HTTPCompanionRouteCache.shared.prioritizedBaseURLs(
            candidateBaseURLs
        )
        let bodyData = try body.map { requestBody in
            try JSONSerialization.data(withJSONObject: requestBody)
        }

        if shouldRaceResolvedURLs(path: path, method: method) {
            let response = try await firstSuccessfulData(
                candidates: Self.baseURLRaceCandidates(for: prioritizedBaseURLs),
                path: path,
                method: method,
                bodyData: bodyData,
                includesAuthentication: includesAuthentication
            )
            await HTTPCompanionRouteCache.shared.rememberSuccessfulBaseURL(
                response.baseURL,
                for: candidateBaseURLs
            )
            return response
        }

        var lastError: Error?

        for baseURL in prioritizedBaseURLs {
            do {
                let response = try await responseData(
                    baseURL: baseURL,
                    path: path,
                    method: method,
                    bodyData: bodyData,
                    includesAuthentication: includesAuthentication
                )
                await HTTPCompanionRouteCache.shared.rememberSuccessfulBaseURL(
                    response.baseURL,
                    for: candidateBaseURLs
                )
                return response
            } catch {
                await HTTPCompanionRouteCache.shared.forgetFailedBaseURL(
                    baseURL,
                    for: candidateBaseURLs
                )
                lastError = error
            }
        }

        throw lastError ?? HTTPCompanionServiceError.invalidResponse
    }

    private func responseData(
        baseURL: URL,
        path: String,
        method: HTTPMethod,
        bodyData: Data? = nil,
        includesAuthentication: Bool = true
    ) async throws -> HTTPCompanionResponseData {
        var request = URLRequest(url: try requestURL(baseURL: baseURL, path: path))
        request.httpMethod = method.rawValue
        request.timeoutInterval = HTTPRequestTimeout.interval(path: path, method: method)
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let passkeySession = includesAuthentication ? CompanionMobileSessionStore.loadValidHeaderValue() : nil
        let hasBearerToken = includesAuthentication && bearerToken != nil
        let hasPasskeySession = passkeySession != nil
        if includesAuthentication, let bearerToken {
            request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        }
        if let passkeySession {
            request.setValue(passkeySession, forHTTPHeaderField: MobileAPIAuthenticationHeader.passkeySession)
        }

        request.httpBody = bodyData

        CompanionDiagnostics.networking.info(
            "Request starting method=\(method.rawValue, privacy: .public) path=\(path, privacy: .public) baseURL=\(baseURL.absoluteString, privacy: .public) bearer=\(hasBearerToken, privacy: .public) passkeySession=\(hasPasskeySession, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "http:start method=\(method.rawValue) path=\(path) baseURL=\(baseURL.absoluteString) bearer=\(hasBearerToken) passkeySession=\(hasPasskeySession)"
        )

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await HTTPCompanionURLSession.shared.data(for: request)
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
            let serverMessage = serverErrorMessage(from: data)
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
        return HTTPCompanionResponseData(data: data, baseURL: baseURL)
    }

    private func shouldRaceResolvedURLs(path: String, method: HTTPMethod) -> Bool {
        method == .get
    }

    private func sessionPath(id: String, suffix: String? = nil) throws -> String {
        guard let encodedID = id.addingPercentEncoding(
            withAllowedCharacters: Self.pathSegmentAllowedCharacters
        ), !encodedID.isEmpty
        else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        if let suffix = normalizedPathSuffix(suffix) {
            return "\(Self.sessionPathPrefix)\(Self.pathSeparator)\(encodedID)\(Self.pathSeparator)\(suffix)"
        }
        return "\(Self.sessionPathPrefix)\(Self.pathSeparator)\(encodedID)"
    }

    private func requestURL(baseURL: URL, path: String) throws -> URL {
        guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false) else {
            throw HTTPCompanionServiceError.invalidResponse
        }
        let requestComponents = URLComponents(string: path)

        let basePath = components.percentEncodedPath.trimmingCharacters(
            in: Self.pathSegmentReservedCharacters
        )
        let requestPath = (requestComponents?.percentEncodedPath ?? path).trimmingCharacters(
            in: Self.pathSegmentReservedCharacters
        )
        let joinedPath = [basePath, requestPath]
            .filter { !$0.isEmpty }
            .joined(separator: Self.pathSeparator)
        components.percentEncodedPath = "\(Self.pathSeparator)\(joinedPath)"
        components.percentEncodedQuery = requestComponents?.percentEncodedQuery
        guard let url = components.url else {
            throw HTTPCompanionServiceError.invalidResponse
        }
        return url
    }

    private func path(
        _ path: String,
        assistantSurface: CompanionAssistantSurface?
    ) -> String {
        guard let assistantSurface else {
            return path
        }

        var components = URLComponents()
        components.path = path
        components.queryItems = [
            URLQueryItem(
                name: Self.assistantSurfaceQueryItemName,
                value: assistantSurface.rawValue
            )
        ]
        return components.string ?? path
    }

    private func normalizedPathSuffix(_ suffix: String?) -> String? {
        let trimmedSuffix = suffix?.trimmingCharacters(in: Self.pathSegmentReservedCharacters)
        guard let trimmedSuffix, !trimmedSuffix.isEmpty else {
            return nil
        }
        return trimmedSuffix
    }

    private static func baseURLRaceCandidates(
        for baseURLs: [URL]
    ) -> [HTTPCompanionBaseURLRaceCandidate] {
        planBaseUrlRaceCandidates(
            baseUrls: baseURLs.map(\.absoluteString),
            fallbackDelayNanoseconds: defaultBaseUrlRaceFallbackDelayNanoseconds()
        ).compactMap { candidate in
            guard let baseURL = URL(string: candidate.baseUrl) else {
                return nil
            }

            return HTTPCompanionBaseURLRaceCandidate(
                baseURL: baseURL,
                delay: .nanoseconds(Int64(clamping: candidate.delayNanoseconds))
            )
        }
    }

    private func firstSuccessfulData(
        candidates: [HTTPCompanionBaseURLRaceCandidate],
        path: String,
        method: HTTPMethod,
        bodyData: Data?,
        includesAuthentication: Bool
    ) async throws -> HTTPCompanionResponseData {
        guard !candidates.isEmpty else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        return try await withThrowingTaskGroup(of: HTTPCompanionBaseURLRaceEvent.self) { group in
            var raceState = HTTPCompanionBaseURLRaceState(candidates: candidates)
            var lastError: Error?

            func enqueueNextRequest() {
                let candidate = raceState.nextRequestCandidate()
                group.addTask {
                    do {
                        try Task.checkCancellation()
                        let response = try await responseData(
                            baseURL: candidate.baseURL,
                            path: path,
                            method: method,
                            bodyData: bodyData,
                            includesAuthentication: includesAuthentication
                        )
                        try Task.checkCancellation()
                        return .response(.success(response))
                    } catch {
                        if isCancellationError(error) {
                            throw error
                        }
                        return .response(.failure(error))
                    }
                }
            }

            func enqueueFallbackTimerIfNeeded() {
                guard let timer = raceState.nextFallbackTimer() else {
                    return
                }

                group.addTask {
                    if timer.delay != .zero {
                        try await Task.sleep(for: timer.delay)
                    }
                    try Task.checkCancellation()
                    return .fallbackTimer(generation: timer.generation)
                }
            }

            enqueueNextRequest()
            enqueueFallbackTimerIfNeeded()

            while raceState.shouldKeepWaiting {
                guard let event = try await group.next() else {
                    break
                }

                switch event {
                case let .response(.success(response)):
                    group.cancelAll()
                    return response

                case let .response(.failure(error)):
                    raceState.finishFailedRequest()
                    lastError = error

                    guard raceState.inFlightRequestCount == 0, raceState.hasRemainingCandidates else {
                        continue
                    }

                    raceState.ignorePendingFallbackTimer()
                    enqueueNextRequest()
                    enqueueFallbackTimerIfNeeded()

                case let .fallbackTimer(generation):
                    guard raceState.acceptsFallbackTimer(generation: generation) else {
                        continue
                    }

                    enqueueNextRequest()
                    enqueueFallbackTimerIfNeeded()
                }
            }

            group.cancelAll()
            throw lastError ?? HTTPCompanionServiceError.invalidResponse
        }
    }

    private func isCancellationError(_ error: Error) -> Bool {
        if error is CancellationError {
            return true
        }

        let nsError = error as NSError
        return nsError.domain == NSURLErrorDomain && nsError.code == NSURLErrorCancelled
    }
}

private enum LocalCompanionServiceDiscovery {
    static let serviceType = "_looper._tcp."
    static let domain = "local."
    static let timeoutSeconds: TimeInterval = 3

    @MainActor
    static func discoverBaseURLs() async -> [URL] {
        await BonjourServiceResolver().discoverBaseURLs()
    }
}

// NetService callbacks are driven from the run loop that starts discovery; cancellation also hops
// back to the main actor before touching resolver state.
private final class BonjourServiceResolver: NSObject, NetServiceBrowserDelegate, NetServiceDelegate, @unchecked Sendable {
    private var browser: NetServiceBrowser?
    private var continuation: CheckedContinuation<[URL], Never>?
    private var resolvedBaseURLs: [URL] = []
    private var services: [NetService] = []

    func discoverBaseURLs() async -> [URL] {
        await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                guard !Task.isCancelled else {
                    continuation.resume(returning: [])
                    return
                }

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
        } onCancel: {
            Task { @MainActor [weak self] in
                self?.finish()
            }
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
            returning: CompanionConfiguration.uniqueAttemptableBaseURLs(resolvedBaseURLs)
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
    static let health: TimeInterval = 2
    static let sessionDetail: TimeInterval = 4
    static let snapshot: TimeInterval = 3
    static let mutation: TimeInterval = 1.0
    static let fallback: TimeInterval = 5
    static let resource: TimeInterval = 6

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

private enum HTTPCompanionURLSession {
    static let shared: URLSession = {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.waitsForConnectivity = false
        configuration.timeoutIntervalForRequest = HTTPRequestTimeout.fallback
        configuration.timeoutIntervalForResource = HTTPRequestTimeout.resource
        return URLSession(configuration: configuration)
    }()
}

private actor HTTPCompanionRouteCache {
    static let shared = HTTPCompanionRouteCache()

    private static let cacheKeySeparator = "\n"

    private var successfulBaseURLByCandidateKey: [String: URL] = [:]

    func prioritizedBaseURLs(_ baseURLs: [URL]) -> [URL] {
        let key = cacheKey(for: baseURLs)
        guard let cachedBaseURL = successfulBaseURLByCandidateKey[key],
              baseURLs.contains(cachedBaseURL)
        else {
            return baseURLs
        }

        return [cachedBaseURL] + baseURLs.filter { $0 != cachedBaseURL }
    }

    func rememberSuccessfulBaseURL(_ baseURL: URL, for baseURLs: [URL]) {
        guard baseURLs.contains(baseURL) else {
            return
        }

        successfulBaseURLByCandidateKey[cacheKey(for: baseURLs)] = baseURL
    }

    func forgetFailedBaseURL(_ baseURL: URL, for baseURLs: [URL]) {
        let key = cacheKey(for: baseURLs)
        guard successfulBaseURLByCandidateKey[key] == baseURL else {
            return
        }

        successfulBaseURLByCandidateKey[key] = nil
    }

    private func cacheKey(for baseURLs: [URL]) -> String {
        baseURLs
            .map(\.absoluteString)
            .joined(separator: Self.cacheKeySeparator)
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
    case localStoreUnavailable
    case unauthorized
    case passkeySessionRequired(String)
    case serverError(String)

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "The looper API returned an invalid response."
        case .localStoreUnavailable:
            return "The looper realtime local store is unavailable."
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
    static let requestFailed = "Request failed."
}

private func serverErrorMessage(from data: Data) -> String {
    if let envelope = try? JSONDecoder().decode(MobileAPIErrorEnvelope.self, from: data),
       let message = envelope.message?.trimmingCharacters(in: .whitespacesAndNewlines),
       !message.isEmpty
    {
        return message
    }

    let fallbackMessage = String(data: data, encoding: .utf8)?
        .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return fallbackMessage.isEmpty ? MobileAPIErrorMessage.requestFailed : fallbackMessage
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

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

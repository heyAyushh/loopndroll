import Foundation
import LooperCompanionCore
import LooperRealtime

enum MobileEventType: String, Decodable, Sendable {
    case connected
    case sessionChanged = "session-changed"
    case promptQueued = "prompt-queued"
    case promptDelivered = "prompt-delivered"
    case lifecycleChanged = "lifecycle-changed"

    init(serverValue: String) {
        switch serverValue {
        case Self.connected.rawValue:
            self = .connected
        case "session.changed", Self.sessionChanged.rawValue:
            self = .sessionChanged
        case "prompt.queued", Self.promptQueued.rawValue:
            self = .promptQueued
        case "prompt.delivered", Self.promptDelivered.rawValue:
            self = .promptDelivered
        case "lifecycle.changed", Self.lifecycleChanged.rawValue:
            self = .lifecycleChanged
        default:
            self = .sessionChanged
        }
    }
}

struct MobileStreamEvent: Decodable, Sendable {
    let eventType: MobileEventType
    let threadID: String?
    let promptID: String?
    let detail: String?
    let serverTime: String?
    let revision: String?

    enum CodingKeys: String, CodingKey {
        case eventType
        case eventTypeSnake = "event_type"
        case threadID = "threadId"
        case threadIDSnake = "thread_id"
        case promptID = "promptId"
        case promptIDSnake = "prompt_id"
        case detail
        case serverTime
        case serverTimeSnake = "server_time"
        case revision
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let rawEventType = try container.decodeIfPresent(String.self, forKey: .eventType)
            ?? container.decodeIfPresent(String.self, forKey: .eventTypeSnake)
            ?? ""
        eventType = MobileEventType(serverValue: rawEventType)
        threadID = try container.decodeIfPresent(String.self, forKey: .threadID)
            ?? container.decodeIfPresent(String.self, forKey: .threadIDSnake)
        promptID = try container.decodeIfPresent(String.self, forKey: .promptID)
            ?? container.decodeIfPresent(String.self, forKey: .promptIDSnake)
        detail = try container.decodeIfPresent(String.self, forKey: .detail)
        serverTime = try container.decodeIfPresent(String.self, forKey: .serverTime)
            ?? container.decodeIfPresent(String.self, forKey: .serverTimeSnake)
        revision = try container.decodeIfPresent(String.self, forKey: .revision)
    }

    init(realtimeEvent: LooperRealtimeEvent) {
        eventType = MobileEventType(serverValue: realtimeEvent.eventName)
        threadID = realtimeEvent.threadID
        promptID = realtimeEvent.promptID
        detail = realtimeEvent.detail
        serverTime = realtimeEvent.serverTime
        revision = realtimeEvent.revision
    }
}

enum MobileEventStreamError: Error, Equatable {
    case invalidResponse
    case unauthorized
    case passkeySessionRequired
}

struct MobileEventStreamClient: Sendable {
    let baseURLs: [URL]
    let bearerToken: String?

    func streamEvents(
        onEvent: @escaping @MainActor @Sendable (MobileStreamEvent) async -> Void
    ) async throws {
        let candidateBaseURLs = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(baseURLs)
        guard !candidateBaseURLs.isEmpty else {
            throw MobileEventStreamError.invalidResponse
        }

        var lastError: Error?

        if let realtimeClient = await RealtimeCompanionClientFactory.makeClient(
            baseURLs: baseURLs,
            bearerToken: bearerToken
        ) {
            do {
                defer {
                    realtimeClient.disconnect()
                }
                try await realtimeClient.streamMobileEvents { event in
                    await onEvent(MobileStreamEvent(realtimeEvent: event))
                }
                return
            } catch {
                lastError = error
            }
        }

        for baseURL in candidateBaseURLs {
            do {
                try await streamEvents(baseURL: baseURL, onEvent: onEvent)
                return
            } catch {
                lastError = error
            }
        }

        throw lastError ?? MobileEventStreamError.invalidResponse
    }

    private func streamEvents(
        baseURL: URL,
        onEvent: @escaping @MainActor @Sendable (MobileStreamEvent) async -> Void
    ) async throws {
        var request = URLRequest(url: baseURL.appending(path: "/api/mobile/events"))
        request.httpMethod = "GET"
        request.timeoutInterval = MobileEventStreamConfiguration.requestTimeout
        request.setValue("text/event-stream", forHTTPHeaderField: "Accept")

        if let bearerToken {
            request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        }

        if let passkeySession = CompanionMobileSessionStore.loadValidHeaderValue() {
            request.setValue(passkeySession, forHTTPHeaderField: MobileAPIAuthenticationHeader.passkeySession)
        }

        let (bytes, response) = try await URLSession.shared.bytes(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw MobileEventStreamError.invalidResponse
        }

        if httpResponse.statusCode == 401 {
            throw HTTPCompanionServiceError.unauthorized
        }

        guard (200..<300).contains(httpResponse.statusCode)
        else {
            throw MobileEventStreamError.invalidResponse
        }

        var dataBuffer = ""
        let decoder = JSONDecoder()

        for try await byte in bytes {
            try Task.checkCancellation()

            let scalar = UnicodeScalar(byte)
            dataBuffer.append(Character(scalar))

            while let newlineRange = dataBuffer.range(of: "\n") {
                let line = String(dataBuffer[..<newlineRange.lowerBound])
                dataBuffer.removeSubrange(..<newlineRange.upperBound)

                guard line.hasPrefix("data:") else {
                    continue
                }

                let payload = line.dropFirst("data:".count).trimmingCharacters(in: .whitespaces)
                guard !payload.isEmpty,
                      let payloadData = payload.data(using: .utf8)
                else {
                    continue
                }

                if let event = try? decoder.decode(MobileStreamEvent.self, from: payloadData) {
                    await onEvent(event)
                }
            }
        }
    }
}

private enum MobileEventStreamConfiguration {
    static let requestTimeout: TimeInterval = 300
}

private enum MobileAPIAuthenticationHeader {
    static let passkeySession = "X-Looper-Mobile-Session"
}

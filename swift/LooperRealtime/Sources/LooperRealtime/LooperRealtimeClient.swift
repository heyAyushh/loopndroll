import Foundation
import LooperClientCore

public final class LooperRealtimeClient: Sendable {
    private static let stateMiniSnapshotPath = "/api/mobile/session-minis/snapshot"

    private let endpoints: [LooperRealtimeEndpoint]
    private let credentials: LooperRealtimeCredentials

    public init(endpoints: [LooperRealtimeEndpoint], credentials: LooperRealtimeCredentials) {
        self.endpoints = endpoints
        self.credentials = credentials
    }

    public func warmConnections() async throws {
        let clientCore = LooperClientCore()
        _ = try await clientCore.warmConnection(
            endpoints: endpoints.map(\.clientCoreEndpoint),
            bearerToken: credentials.bearerToken ?? "",
            mobileSessionHeader: credentials.mobileSessionHeader ?? ""
        )
    }

    public func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        let response = try await clientCore.submitExpectedOutbox(
            endpoints: endpoints.map(\.clientCoreEndpoint),
            bearerToken: credentials.bearerToken ?? "",
            mobileSessionHeader: credentials.mobileSessionHeader ?? "",
            expectedClientMutationIds: expectedClientMutationIDs
        )
        return LooperRealtimeSessionCommandBatchResponse(response)
    }

    public func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot {
        var lastError: Error?
        for endpoint in endpoints {
            do {
                return try await stateMiniSnapshot(endpoint: endpoint)
            } catch {
                lastError = error
            }
        }
        throw lastError ?? LooperRealtimeError.unavailable
    }

    public func startClientCoreStateMiniStream(clientCore: LooperClientCore) async throws {
        _ = try clientCore.startStateMiniStream(
            endpoints: endpoints.map(\.clientCoreEndpoint),
            bearerToken: credentials.bearerToken ?? "",
            mobileSessionHeader: credentials.mobileSessionHeader ?? ""
        )
    }

    public func nextClientCoreStateMiniStreamUpdate(
        clientCore: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate {
        try await clientCore.nextStateMiniStreamUpdate()
    }

    public func stopClientCoreStateMiniStream(clientCore: LooperClientCore) throws {
        _ = try clientCore.stopStateMiniStream()
    }

    private func stateMiniSnapshot(endpoint: LooperRealtimeEndpoint) async throws
        -> LooperRealtimeStateMiniSnapshot
    {
        let url = endpoint.baseURL.appending(path: Self.stateMiniSnapshotPath)
        var request = URLRequest(url: url)
        request.httpMethod = "GET"
        request.timeoutInterval = 2
        for (key, value) in credentials.httpHeaders {
            request.setValue(value, forHTTPHeaderField: key)
        }

        let (data, urlResponse) = try await URLSession.shared.data(for: request)
        guard let httpResponse = urlResponse as? HTTPURLResponse,
              (200 ..< 300).contains(httpResponse.statusCode)
        else {
            throw LooperRealtimeError.unavailable
        }
        let snapshotResponse = try JSONDecoder().decode(
            LooperRealtimeStateMiniSnapshotResponse.self,
            from: data
        )
        return LooperRealtimeStateMiniSnapshot(from: snapshotResponse)
    }
}

extension LooperRealtimeClient: LooperRealtimeSessionCommandSubmitting {}
extension LooperRealtimeClient: LooperRealtimeClientCoreStateMiniStreamTransport {}

private extension LooperRealtimeEndpoint {
    var clientCoreEndpoint: ClientEndpoint {
        ClientEndpoint(url: baseURL.absoluteString, lastGood: false)
    }
}

private extension LooperRealtimeCredentials {
    var httpHeaders: [(String, String)] {
        var headers: [(String, String)] = []
        if let bearerToken {
            headers.append(("authorization", "Bearer \(bearerToken)"))
        }
        if let mobileSessionHeader {
            headers.append(("x-looper-mobile-session", mobileSessionHeader))
        }
        return headers
    }

}

private extension LooperRealtimeStateMiniSnapshot {
    init(from response: LooperRealtimeStateMiniSnapshotResponse) {
        self.init(
            latestSeq: response.latestSeq,
            sessions: response.sessions.compactMap(LooperRealtimeStateMini.init),
            serverTime: response.serverTime
        )
    }
}

private extension LooperRealtimeStateMini {
    init?(_ value: LooperRealtimeStateMiniJSON) {
        guard let sessionID = value.sessionID?.nilIfEmpty else {
            return nil
        }
        let assistantSurface = value.assistantSurface?.nilIfEmpty ?? ""
        self.init(
            sessionID: sessionID,
            assistantSurface: assistantSurface,
            seq: value.seq ?? 0,
            revision: value.revision?.nilIfEmpty ?? "",
            payloadJSON: value.payloadJSON
        )
    }

    init?(seq: Int64, revision: String, payloadJSON: String) {
        guard let data = payloadJSON.data(using: .utf8),
              let value = try? JSONDecoder().decode(LooperRealtimeStateMiniJSON.self, from: data),
              let sessionID = value.sessionID?.nilIfEmpty
        else {
            return nil
        }

        self.init(
            sessionID: sessionID,
            assistantSurface: value.assistantSurface?.nilIfEmpty ?? "",
            seq: value.seq ?? seq,
            revision: value.revision?.nilIfEmpty ?? revision,
            payloadJSON: payloadJSON
        )
    }
}

private struct LooperRealtimeStateMiniSnapshotResponse: Decodable {
    let latestSeq: Int64
    let sessions: [LooperRealtimeStateMiniJSON]
    let serverTime: String?

    private enum CodingKeys: String, CodingKey {
        case latestSeq
        case latestSeqSnake = "latest_seq"
        case sessions
        case serverTime
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        latestSeq = try container.decodeIfPresent(Int64.self, forKey: .latestSeq)
            ?? container.decodeIfPresent(Int64.self, forKey: .latestSeqSnake)
            ?? 0
        sessions = try container.decodeIfPresent(
            [LooperRealtimeStateMiniJSON].self,
            forKey: .sessions
        ) ?? []
        serverTime = try container.decodeIfPresent(String.self, forKey: .serverTime)
    }
}

private struct LooperRealtimeStateMiniJSON: Decodable {
    let sessionID: String?
    let assistantSurface: String?
    let seq: Int64?
    let revision: String?
    let payloadJSON: String

    private enum CodingKeys: String, CodingKey {
        case sessionID
        case sessionId
        case assistantSurface
        case seq
        case revision
    }

    init(from decoder: Decoder) throws {
        payloadJSON = try Self.rawJSON(from: decoder)
        let container = try decoder.container(keyedBy: CodingKeys.self)
        sessionID = try container.decodeIfPresent(String.self, forKey: .sessionID)
            ?? container.decodeIfPresent(String.self, forKey: .sessionId)
        assistantSurface = try container.decodeIfPresent(String.self, forKey: .assistantSurface)
        seq = try container.decodeIfPresent(Int64.self, forKey: .seq)
        revision = try container.decodeIfPresent(String.self, forKey: .revision)
    }

    private static func rawJSON(from decoder: Decoder) throws -> String {
        let value = try LooperRealtimeRawJSON(from: decoder).value
        let data = try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
        return String(decoding: data, as: UTF8.self)
    }
}

private struct LooperRealtimeRawJSON: Decodable {
    let value: Any

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if let object = try? container.decode([String: LooperRealtimeRawJSON].self) {
            value = object.mapValues(\.value)
        } else if let array = try? container.decode([LooperRealtimeRawJSON].self) {
            value = array.map(\.value)
        } else if let string = try? container.decode(String.self) {
            value = string
        } else if let double = try? container.decode(Double.self) {
            value = double
        } else if let bool = try? container.decode(Bool.self) {
            value = bool
        } else if container.decodeNil() {
            value = NSNull()
        } else {
            value = [:] as [String: Any]
        }
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

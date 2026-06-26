import Foundation
import GRPCCore
import GRPCNIOTransportHTTP2
import LooperClientCore
import Synchronization

public final class LooperRealtimeClient: Sendable {
    private static let stateMiniSnapshotPath = "/api/mobile/session-minis/snapshot"

    private let endpoints: [LooperRealtimeEndpoint]
    private let credentials: LooperRealtimeCredentials
    private let connections = LooperRealtimeConnectionPool()

    public init(endpoints: [LooperRealtimeEndpoint], credentials: LooperRealtimeCredentials) {
        self.endpoints = endpoints
        self.credentials = credentials
    }

    deinit {
        disconnect()
    }

    public func disconnect() {
        connections.disconnectAll()
    }

    public func warmConnections() async throws {
        try await withFirstAvailableService { service, _ in
            let response = try await service.health(
                Looper_V1_HealthRequest(),
                options: LooperRealtimeLatencyPolicy.warmupCallOptions
            )
            guard response.ok else {
                throw LooperRealtimeError.unavailable
            }
        }
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

    public func streamStateMinis(
        afterSeq: Int64,
        onDelta: @escaping @Sendable (LooperRealtimeStateMiniDelta) async throws -> Void
    ) async throws {
        try await withFirstAvailableService { service, metadata in
            try await service.session(
                metadata: metadata,
                options: LooperRealtimeLatencyPolicy.streamCallOptions,
                requestProducer: { writer in
                    var frame = Looper_V1_ClientFrame()
                    var resume = Looper_V1_Resume()
                    resume.afterSeq = afterSeq
                    frame.resume = resume
                    try await writer.write(frame)
                },
                onResponse: { response in
                    for try await frame in response.messages {
                        guard case let .stateDelta(delta)? = frame.frame else {
                            continue
                        }
                        try await onDelta(LooperRealtimeStateMiniDelta(delta))
                    }
                }
            )
        }
    }

    private func withFirstAvailableService<Result: Sendable>(
        _ operation: @Sendable @escaping (
            Looper_V1_LooperRealtime.Client<HTTP2ClientTransport.TransportServices>,
            Metadata
        ) async throws -> Result
    ) async throws -> Result {
        var lastError: Error?

        for endpoint in endpoints {
            do {
                return try await withService(endpoint: endpoint, operation)
            } catch {
                connections.disconnect(endpoint: endpoint)
                lastError = error
            }
        }

        throw lastError ?? LooperRealtimeError.unavailable
    }

    private func withService<Result: Sendable>(
        endpoint: LooperRealtimeEndpoint,
        _ operation: @Sendable @escaping (
            Looper_V1_LooperRealtime.Client<HTTP2ClientTransport.TransportServices>,
            Metadata
        ) async throws -> Result
    ) async throws -> Result {
        guard let host = endpoint.host, let port = endpoint.port else {
            throw LooperRealtimeError.invalidEndpoint
        }

        let client = try connections.client(for: endpoint, host: host, port: port)
        let service = Looper_V1_LooperRealtime.Client(wrapping: client)
        return try await operation(service, credentials.metadata)
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

extension LooperRealtimeClient: LooperRealtimeStateMiniSyncTransport {}
extension LooperRealtimeClient: LooperRealtimeSessionCommandSubmitting {}

private extension LooperRealtimeEndpoint {
    var clientCoreEndpoint: ClientEndpoint {
        ClientEndpoint(url: baseURL.absoluteString, lastGood: false)
    }
}

private final class LooperRealtimeConnectionPool: Sendable {
    private typealias TransportServices = HTTP2ClientTransport.TransportServices
    private typealias ManagedClient = GRPCClient<TransportServices>

    private let state = Mutex<[LooperRealtimeEndpoint: LooperRealtimeConnection]>([:])

    func client(
        for endpoint: LooperRealtimeEndpoint,
        host: String,
        port: Int
    ) throws -> GRPCClient<HTTP2ClientTransport.TransportServices> {
        try state.withLock { state in
            if let connection = state[endpoint] {
                return connection.client
            }

            let transport = try TransportServices(
                target: .dns(host: host, port: port),
                transportSecurity: endpoint.usesTLS ? .tls : .plaintext,
                config: LooperRealtimeLatencyPolicy.transportConfig
            )
            let client = ManagedClient(transport: transport)
            let connectionTask = Task {
                try await client.runConnections()
            }
            state[endpoint] = LooperRealtimeConnection(
                client: client,
                connectionTask: connectionTask
            )
            return client
        }
    }

    func disconnect(endpoint: LooperRealtimeEndpoint) {
        guard let connection = state.withLock({ state in
            state.removeValue(forKey: endpoint)
        }) else {
            return
        }

        connection.shutdown()
    }

    func disconnectAll() {
        let connections = state.withLock { state in
            let connections = Array(state.values)
            state.removeAll(keepingCapacity: true)
            return connections
        }

        for connection in connections {
            connection.shutdown()
        }
    }
}

private struct LooperRealtimeConnection: Sendable {
    let client: GRPCClient<HTTP2ClientTransport.TransportServices>
    let connectionTask: Task<Void, any Error>

    func shutdown() {
        client.beginGracefulShutdown()
        connectionTask.cancel()
    }
}

enum LooperRealtimeLatencyPolicy {
    static let keepaliveTime: Duration = .seconds(20)
    static let keepaliveTimeout: Duration = .seconds(5)
    static let reconnectInitialBackoff: Duration = .milliseconds(200)
    static let reconnectMaxBackoff: Duration = .seconds(2)
    static let warmupTimeout: Duration = .milliseconds(1_500)
    static let modeTimeout: Duration = .milliseconds(300)
    static let promptTimeout: Duration = .milliseconds(900)
    static let reconnectMultiplier = 1.2
    static let reconnectJitter = 0.1

    static var transportConfig: HTTP2ClientTransport.TransportServices.Config {
        .defaults { config in
            config.connection.maxIdleTime = nil
            config.connection.keepalive = HTTP2ClientTransport.Config.Keepalive(
                time: keepaliveTime,
                timeout: keepaliveTimeout,
                allowWithoutCalls: true
            )
            config.backoff = HTTP2ClientTransport.Config.Backoff(
                initial: reconnectInitialBackoff,
                max: reconnectMaxBackoff,
                multiplier: reconnectMultiplier,
                jitter: reconnectJitter
            )
        }
    }

    static var warmupCallOptions: CallOptions {
        failFastCallOptions(timeout: warmupTimeout)
    }

    static var modeCallOptions: CallOptions {
        failFastCallOptions(timeout: modeTimeout)
    }

    static var promptCallOptions: CallOptions {
        failFastCallOptions(timeout: promptTimeout)
    }

    static var streamCallOptions: CallOptions {
        failFastCallOptions(timeout: nil)
    }

    private static func failFastCallOptions(timeout: Duration?) -> CallOptions {
        var options = CallOptions.defaults
        options.timeout = timeout
        options.waitForReady = false
        return options
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

    var metadata: Metadata {
        var metadata = Metadata()
        for (key, value) in httpHeaders {
            metadata.addString(value, forKey: key)
        }
        return metadata
    }
}

private extension LooperRealtimeEvent {
    init(_ event: Looper_V1_MobileEvent) {
        self.init(
            eventName: event.eventName,
            threadID: event.threadID.nilIfEmpty,
            promptID: event.promptID.nilIfEmpty,
            detail: event.detail.nilIfEmpty,
            serverTime: event.serverTime.nilIfEmpty,
            revision: event.revision.nilIfEmpty
        )
    }
}

private extension LooperRealtimeCommandAck {
    init(_ ack: Looper_V1_CommandAck) {
        self.init(
            accepted: ack.accepted,
            clientMutationID: ack.clientMutationID,
            ackSeq: ack.ackSeq,
            entityID: ack.entityID,
            revision: ack.revision,
            serverTime: ack.serverTime.nilIfEmpty,
            idempotentReplay: ack.idempotentReplay,
            errorCode: ack.errorCode.nilIfEmpty,
            rejectReason: ack.rejectReason.nilIfEmpty
        )
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

private extension LooperRealtimeStateMiniDelta {
    init(_ delta: Looper_V1_StateMiniDelta) {
        let session = LooperRealtimeStateMini(
            seq: delta.seq,
            revision: delta.revision,
            payloadJSON: delta.payloadJson
        )
        self.init(
            seq: delta.seq,
            latestSeq: delta.seq,
            entityID: delta.entityID,
            kind: delta.kind,
            revision: delta.revision,
            serverTime: delta.serverTime.nilIfEmpty,
            session: session,
            sessionID: session?.sessionID,
            assistantSurface: session?.assistantSurface,
            sessions: session.map { [$0] } ?? []
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

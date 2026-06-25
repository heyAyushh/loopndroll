import Foundation
import GRPCCore
import GRPCNIOTransportHTTP2
import Synchronization

public final class LooperRealtimeClient: Sendable {
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

    public func streamMobileEvents(
        onEvent: @escaping @Sendable (LooperRealtimeEvent) async -> Void
    ) async throws {
        try await withFirstAvailableService { service, metadata in
            try await service.subscribeMobileEvents(
                Looper_V1_SubscribeEventsRequest(),
                metadata: metadata,
                options: LooperRealtimeLatencyPolicy.streamCallOptions
            ) { response in
                for try await event in response.messages {
                    await onEvent(LooperRealtimeEvent(event))
                }
            }
        }
    }

    public func streamDesktopEvents(
        onEvent: @escaping @Sendable (LooperRealtimeEvent) async -> Void
    ) async throws {
        try await withFirstAvailableService { service, _ in
            try await service.subscribeDesktopEvents(
                Looper_V1_SubscribeEventsRequest(),
                options: LooperRealtimeLatencyPolicy.streamCallOptions
            ) { response in
                for try await event in response.messages {
                    await onEvent(LooperRealtimeEvent(event))
                }
            }
        }
    }

    public func setSessionMode(
        threadID: String,
        preset: String?,
        clientMutationID: String = UUID().uuidString
    ) async throws -> LooperRealtimeModeResponse {
        try await withFirstAvailableService { service, metadata in
            var request = Looper_V1_SetSessionModeRequest()
            request.threadID = threadID
            request.preset = preset ?? ""
            request.clientMutationID = clientMutationID

            let response = try await service.setSessionMode(
                request,
                metadata: metadata,
                options: LooperRealtimeLatencyPolicy.modeCallOptions
            )
            return LooperRealtimeModeResponse(response)
        }
    }

    public func sendSessionPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String = UUID().uuidString
    ) async throws -> LooperRealtimePromptResponse {
        try await withFirstAvailableService { service, metadata in
            var request = Looper_V1_SendSessionPromptRequest()
            request.threadID = threadID
            request.prompt = prompt
            request.assistantSurface = assistantSurface ?? ""
            request.clientMutationID = clientMutationID

            let response = try await service.sendSessionPrompt(
                request,
                metadata: metadata,
                options: LooperRealtimeLatencyPolicy.promptCallOptions
            )
            return LooperRealtimePromptResponse(response)
        }
    }

    public func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String = UUID().uuidString
    ) async throws -> LooperRealtimeNotificationReplyResponse {
        try await withFirstAvailableService { service, metadata in
            var request = Looper_V1_SubmitNotificationReplyRequest()
            request.notificationID = notificationID
            request.threadID = threadID
            request.prompt = prompt
            request.assistantSurface = assistantSurface ?? ""
            request.clientMutationID = clientMutationID

            let response = try await service.submitNotificationReply(
                request,
                metadata: metadata,
                options: LooperRealtimeLatencyPolicy.promptCallOptions
            )
            return LooperRealtimeNotificationReplyResponse(response)
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
    var metadata: Metadata {
        var metadata = Metadata()
        if let bearerToken {
            metadata.addString("Bearer \(bearerToken)", forKey: "authorization")
        }
        if let mobileSessionHeader {
            metadata.addString(mobileSessionHeader, forKey: "x-looper-mobile-session")
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
            idempotentReplay: ack.idempotentReplay
        )
    }
}

private extension LooperRealtimeModeResponse {
    init(_ response: Looper_V1_SetSessionModeResponse) {
        self.init(
            accepted: response.accepted,
            threadID: response.threadID,
            preset: response.preset.nilIfEmpty,
            serverTime: response.serverTime.nilIfEmpty,
            clientMutationID: response.clientMutationID,
            ackSeq: response.ackSeq,
            entityID: response.entityID,
            revision: response.revision,
            idempotentReplay: response.idempotentReplay
        )
    }
}

private extension LooperRealtimePromptResponse {
    init(_ response: Looper_V1_SendSessionPromptResponse) {
        self.init(
            accepted: response.accepted,
            dispatchKind: response.dispatchKind,
            promptID: response.promptID.nilIfEmpty,
            serverTime: response.serverTime.nilIfEmpty,
            clientMutationID: response.clientMutationID,
            ackSeq: response.ackSeq,
            entityID: response.entityID,
            revision: response.revision,
            idempotentReplay: response.idempotentReplay
        )
    }
}

private extension LooperRealtimeNotificationReplyResponse {
    init(_ response: Looper_V1_SubmitNotificationReplyResponse) {
        self.init(
            accepted: response.accepted,
            dispatchKind: response.dispatchKind,
            promptID: response.promptID.nilIfEmpty,
            serverTime: response.serverTime.nilIfEmpty,
            clientMutationID: response.clientMutationID,
            ackSeq: response.ackSeq,
            entityID: response.entityID,
            revision: response.revision,
            idempotentReplay: response.idempotentReplay,
            notificationID: response.notificationID
        )
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

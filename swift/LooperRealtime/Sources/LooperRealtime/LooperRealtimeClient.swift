import Foundation
import GRPCCore
import GRPCNIOTransportHTTP2

public struct LooperRealtimeClient: Sendable {
    private let endpoints: [LooperRealtimeEndpoint]
    private let credentials: LooperRealtimeCredentials

    public init(endpoints: [LooperRealtimeEndpoint], credentials: LooperRealtimeCredentials) {
        self.endpoints = endpoints
        self.credentials = credentials
    }

    public func streamMobileEvents(
        onEvent: @escaping @Sendable (LooperRealtimeEvent) async -> Void
    ) async throws {
        try await withFirstAvailableService { service, metadata in
            try await service.subscribeMobileEvents(
                Looper_V1_SubscribeEventsRequest(),
                metadata: metadata
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
            try await service.subscribeDesktopEvents(Looper_V1_SubscribeEventsRequest()) { response in
                for try await event in response.messages {
                    await onEvent(LooperRealtimeEvent(event))
                }
            }
        }
    }

    public func sendSessionPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String?
    ) async throws -> LooperRealtimePromptResponse {
        try await withFirstAvailableService { service, metadata in
            var request = Looper_V1_SendSessionPromptRequest()
            request.threadID = threadID
            request.prompt = prompt
            request.assistantSurface = assistantSurface ?? ""

            let response = try await service.sendSessionPrompt(request, metadata: metadata)
            return LooperRealtimePromptResponse(response)
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

        let transport = try HTTP2ClientTransport.TransportServices(
            target: .dns(host: host, port: port),
            transportSecurity: endpoint.usesTLS ? .tls : .plaintext
        )

        return try await withGRPCClient(transport: transport) { grpcClient in
            let service = Looper_V1_LooperRealtime.Client(wrapping: grpcClient)
            return try await operation(service, credentials.metadata)
        }
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

private extension LooperRealtimePromptResponse {
    init(_ response: Looper_V1_SendSessionPromptResponse) {
        self.init(
            accepted: response.accepted,
            dispatchKind: response.dispatchKind,
            promptID: response.promptID.nilIfEmpty
        )
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

import Foundation

public final class LooperRealtimeClient: Sendable {
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

    public func recoverClientCoreStateMiniSnapshot(
        clientCore: LooperClientCore
    ) async throws -> ClientStateSnapshot {
        try await clientCore.recoverStateMiniSnapshot(
            endpoints: endpoints.map(\.clientCoreEndpoint),
            bearerToken: credentials.bearerToken ?? "",
            mobileSessionHeader: credentials.mobileSessionHeader ?? ""
        )
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
}

extension LooperRealtimeClient: LooperRealtimeSessionCommandSubmitting {}
extension LooperRealtimeClient: LooperClientCoreStateMiniStreamTransport {}

private extension LooperRealtimeEndpoint {
    var clientCoreEndpoint: ClientEndpoint {
        ClientEndpoint(url: baseURL.absoluteString, lastGood: false)
    }
}

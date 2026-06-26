public protocol LooperClientCoreStateMiniStreamTransport: Sendable {
    func startClientCoreStateMiniStream(clientCore: LooperClientCore) async throws
    func nextClientCoreStateMiniStreamUpdate(
        clientCore: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate
    func stopClientCoreStateMiniStream(clientCore: LooperClientCore) throws
}

extension LooperClientCore: LooperClientCoreStateMiniStreamTransport {
    public func startClientCoreStateMiniStream(clientCore _: LooperClientCore) async throws {
        _ = try startConfiguredStateMiniStream()
    }

    public func nextClientCoreStateMiniStreamUpdate(
        clientCore _: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate {
        try await observe()
    }

    public func stopClientCoreStateMiniStream(clientCore _: LooperClientCore) throws {
        _ = try stopStateMiniStream()
    }
}

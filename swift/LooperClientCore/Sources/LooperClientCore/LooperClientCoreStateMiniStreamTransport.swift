public protocol LooperClientCoreStateMiniStreamTransport: Sendable {
    func recoverClientCoreStateMiniSnapshot(
        clientCore: LooperClientCore
    ) async throws -> ClientStateSnapshot
    func startClientCoreStateMiniStream(clientCore: LooperClientCore) async throws
    func nextClientCoreStateMiniStreamUpdate(
        clientCore: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate
    func stopClientCoreStateMiniStream(clientCore: LooperClientCore) throws
}

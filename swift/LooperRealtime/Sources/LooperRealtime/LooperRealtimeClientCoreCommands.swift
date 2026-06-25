import Foundation
import LooperClientCore

public protocol LooperRealtimeSessionCommandSubmitting: Sendable {
    func submitSessionCommandBatch(
        commands: [LooperRealtimeSessionCommand]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse
}

public extension LooperRealtimeSessionCommandSubmitting {
    func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        let frames = try clientCore.takeExpectedOutbox(
            expectedClientMutationIds: expectedClientMutationIDs
        )
        let commands = try frames.map(LooperRealtimeSessionCommand.init(outboundFrame:))
        let response = try await submitSessionCommandBatch(commands: commands)
        _ = try clientCore.applyCommandBatchResponse(response: response.clientCoreResponse())
        return response
    }
}

extension LooperRealtimeClient: LooperRealtimeSessionCommandSubmitting {}

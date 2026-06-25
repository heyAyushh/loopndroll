import Foundation
import LooperClientCore

public protocol LooperRealtimeSessionCommandSubmitting: Sendable {
    func submitSessionCommandBatch(
        commands: [LooperRealtimeSessionCommand]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse
}

public enum LooperRealtimeClientCoreCommandError: Error, Equatable, Sendable {
    case unexpectedOutboxMutations(expected: [String], actual: [String])
}

public extension LooperRealtimeSessionCommandSubmitting {
    func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse {
        let frames = try clientCore.takeOutbox()
        let actualClientMutationIDs = frames.map(\.clientMutationId)
        guard actualClientMutationIDs == expectedClientMutationIDs else {
            throw LooperRealtimeClientCoreCommandError.unexpectedOutboxMutations(
                expected: expectedClientMutationIDs,
                actual: actualClientMutationIDs
            )
        }

        let commands = try frames.map(LooperRealtimeSessionCommand.init(outboundFrame:))
        let response = try await submitSessionCommandBatch(commands: commands)
        for envelope in response.commandAcks {
            _ = try clientCore.applyCommandAck(ack: envelope.ack.clientCoreAck)
        }
        return response
    }
}

extension LooperRealtimeClient: LooperRealtimeSessionCommandSubmitting {}

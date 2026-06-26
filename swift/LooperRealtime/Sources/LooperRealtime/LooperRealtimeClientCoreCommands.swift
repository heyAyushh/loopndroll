import Foundation
import LooperClientCore

public protocol LooperRealtimeSessionCommandSubmitting: Sendable {
    func submitClientCoreOutbox(
        clientCore: LooperClientCore,
        expectedClientMutationIDs: [String]
    ) async throws -> LooperRealtimeSessionCommandBatchResponse
}

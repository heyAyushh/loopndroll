import Foundation
import LooperClientCore

enum CompanionConnectionStateReducer {
    static func snapshotLoadFailure(
        mappedErrorState: ConnectivityState,
        currentState: ConnectivityState,
        hasUsableSnapshot: Bool,
        hasServerHealth: Bool,
        hasReachedBaseURL: Bool
    ) -> ClientSnapshotLoadFailureProjection {
        do {
            return try reduceSnapshotLoadFailure(
                mappedErrorState: mappedErrorState.rawValue,
                currentConnectionState: currentState.rawValue,
                hasUsableSnapshot: hasUsableSnapshot,
                hasServerHealth: hasServerHealth,
                hasReachedBaseUrl: hasReachedBaseURL
            )
        } catch {
            fatalError("Connection state reducer failed: \(error)")
        }
    }

    static func connectionState(
        from projection: ClientSnapshotLoadFailureProjection
    ) -> ConnectivityState {
        guard let state = ConnectivityState(rawValue: projection.connectionState) else {
            fatalError("Connection reducer returned unknown state: \(projection.connectionState)")
        }
        return state
    }
}

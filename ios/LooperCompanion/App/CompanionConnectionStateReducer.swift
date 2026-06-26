import Foundation
import LooperClientCore

enum CompanionConnectionStateReducer {
    static func connectionFailure(
        mappedErrorState: ConnectivityState,
        hasUsableSnapshot: Bool,
        suppressErrorWhenSnapshotUsable: Bool
    ) -> ClientConnectionFailureProjection {
        do {
            return try reduceConnectionFailure(
                mappedErrorState: mappedErrorState.rawValue,
                hasUsableSnapshot: hasUsableSnapshot,
                suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
            )
        } catch {
            fatalError("Connection failure reducer failed: \(error)")
        }
    }

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
        connectionState(rawValue: projection.connectionState)
    }

    static func connectionState(
        from projection: ClientConnectionFailureProjection
    ) -> ConnectivityState {
        connectionState(rawValue: projection.connectionState)
    }

    private static func connectionState(rawValue: String) -> ConnectivityState {
        guard let state = ConnectivityState(rawValue: rawValue) else {
            fatalError("Connection reducer returned unknown state: \(rawValue)")
        }
        return state
    }
}

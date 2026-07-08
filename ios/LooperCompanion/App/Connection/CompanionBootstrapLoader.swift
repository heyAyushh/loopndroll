import Foundation

/// Distinguishes a genuine timeout from a phase that already settled into a
/// state no amount of waiting will resolve (locked/unauthorized/unpaired) —
/// callers bounce back immediately instead of burning the full timeout.
enum CompanionBootstrapOutcome: Equatable {
    case connected
    case blocked(ConnectivityState)
    case timedOut
}

@MainActor
protocol CompanionBootstrapLoaderDelegate: AnyObject {
    var bootstrapLoaderConnectionState: ConnectivityState { get }
}

/// First-install/pairing bootstrap. Replaces sampling `connectionState`
/// immediately after `saveConnectionCode`/`saveConnectionOrbID` (the deleted
/// HTTP snapshot hot path used to make that sample meaningful by blocking on
/// a network round-trip). The realtime stream is async, so pairing now waits
/// on the runtime to actually prove liveness, bounded by a timeout.
@MainActor
final class CompanionBootstrapLoader {
    private enum Polling {
        /// Fine-grained enough that pairing feels instant once the stream
        /// proves live, coarse enough not to spin the run loop while waiting.
        static let interval: Duration = .milliseconds(50)
    }

    private weak var delegate: CompanionBootstrapLoaderDelegate?

    init(delegate: CompanionBootstrapLoaderDelegate) {
        self.delegate = delegate
    }

    @discardableResult
    func awaitPhase(timeout: Duration) async -> CompanionBootstrapOutcome {
        let deadline = ContinuousClock.now.advanced(by: timeout)
        while true {
            if let state = delegate?.bootstrapLoaderConnectionState {
                if state == .connected {
                    return .connected
                }
                if Self.isTerminallyBlocked(state) {
                    return .blocked(state)
                }
            }

            guard ContinuousClock.now < deadline else {
                return .timedOut
            }
            try? await Task.sleep(for: Polling.interval)
        }
    }

    private static func isTerminallyBlocked(_ state: ConnectivityState) -> Bool {
        switch state {
        case .unauthorized, .locked, .unpaired:
            return true
        case .connecting, .connected, .offline:
            return false
        }
    }
}

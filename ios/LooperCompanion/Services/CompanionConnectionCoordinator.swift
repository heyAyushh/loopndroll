import Foundation
import LooperCompanionCore

enum CompanionConnectionResolutionError: LocalizedError {
    case savedConnectionUnavailable(String?)

    var errorDescription: String? {
        switch self {
        case let .savedConnectionUnavailable(message):
            if let message, !message.isEmpty {
                return message
            }
            return "The orb was accepted, but looper is not reachable yet."
        }
    }
}

@MainActor
protocol CompanionConnectionCoordinatorDelegate: AnyObject {
    var connectionCoordinatorCurrentState: ConnectivityState { get }
    var connectionCoordinatorErrorMessage: String? { get }
    var connectionCoordinatorConfiguredBaseURL: String { get }

    func connectionCoordinatorReloadConnection() async
}

@MainActor
final class CompanionConnectionCoordinator {
    private weak var delegate: CompanionConnectionCoordinatorDelegate?

    init(delegate: CompanionConnectionCoordinatorDelegate) {
        self.delegate = delegate
    }

    func saveBaseURL(_ value: String) async {
        let baseURLs = CompanionConfiguration.normalizedBaseURLsForUserInput(value)
        CompanionConfiguration.storeConnection(
            CompanionConnection(baseURLs: baseURLs, bearerToken: nil)
        )
        await delegate?.connectionCoordinatorReloadConnection()
    }

    func saveConnection(_ connection: CompanionConnection) async {
        CompanionConfiguration.storeConnection(connection)
        await delegate?.connectionCoordinatorReloadConnection()
    }

    func setRoutePreference(_ preference: CompanionConnectionRoutePreference) async {
        let currentConnection = CompanionConfiguration.resolvedConnection()
        let currentPreference = CompanionConfiguration.connectionRoutePreference()
        guard preference != currentPreference else {
            return
        }

        CompanionConfiguration.storeConnectionRoutePreference(preference)
        CompanionConfiguration.storeConnection(
            currentConnection,
            mobileSessionPolicy: .preserveIfBearerTokenUnchanged
        )
        await delegate?.connectionCoordinatorReloadConnection()
        let primaryBaseURL = delegate?.connectionCoordinatorConfiguredBaseURL ?? ""
        CompanionDiagnostics.record(
            "connection:route-preference preference=\(preference.rawValue) primary=\(primaryBaseURL)"
        )
    }

    func saveConnectionCode(_ connectionCode: String) async throws {
        let trimmedConnectionCode = connectionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnectionCode.isEmpty else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        let connection = try CompanionConfiguration.resolveConnection(
            fromConnectionCode: trimmedConnectionCode
        )
        await saveConnection(connection)
    }

    func saveConnectionOrbID(_ orbID: String) async throws {
        let resolver = HTTPCompanionService(
            baseURLs: CompanionConfiguration.resolvedBaseURLStrings(),
            bearerToken: nil
        )
        let connectionCode = try await resolver.resolveConnectionCode(orbID: orbID)
        try await saveConnectionCode(connectionCode.code)

        guard delegate?.connectionCoordinatorCurrentState == .connected else {
            throw CompanionConnectionResolutionError.savedConnectionUnavailable(
                delegate?.connectionCoordinatorErrorMessage
            )
        }
    }
}

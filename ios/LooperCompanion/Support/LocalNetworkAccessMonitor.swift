import Foundation
import Network
import Observation
import UIKit

enum LocalNetworkAccessStatus: Equatable, Sendable {
    case notChecked
    case checking
    case available
    case denied
    case unavailable(String)

    var label: String {
        switch self {
        case .notChecked:
            return "Not Checked"
        case .checking:
            return "Checking"
        case .available:
            return "Allowed"
        case .denied:
            return "Denied"
        case .unavailable:
            return "Unavailable"
        }
    }

    var summary: String {
        switch self {
        case .notChecked:
            return "Use the toggle to check whether looper can reach your Mac on Wi-Fi."
        case .checking:
            return "Waiting for iOS to confirm Local Network access."
        case .available:
            return "looper can use the local network for the Mac connection."
        case .denied:
            return "iOS denied Local Network access for looper."
        case let .unavailable(reason):
            return reason
        }
    }

    var symbolName: String {
        switch self {
        case .notChecked:
            return "network"
        case .checking:
            return "network.badge.shield.half.filled"
        case .available:
            return "checkmark.circle.fill"
        case .denied:
            return "xmark.circle.fill"
        case .unavailable:
            return "exclamationmark.triangle.fill"
        }
    }

    var isToggleOn: Bool {
        self == .available
    }

    var canOpenAppSettings: Bool {
        switch self {
        case .denied, .unavailable:
            return true
        case .notChecked, .checking, .available:
            return false
        }
    }
}

@MainActor
@Observable
final class LocalNetworkAccessMonitor {
    private(set) var status: LocalNetworkAccessStatus = .notChecked

    var isChecking: Bool {
        status == .checking
    }

    func setAccessRequested(_ isRequested: Bool) async {
        if isRequested {
            await checkAccess()
            return
        }

        status = .notChecked
        openAppSettings()
    }

    func checkAccess() async {
        guard !isChecking else {
            return
        }

        status = .checking
        status = await LocalNetworkAccessProbe().check()
    }

    func openAppSettings() {
        guard let settingsURL = URL(string: UIApplication.openSettingsURLString) else {
            return
        }

        UIApplication.shared.open(settingsURL)
    }
}

private struct LocalNetworkAccessProbe: Sendable {
    func check() async -> LocalNetworkAccessStatus {
        await withCheckedContinuation { continuation in
            LocalNetworkAccessProbeSession(continuation: continuation).start()
        }
    }
}

private final class LocalNetworkAccessProbeSession: @unchecked Sendable {
    private let browser: NWBrowser
    private let queue = DispatchQueue(label: LocalNetworkAccessProbeConstants.queueLabel)
    private var continuation: CheckedContinuation<LocalNetworkAccessStatus, Never>?

    init(continuation: CheckedContinuation<LocalNetworkAccessStatus, Never>) {
        self.continuation = continuation
        browser = NWBrowser(
            for: .bonjour(
                type: LocalNetworkAccessProbeConstants.bonjourServiceType,
                domain: LocalNetworkAccessProbeConstants.bonjourDomain
            ),
            using: NWParameters()
        )
    }

    func start() {
        browser.stateUpdateHandler = { state in
            self.handleBrowserState(state)
        }
        browser.start(queue: queue)
        queue.asyncAfter(deadline: .now() + LocalNetworkAccessProbeConstants.timeoutSeconds) {
            self.finish(.unavailable(LocalNetworkAccessProbeConstants.timeoutMessage))
        }
    }

    private func handleBrowserState(_ state: NWBrowser.State) {
        switch state {
        case .ready:
            finish(.available)
        case let .failed(error):
            finish(status(for: error))
        case let .waiting(error):
            if isPolicyDenied(error) {
                finish(.denied)
            }
        case .setup, .cancelled:
            break
        @unknown default:
            break
        }
    }

    private func status(for error: NWError) -> LocalNetworkAccessStatus {
        if isPolicyDenied(error) {
            return .denied
        }

        return .unavailable(error.localizedDescription)
    }

    private func isPolicyDenied(_ error: NWError) -> Bool {
        switch error {
        case let .dns(errorCode):
            return errorCode == LocalNetworkAccessProbeConstants.policyDeniedErrorCode
        case .posix, .tls, .wifiAware:
            return false
        @unknown default:
            return false
        }
    }

    private func finish(_ status: LocalNetworkAccessStatus) {
        guard let continuation else {
            return
        }

        self.continuation = nil
        browser.stateUpdateHandler = nil
        browser.cancel()
        continuation.resume(returning: status)
    }
}

private enum LocalNetworkAccessProbeConstants {
    static let bonjourServiceType = "_looper._tcp"
    static let bonjourDomain = "local."
    static let policyDeniedErrorCode = DNSServiceErrorType(kDNSServiceErr_PolicyDenied)
    static let queueLabel = "dev.looper.local-network-access-probe"
    static let timeoutSeconds: TimeInterval = 8
    static let timeoutMessage = "iOS did not finish the Local Network permission check."
}

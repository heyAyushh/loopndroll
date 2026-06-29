import Foundation

public struct CompanionConnectionRoutePresentation: Equatable, Sendable {
    public let route: CompanionBaseURLRoute
    public let title: String
    public let detail: String
    public let systemImageName: String

    public var usesTailscaleLogo: Bool {
        route == .tailscale
    }

    public init?(
        baseURL: URL,
        tailscaleDetail: String? = nil
    ) {
        let route = CompanionBaseURLRouting.route(for: baseURL)
        guard route.isDisplayableConnectionRoute else {
            return nil
        }

        self.route = route
        title = route.presentationTitle
        detail = Self.routeDetail(
            for: route,
            baseURL: baseURL,
            tailscaleDetail: tailscaleDetail
        )
        systemImageName = route.presentationSystemImageName
    }

    private static func routeDetail(
        for route: CompanionBaseURLRoute,
        baseURL: URL,
        tailscaleDetail: String?
    ) -> String {
        if route == .tailscale,
           let tailscaleDetail = trimmedNonEmpty(tailscaleDetail) {
            return tailscaleDetail
        }

        return baseURL.host ?? baseURL.absoluteString
    }

    private static func trimmedNonEmpty(_ value: String?) -> String? {
        let trimmedValue = value?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedValue, !trimmedValue.isEmpty else {
            return nil
        }

        return trimmedValue
    }
}

public enum CompanionConnectionRoutePresentationSelection {
    public static func activeDisplayBaseURL(
        reachedBaseURL: URL?,
        configuredBaseURL: URL?,
        healthBaseURL: URL?,
        tailscaleHealthBaseURL: URL?,
        isTailscaleRunning: Bool
    ) -> URL? {
        firstDisplayableBaseURL(in: [
            displayableConfiguredBaseURL(configuredBaseURL),
            reachedBaseURL,
            healthBaseURL,
            isTailscaleRunning ? tailscaleHealthBaseURL : nil,
        ])
    }

    private static func displayableConfiguredBaseURL(
        _ baseURL: URL?
    ) -> URL? {
        guard let baseURL else {
            return nil
        }

        return baseURL
    }

    private static func firstDisplayableBaseURL(in baseURLs: [URL?]) -> URL? {
        baseURLs.compactMap(\.self).first { baseURL in
            CompanionBaseURLRouting.route(for: baseURL).isDisplayableConnectionRoute
        }
    }
}

private extension CompanionBaseURLRoute {
    var isDisplayableConnectionRoute: Bool {
        switch self {
        case .tailscale, .lan:
            return true
        case .remote, .loopback, .unsupported:
            return false
        }
    }

    var presentationTitle: String {
        switch self {
        case .remote:
            return "Unavailable"
        case .tailscale:
            return "Tailscale"
        case .lan:
            return "LAN"
        case .loopback, .unsupported:
            return "Unavailable"
        }
    }

    var presentationSystemImageName: String {
        switch self {
        case .remote:
            return "network.slash"
        case .tailscale:
            return "circle.grid.3x3.fill"
        case .lan:
            return "wifi.router"
        case .loopback, .unsupported:
            return "network.slash"
        }
    }
}

import Foundation

public enum CompanionConnectionRoutePreference: String, CaseIterable, Codable, Identifiable, Sendable {
    case remote
    case tailscale
    case lan

    public static let defaultPreference = Self.tailscale
    public static let allCases: [Self] = [.remote, .tailscale, .lan]

    public var id: String {
        rawValue
    }
}

public enum CompanionBaseURLRoute: Equatable, Sendable {
    case remote
    case tailscale
    case lan
    case loopback
    case unsupported
}

public enum CompanionBaseURLRouting {
    private static let httpScheme = "http"
    private static let httpsScheme = "https"
    private static let localHostnameSuffix = ".local"
    private static let tailscaleMagicDNSSuffix = ".ts.net"
    private static let tailscaleLegacyMagicDNSSuffix = ".beta.tailscale.net"
    private static let ipv4OctetCount = 4
    private static let ipv4OctetRange = 0...255
    private static let privateTenFirstOctet = 10
    private static let privateOneSevenTwoFirstOctet = 172
    private static let privateOneSevenTwoSecondOctetRange = 16...31
    private static let privateOneNineTwoFirstOctet = 192
    private static let privateOneNineTwoSecondOctet = 168
    private static let linkLocalFirstOctet = 169
    private static let linkLocalSecondOctet = 254
    private static let carrierGradeNatFirstOctet = 100
    private static let carrierGradeNatSecondOctetRange = 64...127
    private static let tailscaleIPv6Prefix = "fd7a:115c:a1e0:"
    private static let loopbackHosts: Set<String> = [
        "127.0.0.1",
        "0:0:0:0:0:0:0:1",
        "::1",
        "localhost",
    ]

    public static func route(for baseURL: URL) -> CompanionBaseURLRoute {
        guard let host = normalizedHost(for: baseURL) else {
            return .unsupported
        }

        if loopbackHosts.contains(host) {
            return .loopback
        }

        if isTailscaleHost(host) {
            return .tailscale
        }

        if isLANHost(host) {
            return .lan
        }

        guard baseURL.scheme?.lowercased() == httpScheme || baseURL.scheme?.lowercased() == httpsScheme else {
            return .unsupported
        }

        return .remote
    }

    public static func isAttemptableOnPhysicalDevice(_ baseURL: URL) -> Bool {
        switch route(for: baseURL) {
        case .loopback, .unsupported:
            return false
        case .remote:
            return baseURL.scheme?.lowercased() == httpsScheme
        case .tailscale, .lan:
            return baseURL.scheme?.lowercased() == httpScheme ||
                baseURL.scheme?.lowercased() == httpsScheme
        }
    }

    public static func sortedBaseURLs(
        _ baseURLs: [URL],
        preference: CompanionConnectionRoutePreference
    ) -> [URL] {
        baseURLs.enumerated()
            .sorted { lhs, rhs in
                let lhsPriority = priority(
                    for: route(for: lhs.element),
                    preference: preference
                )
                let rhsPriority = priority(
                    for: route(for: rhs.element),
                    preference: preference
                )

                guard lhsPriority != rhsPriority else {
                    return lhs.offset < rhs.offset
                }

                return lhsPriority < rhsPriority
            }
            .map(\.element)
    }

    private static func priority(
        for route: CompanionBaseURLRoute,
        preference: CompanionConnectionRoutePreference
    ) -> Int {
        switch preference {
        case .remote:
            return remotePriority(for: route)
        case .tailscale:
            return tailscalePriority(for: route)
        case .lan:
            return lanPriority(for: route)
        }
    }

    private static func remotePriority(for route: CompanionBaseURLRoute) -> Int {
        switch route {
        case .remote:
            return 0
        case .tailscale:
            return 1
        case .lan:
            return 2
        case .loopback:
            return 3
        case .unsupported:
            return 4
        }
    }

    private static func tailscalePriority(for route: CompanionBaseURLRoute) -> Int {
        switch route {
        case .tailscale:
            return 0
        case .lan:
            return 1
        case .remote:
            return 2
        case .loopback:
            return 3
        case .unsupported:
            return 4
        }
    }

    private static func lanPriority(for route: CompanionBaseURLRoute) -> Int {
        switch route {
        case .lan:
            return 0
        case .tailscale:
            return 1
        case .remote:
            return 2
        case .loopback:
            return 3
        case .unsupported:
            return 4
        }
    }

    private static func normalizedHost(for baseURL: URL) -> String? {
        guard let host = baseURL.host?.lowercased().trimmingCharacters(in: .whitespacesAndNewlines),
              !host.isEmpty
        else {
            return nil
        }

        return host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
    }

    private static func isTailscaleHost(_ host: String) -> Bool {
        isCarrierGradeNatAddress(host) ||
            isTailscaleIPv6Address(host) ||
            host.hasSuffix(tailscaleMagicDNSSuffix) ||
            host.hasSuffix(tailscaleLegacyMagicDNSSuffix)
    }

    private static func isLANHost(_ host: String) -> Bool {
        host.hasSuffix(localHostnameSuffix) || isPrivateLANAddress(host) || isLinkLocalAddress(host)
    }

    private static func isPrivateLANAddress(_ host: String) -> Bool {
        guard let octets = ipv4Octets(from: host) else {
            return false
        }

        return octets[0] == privateTenFirstOctet ||
            (
                octets[0] == privateOneSevenTwoFirstOctet &&
                    privateOneSevenTwoSecondOctetRange.contains(octets[1])
            ) ||
            (
                octets[0] == privateOneNineTwoFirstOctet &&
                    octets[1] == privateOneNineTwoSecondOctet
            )
    }

    private static func isLinkLocalAddress(_ host: String) -> Bool {
        guard let octets = ipv4Octets(from: host) else {
            return false
        }

        return octets[0] == linkLocalFirstOctet && octets[1] == linkLocalSecondOctet
    }

    private static func isCarrierGradeNatAddress(_ host: String) -> Bool {
        guard let octets = ipv4Octets(from: host) else {
            return false
        }

        return octets[0] == carrierGradeNatFirstOctet &&
            carrierGradeNatSecondOctetRange.contains(octets[1])
    }

    private static func isTailscaleIPv6Address(_ host: String) -> Bool {
        host.hasPrefix(tailscaleIPv6Prefix)
    }

    private static func ipv4Octets(from host: String) -> [Int]? {
        let parts = host.split(separator: ".")
        guard parts.count == ipv4OctetCount else {
            return nil
        }

        let octets = parts.compactMap { Int($0) }
        guard octets.count == ipv4OctetCount,
              octets.allSatisfy({ ipv4OctetRange.contains($0) })
        else {
            return nil
        }

        return octets
    }
}

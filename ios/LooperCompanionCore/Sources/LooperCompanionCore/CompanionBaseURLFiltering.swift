import Foundation

public enum CompanionBaseURLFiltering {
    private static let httpScheme = "http"
    private static let httpsScheme = "https"
    private static let localHostnameSuffix = ".local"
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
    private static let loopbackHosts: Set<String> = [
        "127.0.0.1",
        "0:0:0:0:0:0:0:1",
        "::1",
        "localhost",
    ]

    public static func uniqueAttemptableBaseURLs(_ baseURLs: [URL]) -> [URL] {
        var seen = Set<String>()
        let uniqueBaseURLs = baseURLs.filter { baseURL in
            shouldAttempt(baseURL) && seen.insert(CompanionBaseURLIdentity.key(for: baseURL)).inserted
        }
        return prioritizedBaseURLs(uniqueBaseURLs)
    }

    private static func prioritizedBaseURLs(_ baseURLs: [URL]) -> [URL] {
        #if targetEnvironment(simulator)
            return baseURLs.sorted { lhs, rhs in
                isLoopback(lhs) && !isLoopback(rhs)
            }
        #else
            return baseURLs
        #endif
    }

    private static func shouldAttempt(_ baseURL: URL) -> Bool {
        #if targetEnvironment(simulator)
            return true
        #else
            guard let host = baseURL.host?.lowercased() else {
                return false
            }

            guard !loopbackHosts.contains(host) else {
                return false
            }

            return supportsPhysicalDeviceTransport(scheme: baseURL.scheme, host: host)
        #endif
    }

    private static func supportsPhysicalDeviceTransport(scheme: String?, host: String) -> Bool {
        if scheme == httpsScheme {
            return true
        }

        guard scheme == httpScheme else {
            return false
        }

        return host.hasSuffix(localHostnameSuffix) || isLocalHTTPHost(host)
    }

    private static func isLocalHTTPHost(_ host: String) -> Bool {
        guard let octets = ipv4Octets(from: host) else {
            return false
        }

        return isPrivateTenAddress(octets) ||
            isPrivateOneSevenTwoAddress(octets) ||
            isPrivateOneNineTwoAddress(octets) ||
            isLinkLocalAddress(octets) ||
            isCarrierGradeNatAddress(octets)
    }

    private static func isPrivateTenAddress(_ octets: [Int]) -> Bool {
        octets[0] == privateTenFirstOctet
    }

    private static func isPrivateOneSevenTwoAddress(_ octets: [Int]) -> Bool {
        octets[0] == privateOneSevenTwoFirstOctet &&
            privateOneSevenTwoSecondOctetRange.contains(octets[1])
    }

    private static func isPrivateOneNineTwoAddress(_ octets: [Int]) -> Bool {
        octets[0] == privateOneNineTwoFirstOctet &&
            octets[1] == privateOneNineTwoSecondOctet
    }

    private static func isLinkLocalAddress(_ octets: [Int]) -> Bool {
        octets[0] == linkLocalFirstOctet &&
            octets[1] == linkLocalSecondOctet
    }

    private static func isCarrierGradeNatAddress(_ octets: [Int]) -> Bool {
        octets[0] == carrierGradeNatFirstOctet &&
            carrierGradeNatSecondOctetRange.contains(octets[1])
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

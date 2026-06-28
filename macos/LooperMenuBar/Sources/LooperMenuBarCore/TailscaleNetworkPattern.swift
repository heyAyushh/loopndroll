import Foundation

enum TailscaleNetworkPattern {
    private enum Constants {
        static let magicDNSSuffix = ".ts.net"
        static let legacyMagicDNSSuffix = ".beta.tailscale.net"
        static let ipv4OctetCount = 4
        static let ipv4OctetRange = 0...255
        static let ipv4FirstOctet = 100
        static let ipv4SecondOctetRange = 64...127
    }

    static func isTailscaleHost(_ host: String) -> Bool {
        let normalizedHost = normalizedHost(host)
        return normalizedHost.hasSuffix(Constants.magicDNSSuffix)
            || normalizedHost.hasSuffix(Constants.legacyMagicDNSSuffix)
            || isTailscaleIPv4Address(normalizedHost)
    }

    private static func normalizedHost(_ host: String) -> String {
        host
            .lowercased()
            .trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
    }

    private static func isTailscaleIPv4Address(_ host: String) -> Bool {
        guard let octets = ipv4Octets(from: host) else {
            return false
        }

        return octets[0] == Constants.ipv4FirstOctet
            && Constants.ipv4SecondOctetRange.contains(octets[1])
    }

    private static func ipv4Octets(from host: String) -> [Int]? {
        let octets = host
            .split(separator: ".", omittingEmptySubsequences: false)
            .compactMap { Int($0) }

        guard octets.count == Constants.ipv4OctetCount,
              octets.allSatisfy({ Constants.ipv4OctetRange.contains($0) })
        else {
            return nil
        }

        return octets
    }
}

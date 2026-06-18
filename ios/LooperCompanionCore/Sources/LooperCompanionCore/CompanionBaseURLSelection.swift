import Foundation

public enum CompanionBaseURLSelection {
    public static func mergedPreferredBaseURLs(
        reached: URL?,
        advertised: [URL],
        existing: [URL],
        preference: CompanionConnectionRoutePreference = .defaultPreference,
        preservingExistingPorts: Bool = false
    ) -> [URL] {
        let discoveredBaseURLs = [reached].compactMap(\.self) + advertised
        let compatibleDiscoveredBaseURLs = preservingExistingPorts ?
            discoveredBaseURLs.compatibleWithPorts(in: existing) :
            discoveredBaseURLs

        return preferredBaseURLs(
            uniqueBaseURLs(compatibleDiscoveredBaseURLs + existing),
            preference: preference
        )
    }

    public static func mergedCandidateBaseURLs(configured: [URL], discovered: [URL]) -> [URL] {
        uniqueBaseURLs(configured + discovered)
    }

    public static func preferredBaseURLs(
        _ baseURLs: [URL],
        preference: CompanionConnectionRoutePreference
    ) -> [URL] {
        CompanionBaseURLRouting.sortedBaseURLs(uniqueBaseURLs(baseURLs), preference: preference)
    }

    private static func uniqueBaseURLs(_ baseURLs: [URL]) -> [URL] {
        CompanionBaseURLIdentity.unique(baseURLs)
    }
}

private extension Array where Element == URL {
    func compatibleWithPorts(in configuredBaseURLs: [URL]) -> [URL] {
        let configuredPorts = Set(configuredBaseURLs.compactMap(\.companionNormalizedServerPort))
        guard !configuredPorts.isEmpty else {
            return self
        }

        return filter { baseURL in
            guard let port = baseURL.companionNormalizedServerPort else {
                return false
            }

            return configuredPorts.contains(port)
        }
    }
}

import Foundation

public enum CompanionBaseURLSelection {
    public static func mergedPreferredBaseURLs(
        reached: URL?,
        advertised: [URL],
        existing: [URL],
        preference: CompanionConnectionRoutePreference = .defaultPreference
    ) -> [URL] {
        preferredBaseURLs(
            uniqueBaseURLs([reached].compactMap(\.self) + advertised + existing),
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
        var seen = Set<String>()
        return baseURLs.filter { baseURL in
            seen.insert(CompanionBaseURLIdentity.key(for: baseURL)).inserted
        }
    }
}

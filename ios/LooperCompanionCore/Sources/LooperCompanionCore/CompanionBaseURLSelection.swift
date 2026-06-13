import Foundation

public enum CompanionBaseURLSelection {
    public static func mergedPreferredBaseURLs(
        reached: URL?,
        advertised: [URL],
        existing: [URL]
    ) -> [URL] {
        uniqueBaseURLs([reached].compactMap(\.self) + advertised + existing)
    }

    public static func mergedCandidateBaseURLs(configured: [URL], discovered: [URL]) -> [URL] {
        uniqueBaseURLs(configured + discovered)
    }

    private static func uniqueBaseURLs(_ baseURLs: [URL]) -> [URL] {
        var seen = Set<String>()
        return baseURLs.filter { baseURL in
            seen.insert(CompanionBaseURLIdentity.key(for: baseURL)).inserted
        }
    }
}

import Foundation

public enum MenuBarRealtimeEndpointResolver {
    public static func endpoints(
        controlPlaneBaseURL: URL,
        health: MobileHealthResponse?,
        preference: MobileRoutePreference
    ) -> [URL] {
        let healthEndpoints = health?.preferredRealtimeBaseURLs ?? []
        let localSeedEndpoint = MobileRouteURLPolicy.canonicalRealtimeGRPCBaseURL(
            for: controlPlaneBaseURL
        )
        return MobileRouteURLPolicy.sortedUniqueURLs(
            healthEndpoints + [localSeedEndpoint],
            preference: preference
        )
    }
}

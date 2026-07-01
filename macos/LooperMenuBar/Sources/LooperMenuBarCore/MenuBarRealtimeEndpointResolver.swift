import Foundation
import LooperClientCore

public enum MenuBarRealtimeEndpointResolver {
    public static func endpoints(
        controlPlaneBaseURL: URL,
        health: MobileHealthResponse?,
        preference: MobileRoutePreference
    ) -> [ClientEndpoint] {
        let recoveryBaseURLs = MobileRouteURLPolicy.sortedUniqueURLs(
            ([health?.baseURL].compactMap(\.self) + (health?.baseURLs ?? []))
                .compactMap(URL.init(string:)) +
                [controlPlaneBaseURL],
            preference: preference
        )
        let fallbackRecoveryBaseURL = recoveryBaseURLs.first?.absoluteString
            ?? controlPlaneBaseURL.absoluteString
        let h3EndpointURLs = MobileRouteURLPolicy.sortedUniqueURLs(
            health?.preferredH3RealtimeBaseURLs ?? [],
            preference: preference
        )
        let h3Endpoints = h3EndpointURLs
            .map {
                ClientEndpoint.h3(
                    url: $0.absoluteString,
                    recoveryBaseURL: recoveryBaseURL(
                        for: $0,
                        candidates: recoveryBaseURLs,
                        fallback: fallbackRecoveryBaseURL
                    ),
                    certificateSha256: health?.grpcH3CertificateSha256
                )
            }
        let healthEndpoints = health?.preferredRealtimeBaseURLs ?? []
        let localSeedEndpoint = MobileRouteURLPolicy.canonicalRealtimeGRPCBaseURL(
            for: controlPlaneBaseURL
        )
        let h2Endpoints = MobileRouteURLPolicy.sortedUniqueURLs(
            healthEndpoints + [localSeedEndpoint],
            preference: preference
        )
        .map {
            ClientEndpoint.h2(
                url: $0.absoluteString,
                recoveryBaseURL: MobileRouteURLPolicy
                    .canonicalHTTPAPIBaseURL(for: $0)
                    .absoluteString
            )
        }
        return h3Endpoints + h2Endpoints
    }

    private static func recoveryBaseURL(
        for endpointURL: URL,
        candidates: [URL],
        fallback: String
    ) -> String {
        let endpointHost = endpointURL.host?.lowercased()
        return candidates
            .first { $0.host?.lowercased() == endpointHost }?
            .absoluteString ?? fallback
    }
}

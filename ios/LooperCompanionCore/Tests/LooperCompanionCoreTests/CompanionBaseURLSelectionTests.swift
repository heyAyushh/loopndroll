import Foundation
import Testing
@testable import LooperCompanionCore

@Suite("Companion base URL selection")
struct CompanionBaseURLSelectionTests {
    private let reachedBonjourURL = "http://looper.local:8765"
    private let advertisedLANURL = "http://192.0.2.10:8765"
    private let advertisedRemoteURL = "https://looper.example.test"
    private let staleConfiguredURL = "http://198.51.100.20:8765"
    private let normalizedReachedURL = "http://LOOPER.local:8765/"
    private let normalizedAdvertisedURL = "http://looper.local:8765"

    @Test("Reached and advertised URLs replace stale configured primary")
    func reachedAndAdvertisedURLsReplaceStaleConfiguredPrimary() throws {
        let urls = CompanionBaseURLSelection.mergedPreferredBaseURLs(
            reached: try url(reachedBonjourURL),
            advertised: try urls(advertisedLANURL, advertisedRemoteURL),
            existing: try urls(
                staleConfiguredURL,
                advertisedRemoteURL
            )
        )

        #expect(urls.map(\.absoluteString) == [
            reachedBonjourURL,
            advertisedLANURL,
            advertisedRemoteURL,
            staleConfiguredURL,
        ])
    }

    @Test("Candidate merge keeps configured order before discovery")
    func candidateMergeKeepsConfiguredOrderBeforeDiscovery() throws {
        let urls = CompanionBaseURLSelection.mergedCandidateBaseURLs(
            configured: try urls(staleConfiguredURL, advertisedRemoteURL),
            discovered: try urls(reachedBonjourURL, advertisedRemoteURL)
        )

        #expect(urls.map(\.absoluteString) == [
            staleConfiguredURL,
            advertisedRemoteURL,
            reachedBonjourURL,
        ])
    }

    @Test("Selection deduplicates normalized equivalent base URLs")
    func selectionDeduplicatesNormalizedEquivalentBaseURLs() throws {
        let urls = CompanionBaseURLSelection.mergedPreferredBaseURLs(
            reached: try url(normalizedReachedURL),
            advertised: try urls(normalizedAdvertisedURL),
            existing: try urls(normalizedAdvertisedURL)
        )

        #expect(urls.count == 1)
        #expect(urls.first?.host?.lowercased() == "looper.local")
    }

    @Test("Candidate merge deduplicates normalized discovery duplicates")
    func candidateMergeDeduplicatesNormalizedDiscoveryDuplicates() throws {
        let urls = CompanionBaseURLSelection.mergedCandidateBaseURLs(
            configured: try urls(normalizedAdvertisedURL),
            discovered: try urls(normalizedReachedURL)
        )

        #expect(urls.count == 1)
        #expect(urls.first?.absoluteString == normalizedAdvertisedURL)
    }

    private func urls(_ values: String...) throws -> [URL] {
        try values.map { value in
            try url(value)
        }
    }

    private func url(_ value: String) throws -> URL {
        try #require(URL(string: value))
    }
}

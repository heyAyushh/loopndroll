import Foundation
import Testing
@testable import LooperCompanionCore

@Suite("Companion base URL filtering")
struct CompanionBaseURLFilteringTests {
    private let carrierGradeNatLowerBoundURL = "http://100.64.0.1:8765"
    private let carrierGradeNatMidRangeURL = "http://100.100.0.1:8765"
    private let carrierGradeNatUpperBoundURL = "http://100.127.255.254:8765"
    private let tailscaleHTTPSURL = "https://100.119.200.69:8781"
    private let publicHTTPURLNearCarrierGradeNatRange = "http://100.128.0.1:8765"
    private let publicHTTPURL = "http://203.0.113.10:8765"
    private let publicHTTPSURL = "https://203.0.113.10:8765"
    private let loopbackHTTPURL = "http://127.0.0.1:8765"
    private let localhostHTTPURL = "http://localhost:8765"
    private let privateTenURL = "http://10.0.0.1:8765"
    private let privateOneSevenTwoLowerBoundURL = "http://172.16.0.1:8765"
    private let privateOneSevenTwoUpperBoundURL = "http://172.31.255.254:8765"
    private let privateOneNineTwoURL = "http://192.168.0.10:8765"
    private let linkLocalURL = "http://169.254.1.1:8765"
    private let bonjourURL = "http://looper.local:8765"
    private let duplicateBonjourURL = "http://LOOPER.local:8765/"

    @Test("Physical policy permits Tailscale CGNAT HTTP URLs")
    func physicalPolicyPermitsTailscaleCGNATHTTPURLs() throws {
        let urls = try [
            carrierGradeNatLowerBoundURL,
            carrierGradeNatMidRangeURL,
            carrierGradeNatUpperBoundURL
        ].map { value in
            try #require(URL(string: value))
        }

        #expect(urls.allSatisfy(CompanionBaseURLRouting.isAttemptableOnPhysicalDevice))
    }

    @Test("Physical device permits Tailscale CGNAT HTTP URLs")
    func physicalDevicePermitsTailscaleCGNATHTTPURLs() throws {
        #if targetEnvironment(simulator)
            #expect(true)
        #else
            let urls = try attemptableURLs(
                carrierGradeNatLowerBoundURL,
                carrierGradeNatMidRangeURL,
                carrierGradeNatUpperBoundURL
            )

            #expect(urls.map(\.absoluteString) == [
                carrierGradeNatLowerBoundURL,
                carrierGradeNatMidRangeURL,
                carrierGradeNatUpperBoundURL
            ])
        #endif
    }

    @Test("Physical device permits Tailscale HTTPS URLs")
    func physicalDevicePermitsTailscaleHTTPSURLs() throws {
        let urls = try attemptableURLs(tailscaleHTTPSURL)

        #expect(urls.map(\.absoluteString) == [tailscaleHTTPSURL])
    }

    @Test("Physical device rejects public HTTP and loopback URLs")
    func physicalDeviceRejectsPublicHTTPAndLoopbackURLs() throws {
        #if targetEnvironment(simulator)
            #expect(true)
        #else
            let urls = try attemptableURLs(
                publicHTTPURLNearCarrierGradeNatRange,
                publicHTTPURL,
                loopbackHTTPURL,
                localhostHTTPURL
            )

            #expect(urls.isEmpty)
        #endif
    }

    @Test("Physical device keeps existing local HTTP ranges")
    func physicalDeviceKeepsExistingLocalHTTPRanges() throws {
        #if targetEnvironment(simulator)
            #expect(true)
        #else
            let urls = try attemptableURLs(
                privateTenURL,
                privateOneSevenTwoLowerBoundURL,
                privateOneSevenTwoUpperBoundURL,
                privateOneNineTwoURL,
                linkLocalURL,
                bonjourURL
            )

            #expect(urls.count == 6)
        #endif
    }

    @Test("Public HTTPS URLs are not route candidates")
    func publicHTTPSURLsAreNotRouteCandidates() throws {
        let urls = try attemptableURLs(publicHTTPSURL)

        #expect(urls.isEmpty)
    }

    @Test("Filtering deduplicates normalized equivalent base URLs")
    func filteringDeduplicatesNormalizedEquivalentBaseURLs() throws {
        let urls = try attemptableURLs(bonjourURL, duplicateBonjourURL)

        #expect(urls.count == 1)
        #expect(urls.first?.absoluteString == bonjourURL)
    }

    private func attemptableURLs(_ values: String...) throws -> [URL] {
        let urls = try values.map { value in
            try #require(URL(string: value))
        }
        return CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(urls)
    }
}

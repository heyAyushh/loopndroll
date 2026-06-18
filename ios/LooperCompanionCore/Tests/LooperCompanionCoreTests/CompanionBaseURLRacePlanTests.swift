import Foundation
import Testing
@testable import LooperCompanionCore

@Suite("Companion base URL race plan")
struct CompanionBaseURLRacePlanTests {
    private let preferredLANURL = "http://192.168.1.26:8765"
    private let duplicatePreferredLANURL = "http://192.168.1.26:8765/"
    private let fallbackLANURL = "http://192.168.1.26:8781"
    private let fallbackDelay: Duration = .milliseconds(125)

    @Test("Race plan starts the preferred URL immediately and defers fallbacks")
    func racePlanStartsPreferredURLImmediatelyAndDefersFallbacks() throws {
        let candidates = CompanionBaseURLRacePlan.candidates(
            for: try urls(preferredLANURL, fallbackLANURL),
            fallbackDelay: fallbackDelay
        )

        #expect(candidates.map(\.baseURL.absoluteString) == [
            preferredLANURL,
            fallbackLANURL,
        ])
        #expect(candidates.map(\.delay) == [
            .zero,
            fallbackDelay,
        ])
    }

    @Test("Race plan deduplicates before assigning delays")
    func racePlanDeduplicatesBeforeAssigningDelays() throws {
        let candidates = CompanionBaseURLRacePlan.candidates(
            for: try urls(preferredLANURL, duplicatePreferredLANURL, fallbackLANURL),
            fallbackDelay: fallbackDelay
        )

        #expect(candidates.map(\.baseURL.absoluteString) == [
            preferredLANURL,
            fallbackLANURL,
        ])
        #expect(candidates.map(\.delay) == [
            .zero,
            fallbackDelay,
        ])
    }

    private func urls(_ values: String...) throws -> [URL] {
        try values.map { value in
            try #require(URL(string: value))
        }
    }
}

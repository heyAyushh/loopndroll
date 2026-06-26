import Foundation
import Testing
@testable import LooperCompanionCore

@Suite("Looper session freshness")
struct LooperSessionFreshnessTests {
    @Test("Display timestamp preserves raw activity string")
    func displayTimestampPreservesRawActivityString() {
        #expect(
            LooperSessionFreshness.displayTimestamp(
                lastActivityAt: "2026-06-16T08:02:00Z"
            ) == "2026-06-16T08:02:00Z"
        )
        #expect(LooperSessionFreshness.displayPrefix() == "active")
    }

    @Test("Display date parses fractional seconds")
    func displayDateParsesFractionalSeconds() {
        let wholeSecondDate = LooperSessionFreshness.displayDate(
            lastActivityAt: "2026-06-16T08:02:00Z"
        )
        let fractionalDate = LooperSessionFreshness.displayDate(
            lastActivityAt: "2026-06-16T08:02:00.250Z"
        )

        #expect(wholeSecondDate != nil)
        #expect(fractionalDate != nil)
        #expect(fractionalDate! > wholeSecondDate!)
    }

    @Test("Invalid display date returns nil")
    func invalidDisplayDateReturnsNil() {
        #expect(
            LooperSessionFreshness.displayDate(lastActivityAt: "not-a-date") == nil
        )
    }
}

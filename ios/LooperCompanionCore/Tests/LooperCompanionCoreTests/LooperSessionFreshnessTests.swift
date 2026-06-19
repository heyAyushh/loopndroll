import Foundation
import Testing
@testable import LooperCompanionCore

@Suite("Looper session freshness")
struct LooperSessionFreshnessTests {
    @Test("Activity ordering ignores older message freshness")
    func activityOrderingIgnoresOlderMessageFreshness() {
        let activeSessionSortsFirst = LooperSessionFreshness.isNewerActivityOrLowerReference(
            leftLastActivityAt: "2026-06-16T08:02:00Z",
            leftRef: "S2",
            rightLastActivityAt: "2026-06-16T08:01:30Z",
            rightRef: "S1"
        )

        #expect(activeSessionSortsFirst)
        #expect(
            LooperSessionFreshness.displayTimestamp(
                lastActivityAt: "2026-06-16T08:02:00Z"
            ) == "2026-06-16T08:02:00Z"
        )
        #expect(LooperSessionFreshness.displayPrefix() == "active")
    }

    @Test("Activity ordering parses fractional seconds before string fallback")
    func activityOrderingParsesFractionalSeconds() {
        let fractionalActivitySortsFirst = LooperSessionFreshness.isNewerActivityOrLowerReference(
            leftLastActivityAt: "2026-06-16T08:02:00.250Z",
            leftRef: "S2",
            rightLastActivityAt: "2026-06-16T08:02:00Z",
            rightRef: "S1"
        )

        #expect(fractionalActivitySortsFirst)
    }

    @Test("Precomputed activity sort keys preserve freshness ordering")
    func activitySortKeysPreserveFreshnessOrdering() {
        let fractionalActivityKey = LooperSessionFreshness.activitySortKey(
            lastActivityAt: "2026-06-16T08:02:00.250Z",
            ref: "S2"
        )
        let wholeSecondActivityKey = LooperSessionFreshness.activitySortKey(
            lastActivityAt: "2026-06-16T08:02:00Z",
            ref: "S1"
        )

        #expect(
            LooperSessionFreshness.isNewerActivityOrLowerReference(
                leftKey: fractionalActivityKey,
                rightKey: wholeSecondActivityKey
            )
        )
    }

    @Test("Activity ordering falls back to lower reference on ties")
    func activityOrderingFallsBackToLowerReference() {
        let lowerRefSortsFirst = LooperSessionFreshness.isNewerActivityOrLowerReference(
            leftLastActivityAt: "2026-06-16T08:02:00Z",
            leftRef: "S1",
            rightLastActivityAt: "2026-06-16T08:02:00Z",
            rightRef: "S2"
        )

        #expect(lowerRefSortsFirst)
    }
}

import Foundation

enum CompanionRealtimeSync {
    static let snapshotRefreshDebounce: Duration = .milliseconds(350)
    static let snapshotRevisionChangedDetail = "snapshot-revision-changed"

    static func shouldRefreshSnapshot(
        for event: MobileStreamEvent,
        currentRevision: String?,
        hasSnapshot: Bool
    ) -> Bool {
        if event.eventType == .connected {
            return !hasSnapshot
        }

        guard let revision = event.revision else {
            return true
        }

        return revision != currentRevision
    }

    static func normalizedRevision(_ revision: String?) -> String? {
        let trimmedRevision = revision?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedRevision, !trimmedRevision.isEmpty else {
            return nil
        }
        return trimmedRevision
    }
}

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
            guard let revision = normalizedRevision(event.revision) else {
                return !hasSnapshot
            }

            return !hasSnapshot || revision != normalizedRevision(currentRevision)
        }

        guard let revision = normalizedRevision(event.revision) else {
            return true
        }

        return revision != normalizedRevision(currentRevision)
    }

    static func normalizedRevision(_ revision: String?) -> String? {
        let trimmedRevision = revision?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedRevision, !trimmedRevision.isEmpty else {
            return nil
        }
        return trimmedRevision
    }

    static func revisionForRealtimeGate(
        currentRevision: String?,
        hasValidatedSnapshotWithHTTP: Bool
    ) -> String? {
        guard hasValidatedSnapshotWithHTTP else {
            return nil
        }

        return normalizedRevision(currentRevision)
    }

    static func hasRealtimeValidatedSnapshot(
        hasSnapshot: Bool,
        hasValidatedSnapshotWithHTTP: Bool
    ) -> Bool {
        hasSnapshot && hasValidatedSnapshotWithHTTP
    }
}

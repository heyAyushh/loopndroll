import Foundation

public enum LooperSessionFreshness {
    public struct ActivitySortKey: Sendable {
        fileprivate let date: Date?
        fileprivate let lastActivityAt: String
        fileprivate let ref: String
    }

    private static let parser = LockedISO8601DateParser()

    public static func displayTimestamp(lastActivityAt: String) -> String {
        lastActivityAt
    }

    public static func displayPrefix() -> String {
        "active"
    }

    public static func displayDate(lastActivityAt: String) -> Date? {
        date(from: lastActivityAt)
    }

    public static func date(from value: String) -> Date? {
        parser.date(from: value)
    }

    public static func activitySortKey(
        lastActivityAt: String,
        ref: String
    ) -> ActivitySortKey {
        ActivitySortKey(
            date: date(from: lastActivityAt),
            lastActivityAt: lastActivityAt,
            ref: ref
        )
    }

    public static func isNewerActivityOrLowerReference(
        leftLastActivityAt: String,
        leftRef: String,
        rightLastActivityAt: String,
        rightRef: String
    ) -> Bool {
        isNewerActivityOrLowerReference(
            leftKey: activitySortKey(lastActivityAt: leftLastActivityAt, ref: leftRef),
            rightKey: activitySortKey(lastActivityAt: rightLastActivityAt, ref: rightRef)
        )
    }

    public static func isNewerActivityOrLowerReference(
        leftKey: ActivitySortKey,
        rightKey: ActivitySortKey
    ) -> Bool {
        if let leftDate = leftKey.date,
           let rightDate = rightKey.date,
           leftDate != rightDate
        {
            return leftDate > rightDate
        }

        if leftKey.lastActivityAt != rightKey.lastActivityAt {
            return leftKey.lastActivityAt > rightKey.lastActivityAt
        }

        return leftKey.ref < rightKey.ref
    }
}

private final class LockedISO8601DateParser: @unchecked Sendable {
    private let lock = NSLock()
    private let fractionalFormatter = ISO8601DateFormatter()
    private let wholeSecondFormatter = ISO8601DateFormatter()

    init() {
        fractionalFormatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        wholeSecondFormatter.formatOptions = [.withInternetDateTime]
    }

    func date(from value: String) -> Date? {
        lock.lock()
        defer {
            lock.unlock()
        }

        return fractionalFormatter.date(from: value) ??
            wholeSecondFormatter.date(from: value)
    }
}

import Foundation

public enum LooperSessionFreshness {
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
        iso8601Formatter(formatOptions: [.withInternetDateTime, .withFractionalSeconds])
            .date(from: value) ??
            iso8601Formatter(formatOptions: [.withInternetDateTime])
            .date(from: value)
    }

    public static func isNewerActivityOrLowerReference(
        leftLastActivityAt: String,
        leftRef: String,
        rightLastActivityAt: String,
        rightRef: String
    ) -> Bool {
        if let leftDate = date(from: leftLastActivityAt),
           let rightDate = date(from: rightLastActivityAt),
           leftDate != rightDate
        {
            return leftDate > rightDate
        }

        if leftLastActivityAt != rightLastActivityAt {
            return leftLastActivityAt > rightLastActivityAt
        }

        return leftRef < rightRef
    }

    private static func iso8601Formatter(
        formatOptions: ISO8601DateFormatter.Options
    ) -> ISO8601DateFormatter {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = formatOptions
        return formatter
    }
}

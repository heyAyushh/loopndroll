import Foundation

public enum LooperSessionFreshness {
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

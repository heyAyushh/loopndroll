import Foundation

public enum LooperHandoffHoldDuration: String, CaseIterable, Identifiable, Sendable {
    case thirtySeconds
    case oneMinute
    case twoMinutes
    case fiveMinutes
    case tenMinutes

    private enum Timing {
        static let secondsPerMinute: TimeInterval = 60
        static let thirtySeconds: TimeInterval = 30
        static let oneMinute: TimeInterval = 1
        static let twoMinutes: TimeInterval = 2
        static let fiveMinutes: TimeInterval = 5
        static let tenMinutes: TimeInterval = 10
    }

    public static let userDefaultsKey = "handoffHoldDuration"
    public static let defaultOption: LooperHandoffHoldDuration = .twoMinutes
    public static let allOptions: [LooperHandoffHoldDuration] = [
        .thirtySeconds,
        .oneMinute,
        .twoMinutes,
        .fiveMinutes,
        .tenMinutes,
    ]

    public var id: String {
        rawValue
    }

    public var durationSeconds: TimeInterval {
        switch self {
        case .thirtySeconds:
            Timing.thirtySeconds
        case .oneMinute:
            Timing.oneMinute * Timing.secondsPerMinute
        case .twoMinutes:
            Timing.twoMinutes * Timing.secondsPerMinute
        case .fiveMinutes:
            Timing.fiveMinutes * Timing.secondsPerMinute
        case .tenMinutes:
            Timing.tenMinutes * Timing.secondsPerMinute
        }
    }

    public var menuTitle: String {
        switch self {
        case .thirtySeconds:
            "30 sec"
        case .oneMinute:
            "1 min"
        case .twoMinutes:
            "2 min"
        case .fiveMinutes:
            "5 min"
        case .tenMinutes:
            "10 min"
        }
    }

    public static func stored(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) -> LooperHandoffHoldDuration {
        guard let storedValue = userDefaults.string(forKey: key),
              let option = LooperHandoffHoldDuration(rawValue: storedValue)
        else {
            return defaultOption
        }

        return option
    }

    public func save(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) {
        userDefaults.set(rawValue, forKey: key)
    }
}

public struct LooperHandoffActivationLease: Sendable {
    public private(set) var expiresAt: Date?

    public init(expiresAt: Date? = nil) {
        self.expiresAt = expiresAt
    }

    public mutating func activate(
        now: Date = Date(),
        holdDuration: LooperHandoffHoldDuration
    ) {
        activate(now: now, durationSeconds: holdDuration.durationSeconds)
    }

    public mutating func activate(
        now: Date = Date(),
        durationSeconds: TimeInterval
    ) {
        guard durationSeconds > .zero else {
            invalidate()
            return
        }

        let nextExpiration = now.addingTimeInterval(durationSeconds)
        if let expiresAt, expiresAt > nextExpiration {
            return
        }

        expiresAt = nextExpiration
    }

    public mutating func isActive(now: Date = Date()) -> Bool {
        guard let expiresAt else {
            return false
        }

        guard expiresAt > now else {
            invalidate()
            return false
        }

        return true
    }

    public mutating func invalidate() {
        expiresAt = nil
    }
}

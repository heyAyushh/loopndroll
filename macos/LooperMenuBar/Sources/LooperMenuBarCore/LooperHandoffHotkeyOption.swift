import Foundation

public enum LooperHandoffHotkeyOption: String, CaseIterable, Identifiable, Sendable {
    case commandL
    case commandShiftL
    case commandOptionL
    case controlL
    case disabled

    public static let userDefaultsKey = "handoffHotkeyOption"
    public static let legacyRightNowUserDefaultsKey = "rightNowHotkeyOption"
    public static let defaultOption: LooperHandoffHotkeyOption = .commandL
    public static let allOptions = [commandL, commandShiftL, commandOptionL, controlL, disabled]

    public var id: String {
        rawValue
    }

    public var menuTitle: String {
        switch self {
        case .commandL:
            "⌘L"
        case .commandShiftL:
            "⌘⇧L"
        case .commandOptionL:
            "⌘⌥L"
        case .controlL:
            "⌃L"
        case .disabled:
            "Disabled"
        }
    }

    public var isEnabled: Bool {
        self != .disabled
    }

    public static func stored(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey,
        legacyKey: String = legacyRightNowUserDefaultsKey
    ) -> LooperHandoffHotkeyOption {
        if let storedValue = userDefaults.string(forKey: key),
           let option = LooperHandoffHotkeyOption(rawValue: storedValue)
        {
            return option
        }

        if let legacyValue = userDefaults.string(forKey: legacyKey),
           let option = LooperHandoffHotkeyOption(rawValue: legacyValue)
        {
            return option
        }

        return defaultOption
    }

    public static func migrateStoredPreference(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey,
        legacyKey: String = legacyRightNowUserDefaultsKey
    ) {
        guard userDefaults.object(forKey: legacyKey) != nil else {
            return
        }

        defer {
            userDefaults.removeObject(forKey: legacyKey)
        }

        guard userDefaults.object(forKey: key) == nil,
              let legacyValue = userDefaults.string(forKey: legacyKey),
              LooperHandoffHotkeyOption(rawValue: legacyValue) != nil
        else {
            return
        }

        userDefaults.set(legacyValue, forKey: key)
    }

    public func save(
        in userDefaults: UserDefaults = .standard,
        key: String = LooperHandoffHotkeyOption.userDefaultsKey,
        legacyKey: String = LooperHandoffHotkeyOption.legacyRightNowUserDefaultsKey
    ) {
        userDefaults.set(rawValue, forKey: key)
        userDefaults.removeObject(forKey: legacyKey)
    }
}

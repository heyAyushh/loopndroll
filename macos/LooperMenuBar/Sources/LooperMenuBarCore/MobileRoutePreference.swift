import Foundation

public enum MobileRoutePreference: String, CaseIterable, Identifiable, Sendable {
    case tailscale
    case lan

    /// Raw value previously stored for a "Remote first" option, retired when
    /// LAN and Tailscale became the only user-visible routes. Stored prefs
    /// with this legacy value migrate to `.tailscale` on read.
    private static let legacyRemoteRawValue = "remote"

    public static let userDefaultsKey = "mobileRoutePreference"
    public static let defaultOption: MobileRoutePreference = .tailscale
    public static let allOptions: [MobileRoutePreference] = [.tailscale, .lan]

    public var id: String {
        rawValue
    }

    public var menuTitle: String {
        switch self {
        case .tailscale:
            "Tailscale first"
        case .lan:
            "LAN first"
        }
    }

    public static func stored(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) -> MobileRoutePreference {
        guard let storedValue = userDefaults.string(forKey: key) else {
            return defaultOption
        }
        guard storedValue != legacyRemoteRawValue else {
            return .tailscale
        }

        return MobileRoutePreference(rawValue: storedValue) ?? defaultOption
    }

    public func save(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) {
        userDefaults.set(rawValue, forKey: key)
    }
}

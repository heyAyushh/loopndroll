import Foundation

public enum MobileRoutePreference: String, CaseIterable, Identifiable, Sendable {
    case remote
    case tailscale
    case lan

    public static let userDefaultsKey = "mobileRoutePreference"
    public static let defaultOption: MobileRoutePreference = .tailscale
    public static let allOptions: [MobileRoutePreference] = [.remote, .tailscale, .lan]

    public var id: String {
        rawValue
    }

    public var menuTitle: String {
        switch self {
        case .remote:
            "Remote"
        case .tailscale:
            "Tailscale"
        case .lan:
            "LAN"
        }
    }

    public static func stored(
        in userDefaults: UserDefaults = .standard,
        key: String = userDefaultsKey
    ) -> MobileRoutePreference {
        guard let storedValue = userDefaults.string(forKey: key),
              let option = MobileRoutePreference(rawValue: storedValue)
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

import Foundation
import Security

struct CompanionMobileSession: Codable, Sendable {
    let sessionId: String
    let sessionToken: String
    let expiresAt: String

    var headerValue: String {
        "\(sessionId).\(sessionToken)"
    }

    var isExpired: Bool {
        guard let expiresAtDate = Self.parseServerTimestamp(expiresAt) else {
            return true
        }

        return expiresAtDate <= Date().addingTimeInterval(Self.expirySkewSeconds)
    }

    // The server formats RFC 3339 with fractional seconds (e.g.
    // "2026-07-02T08:05:46.280272Z"); a plain ISO8601DateFormatter returns nil for
    // that, which used to make every freshly minted session read as expired and get
    // deleted — locking the app out of its own auth.
    private static func parseServerTimestamp(_ value: String) -> Date? {
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let date = fractional.date(from: value) {
            return date
        }
        return ISO8601DateFormatter().date(from: value)
    }

    private static let expirySkewSeconds: TimeInterval = 30
}

enum CompanionMobileSessionStore {
    private static let service = "dev.looper.companion"
    private static let account = "mobile-api-passkey-session"

    static func store(_ session: CompanionMobileSession) {
        clear()

        guard let data = try? JSONEncoder().encode(session) else {
            return
        }

        var item = keychainQuery(returnData: false)
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    static func loadValidHeaderValue() -> String? {
        #if DEBUG
        if let liveMobileSession = UITestLaunchArguments.liveMobileSession {
            return liveMobileSession
        }
        #endif

        guard let session = load() else {
            return nil
        }

        guard !session.isExpired else {
            clear()
            return nil
        }

        return session.headerValue
    }

    static func clear() {
        SecItemDelete(keychainQuery(returnData: false) as CFDictionary)
    }

    private static func load() -> CompanionMobileSession? {
        let query = keychainQuery(returnData: true)
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess, let data = item as? Data else {
            return nil
        }

        return try? JSONDecoder().decode(CompanionMobileSession.self, from: data)
    }

    private static func keychainQuery(returnData: Bool) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]

        if returnData {
            query[kSecReturnData as String] = true
            query[kSecMatchLimit as String] = kSecMatchLimitOne
        }

        return query
    }
}

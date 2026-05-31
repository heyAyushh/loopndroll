import Foundation
import Security

struct CompanionConnection: Sendable {
    let baseURLs: [URL]
    let bearerToken: String?

    var baseURLString: String {
        baseURLs.first?.absoluteString ?? ""
    }

    var storageValue: String {
        baseURLs.map(\.absoluteString).joined(separator: "\n")
    }
}

enum CompanionConfiguration {
    static let apiBaseURLOverrideKey = "looper.apiBaseURLOverride"
    private static let apiBearerTokenService = "dev.looper.companion"
    private static let apiBearerTokenAccount = "mobile-api-bearer-token"
    private static let supportedConnectionCodeSchemes = ["looper", "loopndroll"]
    private static let supportedConnectionCodeKeys = ["baseURL", "base_url", "url"]
    private static let supportedConnectionCodeListKeys = ["baseURLs", "base_urls", "urls"]
    private static let supportedPairingTokenIDKeys = ["pairingTokenId", "pairing_token_id", "tokenId"]
    private static let supportedPairingTokenKeys = ["pairingToken", "pairing_token", "token"]

    static func resolvedBaseURLString() -> String {
        resolvedConnection().baseURLString
    }

    static func resolvedBaseURLStrings() -> [URL] {
        resolvedConnection().baseURLs
    }

    static func resolvedConnection() -> CompanionConnection {
        let storedValue = UserDefaults.standard.string(forKey: apiBaseURLOverrideKey) ?? ""
        let storedURLs = normalizedBaseURLs(from: storedValue)
        if !storedURLs.isEmpty {
            return CompanionConnection(baseURLs: storedURLs, bearerToken: loadBearerToken())
        }

        let bundledValue = (
            Bundle.main.object(forInfoDictionaryKey: "LOOPER_API_BASE_URL")
                ?? Bundle.main.object(forInfoDictionaryKey: "LOOPNDROLL_API_BASE_URL")
        ) as? String
        let bundledListValue = Bundle.main.object(forInfoDictionaryKey: "LOOPER_API_BASE_URLS") as? String
        let bundledURLs = normalizedBaseURLs(
            from: [bundledValue, bundledListValue]
                .compactMap(\.self)
                .joined(separator: "\n")
        )
        let bundledBearerToken = Bundle.main.object(
            forInfoDictionaryKey: "LOOPER_API_BEARER_TOKEN"
        ) as? String

        if !bundledURLs.isEmpty {
            return CompanionConnection(
                baseURLs: bundledURLs,
                bearerToken: nonEmptyString(bundledBearerToken)
            )
        }

        #if targetEnvironment(simulator)
            return CompanionConnection(
                baseURLs: [URL(string: "http://127.0.0.1:8787")].compactMap(\.self),
                bearerToken: nil
            )
        #else
            return CompanionConnection(baseURLs: [], bearerToken: nil)
        #endif
    }

    static func resolveBaseURLString(fromConnectionCode connectionCode: String) throws -> String {
        try resolveConnection(fromConnectionCode: connectionCode).baseURLs.map(\.absoluteString)
            .joined(separator: "\n")
    }

    static func resolveBaseURLStrings(fromConnectionCode connectionCode: String) throws -> [URL] {
        try resolveConnection(fromConnectionCode: connectionCode).baseURLs
    }

    static func resolveConnection(fromConnectionCode connectionCode: String) throws -> CompanionConnection {
        try resolveConnection(fromConnectionCode: connectionCode, remainingDepth: 2)
    }

    static func storeConnection(_ connection: CompanionConnection) {
        storeBaseURLString(connection.storageValue)
        storeBearerToken(connection.bearerToken)
    }

    static func normalizedBaseURLsForUserInput(_ value: String) -> [URL] {
        normalizedBaseURLs(from: value)
    }

    static func storeBaseURLString(_ value: String) {
        let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmedValue.isEmpty {
            UserDefaults.standard.removeObject(forKey: apiBaseURLOverrideKey)
            return
        }

        UserDefaults.standard.set(trimmedValue, forKey: apiBaseURLOverrideKey)
    }

    static func currentRelease() -> CompanionRelease {
        CompanionRelease(
            marketingVersion: (
                Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString")
                    as? String
            )?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "1.0",
            buildNumber: (
                Bundle.main.object(forInfoDictionaryKey: kCFBundleVersionKey as String)
                    as? String
            )?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "1",
            releaseChannel: (
                Bundle.main.object(forInfoDictionaryKey: "LOOPER_RELEASE_CHANNEL")
                    as? String
            )?.trimmingCharacters(in: .whitespacesAndNewlines),
            branchName: (
                Bundle.main.object(forInfoDictionaryKey: "LOOPER_BUILD_BRANCH")
                    as? String
            )?.trimmingCharacters(in: .whitespacesAndNewlines)
        )
    }

    private static func resolveConnection(
        fromConnectionCode connectionCode: String,
        remainingDepth: Int
    ) throws -> CompanionConnection {
        let trimmedConnectionCode = connectionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnectionCode.isEmpty else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        if let normalizedBaseURL = try? normalizeBaseURL(trimmedConnectionCode) {
            return CompanionConnection(baseURLs: [normalizedBaseURL], bearerToken: nil)
        }

        if let connection = resolveConnection(fromSchemeCode: trimmedConnectionCode) {
            return connection
        }

        if let payloadConnection = try? resolveConnection(fromJSONPayload: trimmedConnectionCode) {
            return payloadConnection
        }

        guard remainingDepth > 0, let decodedConnectionCode = decodeBase64URLString(trimmedConnectionCode)
        else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        return try resolveConnection(
            fromConnectionCode: decodedConnectionCode,
            remainingDepth: remainingDepth - 1
        )
    }

    private static func resolveConnection(fromSchemeCode schemeCode: String) -> CompanionConnection? {
        guard
            let url = URL(string: schemeCode),
            let scheme = url.scheme?.lowercased(),
            supportedConnectionCodeSchemes.contains(scheme),
            let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else {
            return nil
        }

        let candidateValues = components.queryItems?.compactMap { item -> [String]? in
            if supportedConnectionCodeKeys.contains(item.name), let value = item.value {
                return [value]
            }

            if supportedConnectionCodeListKeys.contains(item.name), let value = item.value {
                return splitBaseURLValues(value)
            }

            return nil
        }.flatMap(\.self) ?? []

        let urls = normalizedBaseURLs(from: candidateValues.joined(separator: "\n"))
        guard !urls.isEmpty else {
            return nil
        }

        return CompanionConnection(
            baseURLs: urls,
            bearerToken: bearerToken(from: components.queryItems ?? [])
        )
    }

    private static func resolveConnection(fromJSONPayload jsonPayload: String) throws -> CompanionConnection {
        let payloadData = Data(jsonPayload.utf8)
        let payload = try JSONDecoder().decode(ConnectionCodePayload.self, from: payloadData)
        let urls = payload.baseURLs.compactMap { try? normalizeBaseURL($0) }
        guard !urls.isEmpty else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        return CompanionConnection(baseURLs: uniqueURLs(urls), bearerToken: payload.bearerToken)
    }

    private static func normalizeBaseURL(_ value: String) throws -> URL {
        let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedValue.isEmpty, !trimmedValue.hasPrefix("$(") else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        let candidateValue = trimmedValue.contains("://") ? trimmedValue : "http://\(trimmedValue)"
        guard
            let url = URL(string: candidateValue),
            let scheme = url.scheme?.lowercased(),
            ["http", "https"].contains(scheme),
            url.host != nil
        else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        return url
    }

    private static func normalizedBaseURLs(from value: String) -> [URL] {
        uniqueURLs(splitBaseURLValues(value).compactMap { try? normalizeBaseURL($0) })
    }

    private static func splitBaseURLValues(_ value: String) -> [String] {
        value
            .components(separatedBy: CharacterSet(charactersIn: "\n,; "))
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
    }

    private static func uniqueURLs(_ urls: [URL]) -> [URL] {
        var seen = Set<String>()
        return urls.filter { url in
            let key = url.absoluteString
            guard !seen.contains(key) else {
                return false
            }

            seen.insert(key)
            return true
        }
    }

    private static func bearerToken(from queryItems: [URLQueryItem]) -> String? {
        var tokenID: String?
        var token: String?

        for item in queryItems {
            if supportedPairingTokenIDKeys.contains(item.name) {
                tokenID = nonEmptyString(item.value)
            } else if supportedPairingTokenKeys.contains(item.name) {
                token = nonEmptyString(item.value)
            }
        }

        guard let tokenID, let token else {
            return nil
        }

        return "\(tokenID).\(token)"
    }

    private static func nonEmptyString(_ value: String?) -> String? {
        let trimmedValue = value?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return trimmedValue.isEmpty ? nil : trimmedValue
    }

    private static func loadBearerToken() -> String? {
        let query = bearerTokenKeychainQuery(returnData: true)
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard
            status == errSecSuccess,
            let data = item as? Data,
            let token = String(data: data, encoding: .utf8)
        else {
            return nil
        }

        return nonEmptyString(token)
    }

    private static func storeBearerToken(_ token: String?) {
        let baseQuery = bearerTokenKeychainQuery(returnData: false)
        SecItemDelete(baseQuery as CFDictionary)

        guard let token = nonEmptyString(token),
              let data = token.data(using: .utf8)
        else {
            return
        }

        var item = baseQuery
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        SecItemAdd(item as CFDictionary, nil)
    }

    private static func bearerTokenKeychainQuery(returnData: Bool) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: apiBearerTokenService,
            kSecAttrAccount as String: apiBearerTokenAccount
        ]

        if returnData {
            query[kSecReturnData as String] = true
            query[kSecMatchLimit as String] = kSecMatchLimitOne
        }

        return query
    }

    private static func decodeBase64URLString(_ value: String) -> String? {
        let compactValue = value.replacingOccurrences(of: " ", with: "")
        let paddedValue = compactValue
            .replacingOccurrences(of: "-", with: "+")
            .replacingOccurrences(of: "_", with: "/")
            .padding(
                toLength: ((compactValue.count + 3) / 4) * 4,
                withPad: "=",
                startingAt: 0
            )

        guard
            let decodedData = Data(base64Encoded: paddedValue),
            let decodedString = String(data: decodedData, encoding: .utf8)
        else {
            return nil
        }

        return decodedString
    }
}

private struct ConnectionCodePayload: Decodable {
    let baseURLs: [String]
    let bearerToken: String?

    enum CodingKeys: String, CodingKey {
        case baseURL
        case baseURLs
        case alternateBaseURL = "base_url"
        case alternateBaseURLs = "base_urls"
        case url
        case urls
        case pairingTokenId
        case alternatePairingTokenId = "pairing_token_id"
        case tokenId
        case pairingToken
        case alternatePairingToken = "pairing_token"
        case token
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        var values: [String] = []

        values.append(contentsOf: try container.decodeIfPresent([String].self, forKey: .baseURLs) ?? [])
        values.append(contentsOf: try container.decodeIfPresent([String].self, forKey: .alternateBaseURLs) ?? [])
        values.append(contentsOf: try container.decodeIfPresent([String].self, forKey: .urls) ?? [])

        if let primaryBaseURL = try container.decodeIfPresent(String.self, forKey: .baseURL) {
            values.append(primaryBaseURL)
        }

        if let snakeCaseBaseURL = try container.decodeIfPresent(String.self, forKey: .alternateBaseURL) {
            values.append(snakeCaseBaseURL)
        }

        if let genericURL = try container.decodeIfPresent(String.self, forKey: .url) {
            values.append(genericURL)
        }

        let tokenID =
            try container.decodeIfPresent(String.self, forKey: .pairingTokenId)
            ?? container.decodeIfPresent(String.self, forKey: .alternatePairingTokenId)
            ?? container.decodeIfPresent(String.self, forKey: .tokenId)
        let token =
            try container.decodeIfPresent(String.self, forKey: .pairingToken)
            ?? container.decodeIfPresent(String.self, forKey: .alternatePairingToken)
            ?? container.decodeIfPresent(String.self, forKey: .token)

        baseURLs = values
        bearerToken = tokenID.flatMap { tokenID in
            token.map { token in "\(tokenID).\(token)" }
        }
    }
}

struct CompanionRelease: Sendable {
    let marketingVersion: String
    let buildNumber: String
    let releaseChannel: String?
    let branchName: String?

    var footerLabel: String {
        let primaryLine = "looper \(marketingVersion) (\(buildNumber))"
        let metadataLine = [releaseChannel, branchName]
            .compactMap { value -> String? in
                guard let value else {
                    return nil
                }

                let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
                return trimmedValue.isEmpty ? nil : trimmedValue
            }
            .joined(separator: " • ")

        guard !metadataLine.isEmpty else {
            return primaryLine
        }

        return [primaryLine, metadataLine].joined(separator: "\n")
    }
}

enum CompanionConfigurationError: LocalizedError {
    case apiBaseURLNotConfigured
    case invalidConnectionCode
    case invalidAPIBaseURL

    var errorDescription: String? {
        switch self {
        case .apiBaseURLNotConfigured:
            return "Scan the Mac code or enter the device code in Settings to connect this iPhone."
        case .invalidConnectionCode:
            return "The device code is invalid."
        case .invalidAPIBaseURL:
            return "The saved Mac connection is invalid."
        }
    }
}

struct UnconfiguredCompanionService: CompanionService {
    private let error: CompanionConfigurationError

    init(error: CompanionConfigurationError) {
        self.error = error
    }

    func loadServerHealth() async throws -> CompanionServerHealth { throw error }
    func loadSnapshot() async throws -> MobileSnapshot { throw error }
    func loadSessionDetail(id _: String) async throws -> SessionDetail { throw error }
    func setSessionMode(id _: String, preset _: SessionMode?) async throws -> MobileSnapshot {
        throw error
    }
    func setSessionArchived(id _: String, archived _: Bool) async throws -> MobileSnapshot {
        throw error
    }
    func deleteSession(id _: String) async throws -> MobileSnapshot { throw error }
    func saveDefaultPrompt(_: String) async throws -> MobileSnapshot { throw error }
    func registerPushDevice(
        _: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        throw error
    }
    func sendTestPush(installationID _: String) async throws -> RemotePushTestResponse {
        throw error
    }
}

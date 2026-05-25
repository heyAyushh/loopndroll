import Foundation

enum CompanionConfiguration {
    static let apiBaseURLOverrideKey = "looper.apiBaseURLOverride"
    private static let supportedConnectionCodeSchemes = ["looper", "loopndroll"]
    private static let supportedConnectionCodeKeys = ["baseURL", "base_url", "url"]

    static func resolvedBaseURLString() -> String {
        let storedValue = UserDefaults.standard.string(forKey: apiBaseURLOverrideKey) ?? ""
        let trimmedStoredValue = storedValue.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmedStoredValue.isEmpty {
            return trimmedStoredValue
        }

        let bundledValue = (
            Bundle.main.object(forInfoDictionaryKey: "LOOPER_API_BASE_URL")
                ?? Bundle.main.object(forInfoDictionaryKey: "LOOPNDROLL_API_BASE_URL")
        ) as? String
        let trimmedBundledValue = bundledValue?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        if !trimmedBundledValue.isEmpty && !trimmedBundledValue.hasPrefix("$(") {
            return trimmedBundledValue
        }

        #if targetEnvironment(simulator)
            return "http://127.0.0.1:8787"
        #else
            return ""
        #endif
    }

    static func resolveBaseURLString(fromConnectionCode connectionCode: String) throws -> String {
        try resolveBaseURLString(fromConnectionCode: connectionCode, remainingDepth: 2)
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

    private static func resolveBaseURLString(
        fromConnectionCode connectionCode: String,
        remainingDepth: Int
    ) throws -> String {
        let trimmedConnectionCode = connectionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnectionCode.isEmpty else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        if let normalizedBaseURL = try? normalizeBaseURLString(trimmedConnectionCode) {
            return normalizedBaseURL
        }

        if let baseURLFromScheme = resolveBaseURLString(fromSchemeCode: trimmedConnectionCode) {
            return baseURLFromScheme
        }

        if let payloadBaseURL = try? resolveBaseURLString(fromJSONPayload: trimmedConnectionCode) {
            return payloadBaseURL
        }

        guard remainingDepth > 0, let decodedConnectionCode = decodeBase64URLString(trimmedConnectionCode)
        else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        return try resolveBaseURLString(
            fromConnectionCode: decodedConnectionCode,
            remainingDepth: remainingDepth - 1
        )
    }

    private static func resolveBaseURLString(fromSchemeCode schemeCode: String) -> String? {
        guard
            let url = URL(string: schemeCode),
            let scheme = url.scheme?.lowercased(),
            supportedConnectionCodeSchemes.contains(scheme),
            let components = URLComponents(url: url, resolvingAgainstBaseURL: false)
        else {
            return nil
        }

        let candidateBaseURL = components.queryItems?
            .first(where: { supportedConnectionCodeKeys.contains($0.name) })?
            .value

        guard let candidateBaseURL else {
            return nil
        }

        return try? normalizeBaseURLString(candidateBaseURL)
    }

    private static func resolveBaseURLString(fromJSONPayload jsonPayload: String) throws -> String {
        let payloadData = Data(jsonPayload.utf8)
        let payload = try JSONDecoder().decode(ConnectionCodePayload.self, from: payloadData)
        guard let candidateBaseURL = payload.baseURL else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        return try normalizeBaseURLString(candidateBaseURL)
    }

    private static func normalizeBaseURLString(_ value: String) throws -> String {
        let trimmedValue = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard
            let url = URL(string: trimmedValue),
            let scheme = url.scheme?.lowercased(),
            ["http", "https"].contains(scheme),
            url.host != nil
        else {
            throw CompanionConfigurationError.invalidConnectionCode
        }

        return trimmedValue
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
    let baseURL: String?

    private let alternateBaseURL: String?
    private let url: String?

    enum CodingKeys: String, CodingKey {
        case baseURL
        case alternateBaseURL = "base_url"
        case url
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let primaryBaseURL = try container.decodeIfPresent(String.self, forKey: .baseURL)
        let snakeCaseBaseURL = try container.decodeIfPresent(String.self, forKey: .alternateBaseURL)
        let genericURL = try container.decodeIfPresent(String.self, forKey: .url)

        baseURL = primaryBaseURL ?? snakeCaseBaseURL ?? genericURL
        alternateBaseURL = snakeCaseBaseURL
        url = genericURL
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

import Foundation

public struct LooperSiriEntityIdentifier: Equatable, Hashable, Sendable {
    private static let componentSeparator: Character = "|"

    public let assistantSurface: String
    public let sessionID: String

    public var rawValue: String {
        "\(assistantSurface)\(Self.componentSeparator)\(sessionID)"
    }

    public init(assistantSurface: String, sessionID: String) {
        self.assistantSurface = assistantSurface
        self.sessionID = sessionID
    }

    public init?(rawValue: String) {
        let parts = rawValue
            .split(separator: Self.componentSeparator, maxSplits: 1)
            .map(String.init)
        guard parts.count == 2,
              !parts[0].isEmpty,
              !parts[1].isEmpty
        else {
            return nil
        }

        assistantSurface = parts[0]
        sessionID = parts[1]
    }

    public static func parsedOrLegacyCodexIdentifier(rawValue: String) -> Self? {
        let trimmedValue = rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedValue.isEmpty else {
            return nil
        }

        return Self(rawValue: trimmedValue) ?? Self(
            assistantSurface: LooperSiriEntitySearch.codexAssistantSurface,
            sessionID: trimmedValue
        )
    }
}

public struct LooperSiriSyncableIdentifier: Equatable, Hashable, Sendable {
    private static let componentSeparator = "=>"

    public let localID: String
    public let stableID: String

    public var rawValue: String {
        "\(localID)\(Self.componentSeparator)\(stableID)"
    }

    public init(localID: String, stableID: String) {
        self.localID = localID
        self.stableID = stableID
    }

    public init?(rawValue: String) {
        let components = rawValue.components(separatedBy: Self.componentSeparator)
        guard components.count == 2,
              let localID = components[0]
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .nilIfEmpty,
              let stableID = components[1]
                .trimmingCharacters(in: .whitespacesAndNewlines)
                .nilIfEmpty
        else {
            return nil
        }

        self.localID = localID
        self.stableID = stableID
    }
}

public enum LooperSiriEntitySearch {
    public static let codexAssistantSurface = "codex"

    public static func searchableText(fields: [String?]) -> String {
        fields
            .compactMap { value in
                value?
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                    .nilIfEmpty
            }
            .joined(separator: " ")
    }

    public static func matches(searchText: String, fields: [String?]) -> Bool {
        let normalizedSearchText = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalizedSearchText.isEmpty else {
            return true
        }

        return searchableText(fields: fields)
            .localizedCaseInsensitiveContains(normalizedSearchText)
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

import Foundation

public struct LooperThreadOpenTarget: Equatable, Sendable {
    public let threadId: String
    public let codexURL: URL?
    public let transcriptURL: URL?
    public let projectURL: URL?

    public init(
        threadId: String,
        transcriptPath: String?,
        workingDirectory: String?,
        agentPath: String? = nil
    ) {
        self.threadId = threadId
        let usesExternalAssistantSession = Self.isExternalAssistantSession(
            threadId: threadId,
            transcriptPath: transcriptPath,
            workingDirectory: workingDirectory,
            agentPath: agentPath
        )
        self.codexURL = usesExternalAssistantSession ? nil : Self.codexThreadURL(for: threadId)
        self.transcriptURL = Self.fileURL(from: transcriptPath, isDirectory: false)
        self.projectURL = Self.fileURL(from: workingDirectory, isDirectory: true)
    }

    public var firstLocalFallbackURL: URL? {
        transcriptURL ?? projectURL
    }

    static func codexThreadURL(for threadId: String) -> URL? {
        let normalizedThreadId = normalizedString(threadId)
        guard let normalizedThreadId else {
            return nil
        }

        var components = URLComponents()
        components.scheme = DeepLink.codexScheme
        components.host = DeepLink.codexThreadHost
        components.path = "\(DeepLink.pathSeparator)\(normalizedThreadId)"
        return components.url
    }

    private static func fileURL(from path: String?, isDirectory: Bool) -> URL? {
        guard let normalizedPath = normalizedString(path) else {
            return nil
        }
        return URL(fileURLWithPath: normalizedPath, isDirectory: isDirectory).standardizedFileURL
    }

    private static func normalizedString(_ value: String?) -> String? {
        let trimmedValue = value?.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let trimmedValue, !trimmedValue.isEmpty else {
            return nil
        }
        return trimmedValue
    }

    static func isExternalAssistantSession(
        threadId: String,
        transcriptPath: String?,
        workingDirectory: String?,
        agentPath: String? = nil
    ) -> Bool {
        let needles = [
            "/.grok/",
            ".grok/sessions",
            "grok agent",
            "grok-build",
            "devin:devin-cli:",
            "devin:devin-cloud:",
            "acp/devin-cli/",
            "acp/devin-cloud/",
            "/library/application support/devin/",
            "/library/application support/devin - next/",
            "devin-desktop",
        ]
        return [threadId, transcriptPath, workingDirectory, agentPath]
            .compactMap { $0?.lowercased() }
            .contains { haystack in
                needles.contains { haystack.contains($0) }
            }
    }

    private enum DeepLink {
        static let codexScheme = "codex"
        static let codexThreadHost = "threads"
        static let pathSeparator = "/"
    }
}

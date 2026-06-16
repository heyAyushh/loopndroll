import Foundation

public struct LooperSessionResolutionCandidate: Equatable, Sendable {
    public let sessionID: String
    public let assistantSurface: String
    public let isArchived: Bool

    public init(
        sessionID: String,
        assistantSurface: String,
        isArchived: Bool
    ) {
        self.sessionID = sessionID
        self.assistantSurface = assistantSurface
        self.isArchived = isArchived
    }
}

public enum LooperCurrentSessionResolution {
    public enum Source: Equatable, Sendable {
        case currentSession
        case defaultSession
    }

    public struct ResolvedSession: Equatable, Sendable {
        public let sessionID: String
        public let assistantSurface: String
        public let source: Source

        public init(
            sessionID: String,
            assistantSurface: String,
            source: Source
        ) {
            self.sessionID = sessionID
            self.assistantSurface = assistantSurface
            self.source = source
        }
    }

    public static func resolve(
        currentSessionID: String?,
        currentAssistantSurface: String?,
        defaultSessionID: String?,
        defaultAssistantSurface: String?,
        candidates: [LooperSessionResolutionCandidate],
        fallbackAssistantSurface: String = LooperSiriEntitySearch.codexAssistantSurface
    ) -> ResolvedSession? {
        let activeCandidates = normalizedActiveCandidates(candidates)

        if let currentSession = resolveConfiguredSession(
            sessionID: currentSessionID,
            assistantSurface: currentAssistantSurface,
            source: .currentSession,
            candidates: activeCandidates,
            fallbackAssistantSurface: fallbackAssistantSurface
        ) {
            return currentSession
        }

        return resolveConfiguredSession(
            sessionID: defaultSessionID,
            assistantSurface: defaultAssistantSurface,
            source: .defaultSession,
            candidates: activeCandidates,
            fallbackAssistantSurface: fallbackAssistantSurface
        )
    }

    private static func resolveConfiguredSession(
        sessionID: String?,
        assistantSurface: String?,
        source: Source,
        candidates: [LooperSessionResolutionCandidate],
        fallbackAssistantSurface: String
    ) -> ResolvedSession? {
        guard let normalizedSessionID = normalized(sessionID) else {
            return nil
        }

        let resolvedAssistantSurface = normalized(assistantSurface)
            ?? inferredAssistantSurface(for: normalizedSessionID, candidates: candidates)
            ?? normalized(fallbackAssistantSurface)
            ?? LooperSiriEntitySearch.codexAssistantSurface

        guard candidates.contains(where: { candidate in
            candidate.sessionID == normalizedSessionID &&
                candidate.assistantSurface == resolvedAssistantSurface
        }) else {
            return nil
        }

        return ResolvedSession(
            sessionID: normalizedSessionID,
            assistantSurface: resolvedAssistantSurface,
            source: source
        )
    }

    private static func normalizedActiveCandidates(
        _ candidates: [LooperSessionResolutionCandidate]
    ) -> [LooperSessionResolutionCandidate] {
        candidates.compactMap { candidate in
            guard !candidate.isArchived,
                  let sessionID = normalized(candidate.sessionID),
                  let assistantSurface = normalized(candidate.assistantSurface)
            else {
                return nil
            }

            return LooperSessionResolutionCandidate(
                sessionID: sessionID,
                assistantSurface: assistantSurface,
                isArchived: false
            )
        }
    }

    private static func inferredAssistantSurface(
        for sessionID: String,
        candidates: [LooperSessionResolutionCandidate]
    ) -> String? {
        candidates.first { candidate in
            candidate.sessionID == sessionID
        }?.assistantSurface
    }

    private static func normalized(_ value: String?) -> String? {
        value?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .nilIfEmpty
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

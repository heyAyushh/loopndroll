import Testing
@testable import LooperCompanionCore

@Suite("Looper current session resolution")
struct LooperCurrentSessionResolutionTests {
    private let codexSurface = "codex"
    private let devinSurface = "devin"
    private let currentSessionID = "current-thread"
    private let defaultSessionID = "default-thread"
    private let unrelatedSessionID = "newer-thread"

    @Test("Current session wins over the configured default")
    func currentSessionWinsOverDefault() throws {
        let resolution = try #require(LooperCurrentSessionResolution.resolve(
            currentSessionID: currentSessionID,
            currentAssistantSurface: devinSurface,
            defaultSessionID: defaultSessionID,
            defaultAssistantSurface: codexSurface,
            candidates: [
                candidate(defaultSessionID, surface: codexSurface),
                candidate(currentSessionID, surface: devinSurface),
            ]
        ))

        #expect(resolution.sessionID == currentSessionID)
        #expect(resolution.assistantSurface == devinSurface)
        #expect(resolution.source == .currentSession)
    }

    @Test("Stale current session falls back to the configured default")
    func staleCurrentSessionFallsBackToDefault() throws {
        let resolution = try #require(LooperCurrentSessionResolution.resolve(
            currentSessionID: "stale-thread",
            currentAssistantSurface: devinSurface,
            defaultSessionID: defaultSessionID,
            defaultAssistantSurface: codexSurface,
            candidates: [
                candidate(defaultSessionID, surface: codexSurface),
                candidate(unrelatedSessionID, surface: devinSurface),
            ]
        ))

        #expect(resolution.sessionID == defaultSessionID)
        #expect(resolution.assistantSurface == codexSurface)
        #expect(resolution.source == .defaultSession)
    }

    @Test("Unset assistant surface is inferred from the matching session")
    func unsetAssistantSurfaceIsInferred() throws {
        let resolution = try #require(LooperCurrentSessionResolution.resolve(
            currentSessionID: currentSessionID,
            currentAssistantSurface: nil,
            defaultSessionID: defaultSessionID,
            defaultAssistantSurface: codexSurface,
            candidates: [
                candidate(defaultSessionID, surface: codexSurface),
                candidate(currentSessionID, surface: devinSurface),
            ]
        ))

        #expect(resolution.sessionID == currentSessionID)
        #expect(resolution.assistantSurface == devinSurface)
        #expect(resolution.source == .currentSession)
    }

    @Test("Resolver does not choose an arbitrary latest session")
    func resolverDoesNotChooseArbitraryLatestSession() {
        let resolution = LooperCurrentSessionResolution.resolve(
            currentSessionID: nil,
            currentAssistantSurface: nil,
            defaultSessionID: nil,
            defaultAssistantSurface: nil,
            candidates: [
                candidate(unrelatedSessionID, surface: codexSurface),
            ]
        )

        #expect(resolution == nil)
    }

    @Test("Archived configured sessions are unavailable")
    func archivedConfiguredSessionsAreUnavailable() {
        let resolution = LooperCurrentSessionResolution.resolve(
            currentSessionID: currentSessionID,
            currentAssistantSurface: codexSurface,
            defaultSessionID: defaultSessionID,
            defaultAssistantSurface: devinSurface,
            candidates: [
                candidate(currentSessionID, surface: codexSurface, isArchived: true),
                candidate(defaultSessionID, surface: devinSurface, isArchived: true),
            ]
        )

        #expect(resolution == nil)
    }

    @Test(
        "Blank configured session IDs never resolve to an arbitrary candidate",
        arguments: ["", " ", "\n", "\t"]
    )
    func blankConfiguredSessionIDsNeverResolve(blankSessionID: String) {
        let resolution = LooperCurrentSessionResolution.resolve(
            currentSessionID: blankSessionID,
            currentAssistantSurface: devinSurface,
            defaultSessionID: blankSessionID,
            defaultAssistantSurface: codexSurface,
            candidates: [
                candidate(unrelatedSessionID, surface: codexSurface),
                candidate(currentSessionID, surface: devinSurface),
            ]
        )

        #expect(resolution == nil)
    }

    private func candidate(
        _ sessionID: String,
        surface: String,
        isArchived: Bool = false
    ) -> LooperSessionResolutionCandidate {
        LooperSessionResolutionCandidate(
            sessionID: sessionID,
            assistantSurface: surface,
            isArchived: isArchived
        )
    }
}

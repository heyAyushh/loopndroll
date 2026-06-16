import Testing
@testable import LooperCompanionCore

@Suite("Looper Siri entity support")
struct LooperSiriEntitySupportTests {
    private let codexSurface = "codex"
    private let grokBuildSurface = "grok-build"
    private let colonSeparatedSessionID = "devin:devin-cli:thread-1"
    private let branchName = "cx/siri-ai"
    private let projectPath = "/Users/ay/Documents/looper"
    private let repositoryName = "looper"
    private let pluginName = "Superpowers"

    @Test("Qualified entity IDs preserve assistant surface and session ID")
    func qualifiedEntityIDsPreserveSurfaceAndSessionID() throws {
        let codexIdentifier = LooperSiriEntityIdentifier(
            assistantSurface: codexSurface,
            sessionID: colonSeparatedSessionID
        )
        #expect(codexIdentifier.rawValue == "codex|devin:devin-cli:thread-1")

        let identifier = LooperSiriEntityIdentifier(
            assistantSurface: grokBuildSurface,
            sessionID: colonSeparatedSessionID
        )

        #expect(identifier.rawValue == "grok-build|devin:devin-cli:thread-1")

        let parsedIdentifier = try #require(LooperSiriEntityIdentifier(rawValue: identifier.rawValue))
        #expect(parsedIdentifier.assistantSurface == grokBuildSurface)
        #expect(parsedIdentifier.sessionID == colonSeparatedSessionID)
    }

    @Test("Syncable IDs preserve local and stable entity identifiers")
    func syncableIDsPreserveLocalAndStableIdentifiers() throws {
        let identifier = LooperSiriSyncableIdentifier(
            localID: "codex|local-thread",
            stableID: "codex|stable-thread"
        )

        #expect(identifier.localID == "codex|local-thread")
        #expect(identifier.stableID == "codex|stable-thread")
        #expect(identifier.rawValue == "codex|local-thread=>codex|stable-thread")

        let parsedIdentifier = try #require(LooperSiriSyncableIdentifier(rawValue: identifier.rawValue))
        #expect(parsedIdentifier.localID == "codex|local-thread")
        #expect(parsedIdentifier.stableID == "codex|stable-thread")
    }

    @Test("Legacy plain session IDs remain Codex identifiers")
    func legacyPlainSessionIDsRemainCodexIdentifiers() throws {
        let parsedIdentifier = try #require(
            LooperSiriEntityIdentifier.parsedOrLegacyCodexIdentifier(
                rawValue: colonSeparatedSessionID
            )
        )

        #expect(parsedIdentifier.assistantSurface == codexSurface)
        #expect(parsedIdentifier.sessionID == colonSeparatedSessionID)
    }

    @Test("Spotlight identifiers recover raw session IDs")
    func spotlightIdentifiersRecoverRawSessionIDs() throws {
        let searchableIdentifier = LooperSiriEntityIdentifier(
            assistantSurface: grokBuildSurface,
            sessionID: colonSeparatedSessionID
        ).rawValue

        let parsedIdentifier = try #require(
            LooperSiriEntityIdentifier.parsedOrLegacyCodexIdentifier(
                rawValue: searchableIdentifier
            )
        )

        #expect(parsedIdentifier.assistantSurface == grokBuildSurface)
        #expect(parsedIdentifier.sessionID == colonSeparatedSessionID)
    }

    @Test("Search matches repository branch project path and plugin fields")
    func searchMatchesSessionMetadataFields() {
        let fields = [
            "Looper Siri AI",
            projectPath,
            repositoryName,
            branchName,
            pluginName,
            grokBuildSurface,
        ]

        #expect(LooperSiriEntitySearch.matches(searchText: "cx/siri", fields: fields))
        #expect(LooperSiriEntitySearch.matches(searchText: "Documents/looper", fields: fields))
        #expect(LooperSiriEntitySearch.matches(searchText: "superpowers", fields: fields))
        #expect(!LooperSiriEntitySearch.matches(searchText: "unrelated-project", fields: fields))
    }
}

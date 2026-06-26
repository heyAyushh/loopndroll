import Foundation
import Testing
@testable import Looper

@Suite("HTTPCompanionService command core")
struct HTTPCompanionServiceCommandCoreTests {
    @Test
    func failedCommandSubmissionsStayInOneClientCoreOutbox() async throws {
        let store = try Self.temporaryMiniStore()
        let service = HTTPCompanionService(
            baseURLs: [],
            bearerToken: nil,
            sessionMiniLocalStore: store
        )

        await #expect(throws: Error.self) {
            _ = try await service.setSessionMode(
                id: "thread-1",
                preset: .maxTurns2,
                clientMutationID: "mutation-mode"
            )
        }
        #expect(try service.commandOutboxDepthForSelfTest() == 1)
        #expect(store.pendingCommands().count == 1)
        #expect(store.pendingCommands().first?.attemptCount == 1)

        await #expect(throws: Error.self) {
            _ = try await service.sendSessionPrompt(
                id: "thread-1",
                prompt: "continue",
                assistantSurface: .codex,
                clientMutationID: "mutation-prompt"
            )
        }

        #expect(try service.commandOutboxDepthForSelfTest() == 2)
        #expect(store.pendingCommands().count == 2)
        #expect(store.pendingCommands().map(\.clientMutationID) == [
            "mutation-mode",
            "mutation-prompt",
        ])
    }

    private static func temporaryMiniStore() throws -> CompanionSessionMiniLocalStore {
        let directoryURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent(
                ".test-artifacts/http-command-core/\(UUID().uuidString)",
                isDirectory: true
            )
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return try CompanionSessionMiniLocalStore(
            fileURL: directoryURL.appendingPathComponent(
                CompanionSessionMiniLocalStore.defaultFileName
            )
        )
    }
}

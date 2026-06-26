import Foundation
import Testing
@testable import Looper

@Suite("HTTPCompanionService command core")
struct HTTPCompanionServiceCommandCoreTests {
    @Test
    func sessionRuntimeStartIfNeededOwnsEndpointMapping() async throws {
        let store = try Self.temporaryMiniStore()
        let runtime = CompanionSessionRuntime(localStore: store)
        let endpointURL = try #require(URL(string: "http://100.64.0.2:8765"))
        var endpointProviderCalls = 0

        let snapshot = try await runtime.startIfNeeded(
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        ) {
            endpointProviderCalls += 1
            return [endpointURL]
        }

        let secondStart = try await runtime.startIfNeeded(
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        ) {
            endpointProviderCalls += 1
            return []
        }

        #expect(snapshot?.phase == .ready)
        #expect(snapshot?.endpointUrl == endpointURL.absoluteString)
        #expect(secondStart == nil)
        #expect(endpointProviderCalls == 1)
    }

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

    @Test
    func generatedCommandMutationsComeFromRustCore() async throws {
        let store = try Self.temporaryMiniStore()
        let service = HTTPCompanionService(
            baseURLs: [],
            bearerToken: nil,
            sessionMiniLocalStore: store
        )

        await #expect(throws: Error.self) {
            _ = try await service.setSessionMode(
                id: "thread-1",
                preset: .maxTurns2
            )
        }
        await #expect(throws: Error.self) {
            _ = try await service.sendSessionPrompt(
                id: "thread-1",
                prompt: "continue",
                assistantSurface: .codex
            )
        }

        #expect(store.pendingCommands().map(\.kind) == [.setSessionMode, .sendSessionPrompt])
        #expect(store.pendingCommands()[0].clientMutationID.hasPrefix("mode-"))
        #expect(store.pendingCommands()[1].clientMutationID.hasPrefix("prompt-"))
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

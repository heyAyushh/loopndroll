import Foundation
import Testing
@testable import Looper

@Suite("CompanionSessionRuntime command core")
struct CompanionSessionRuntimeCommandCoreTests {
    @Test
    func sessionRuntimeStartIfNeededLetsRustCoreOwnStreamRestart() async throws {
        let runtime = try Self.temporarySessionRuntime()
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
            return [endpointURL]
        }
        _ = try runtime.stop()
        let restartedSnapshot = try await runtime.startIfNeeded(
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        ) {
            endpointProviderCalls += 1
            return [endpointURL]
        }

        #expect(snapshot?.phase == .ready)
        #expect(snapshot?.endpointUrl == endpointURL.absoluteString)
        #expect(secondStart?.phase == .ready)
        #expect(secondStart?.endpointUrl == endpointURL.absoluteString)
        #expect(restartedSnapshot?.phase == .ready)
        #expect(restartedSnapshot?.endpointUrl == endpointURL.absoluteString)
        #expect(endpointProviderCalls == 3)
    }

    @Test
    func localCommandAcceptanceUsesOneClientCoreOutbox() async throws {
        let runtime = try Self.temporarySessionRuntime()
        let modeResult = try await runtime.setMode(
            threadID: "thread-1",
            preset: .maxTurns2
        )
        #expect(modeResult.accepted)
        #expect(try runtime.outboxDepth() == 1)
        #expect(runtime.pendingCommands().count == 1)
        #expect(runtime.pendingCommands().first?.attemptCount == 1)

        let promptResult = try await runtime.sendPrompt(
            threadID: "thread-1",
            prompt: "continue",
            assistantSurface: .codex
        )
        #expect(promptResult.accepted)

        #expect(try runtime.outboxDepth() == 2)
        #expect(runtime.pendingCommands().count == 2)
        #expect(runtime.pendingCommands()[0].clientMutationID.hasPrefix("mode-"))
        #expect(runtime.pendingCommands()[1].clientMutationID.hasPrefix("prompt-"))
    }

    @Test
    func generatedCommandMutationsComeFromRustCore() async throws {
        let runtime = try Self.temporarySessionRuntime()

        let modeResult = try await runtime.setMode(
            threadID: "thread-1",
            preset: .maxTurns2
        )
        let promptResult = try await runtime.sendPrompt(
            threadID: "thread-1",
            prompt: "continue",
            assistantSurface: .codex
        )
        #expect(modeResult.accepted)
        #expect(promptResult.accepted)

        #expect(runtime.pendingCommands().map(\.kind) == [.setSessionMode, .sendSessionPrompt])
        #expect(runtime.pendingCommands()[0].clientMutationID.hasPrefix("mode-"))
        #expect(runtime.pendingCommands()[1].clientMutationID.hasPrefix("prompt-"))
    }

    private static func temporarySessionRuntime() throws -> CompanionSessionRuntime {
        let directoryURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("looper-session-runtime-command-core", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return try CompanionSessionRuntime(
            fileURL: directoryURL.appendingPathComponent(
                CompanionSessionRuntime.defaultFileName
            )
        )
    }
}

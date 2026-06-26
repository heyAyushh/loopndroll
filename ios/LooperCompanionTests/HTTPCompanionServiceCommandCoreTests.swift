import Foundation
import Testing
@testable import Looper

@Suite("HTTPCompanionService command core")
struct HTTPCompanionServiceCommandCoreTests {
    @Test
    func sessionRuntimeStartIfNeededOwnsEndpointMapping() async throws {
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
            return []
        }

        #expect(snapshot?.phase == .ready)
        #expect(snapshot?.endpointUrl == endpointURL.absoluteString)
        #expect(secondStart == nil)
        #expect(endpointProviderCalls == 1)
    }

    @Test
    func failedCommandSubmissionsStayInOneClientCoreOutbox() async throws {
        let runtime = try Self.temporarySessionRuntime()
        let service = HTTPCompanionService(
            baseURLs: [],
            bearerToken: nil,
            sessionRuntime: runtime
        )

        await #expect(throws: Error.self) {
            _ = try await service.setSessionMode(
                id: "thread-1",
                preset: .maxTurns2
            )
        }
        #expect(try service.commandOutboxDepthForSelfTest() == 1)
        #expect(runtime.pendingCommands().count == 1)
        #expect(runtime.pendingCommands().first?.attemptCount == 1)

        await #expect(throws: Error.self) {
            _ = try await service.sendSessionPrompt(
                id: "thread-1",
                prompt: "continue",
                assistantSurface: .codex
            )
        }

        #expect(try service.commandOutboxDepthForSelfTest() == 2)
        #expect(runtime.pendingCommands().count == 2)
        #expect(runtime.pendingCommands()[0].clientMutationID.hasPrefix("mode-"))
        #expect(runtime.pendingCommands()[1].clientMutationID.hasPrefix("prompt-"))
    }

    @Test
    func generatedCommandMutationsComeFromRustCore() async throws {
        let runtime = try Self.temporarySessionRuntime()
        let service = HTTPCompanionService(
            baseURLs: [],
            bearerToken: nil,
            sessionRuntime: runtime
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

        #expect(runtime.pendingCommands().map(\.kind) == [.setSessionMode, .sendSessionPrompt])
        #expect(runtime.pendingCommands()[0].clientMutationID.hasPrefix("mode-"))
        #expect(runtime.pendingCommands()[1].clientMutationID.hasPrefix("prompt-"))
    }

    private static func temporarySessionRuntime() throws -> CompanionSessionRuntime {
        let directoryURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent(
                ".test-artifacts/http-command-core/\(UUID().uuidString)",
                isDirectory: true
            )
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return try CompanionSessionRuntime(
            fileURL: directoryURL.appendingPathComponent(
                CompanionSessionRuntime.defaultFileName
            )
        )
    }
}

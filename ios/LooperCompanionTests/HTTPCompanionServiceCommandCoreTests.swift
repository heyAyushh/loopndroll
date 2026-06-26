import Foundation
import Testing
@testable import Looper

@Suite("HTTPCompanionService command core")
struct HTTPCompanionServiceCommandCoreTests {
    @Test
    func failedCommandSubmissionsStayInOneClientCoreOutbox() async throws {
        let service = HTTPCompanionService(baseURLs: [], bearerToken: nil)

        await #expect(throws: Error.self) {
            _ = try await service.setSessionMode(
                id: "thread-1",
                preset: .maxTurns2,
                clientMutationID: "mutation-mode"
            )
        }
        #expect(try service.commandOutboxDepthForSelfTest() == 1)

        await #expect(throws: Error.self) {
            _ = try await service.sendSessionPrompt(
                id: "thread-1",
                prompt: "continue",
                assistantSurface: .codex,
                clientMutationID: "mutation-prompt"
            )
        }

        #expect(try service.commandOutboxDepthForSelfTest() == 2)
    }
}

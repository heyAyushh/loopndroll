import Foundation
import Testing
@testable import LooperClientCore

@Suite("LooperClientCoreTests", .serialized)
struct LooperClientCoreTests {
    private let primaryEndpoint = "http://127.0.0.1:8765"
    private let lastGoodEndpoint = "http://100.64.0.2:8765"
    private let threadID = "thread-1"
    private let serverTime = "2026-06-25T00:00:00Z"

    @Test
    func configureSessionRuntimeStartsConnectingUntilStreamProvesEndpoint() throws {
        let manager = try temporarySessionManager()

        let snapshot = try manager.start(
            endpoints: [
                ClientEndpoint.h2(
                    url: primaryEndpoint,
                    recoveryBaseURL: "http://127.0.0.1:8765"
                ),
                ClientEndpoint.h3(
                    url: "https://100.64.0.2:8766",
                    recoveryBaseURL: "http://100.64.0.2:8765",
                    certificateSha256: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
                    lastGood: true
                ),
            ],
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        )

        #expect(snapshot.phase == .connecting)
        #expect(snapshot.endpointUrl.isEmpty)
        #expect(snapshot.outboxDepth == 0)
    }

    @Test
    func snapshotReflectsConnectingRuntimeBeforeFirstHeartbeat() throws {
        let manager = try temporarySessionManager()

        _ = try manager.start(
            endpoints: [
                ClientEndpoint.h2(
                    url: primaryEndpoint,
                    recoveryBaseURL: "http://127.0.0.1:8765",
                    lastGood: true
                )
            ],
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        )
        let snapshot = try manager.stateSnapshot()

        #expect(snapshot.phase == .connecting)
        #expect(snapshot.endpointUrl.isEmpty)
        #expect(snapshot.pendingMutations.isEmpty)
    }

    @Test
    func endpointFactoryPreservesH3MetadataForRustClientCore() throws {
        let endpoint = ClientEndpoint.h3(
            url: "https://100.64.0.2:8766",
            recoveryBaseURL: "http://100.64.0.2:8765",
            certificateSha256: "sha256:abcdef",
            certificateSpkiSha256: "sha256:fedcba",
            lastGood: true
        )

        #expect(endpoint.transport == .h3)
        #expect(endpoint.url == "https://100.64.0.2:8766")
        #expect(endpoint.recoveryBaseUrl == "http://100.64.0.2:8765")
        #expect(endpoint.h3CertificateSha256 == "sha256:abcdef")
        #expect(endpoint.h3CertificateSpkiSha256 == "sha256:fedcba")
        #expect(endpoint.lastGood)
    }

    @Test
    func sessionManagerAcceptsDeadH3ThenH2FallbackCandidatesForRustPolicy() throws {
        let manager = try temporarySessionManager()
        let snapshot = try manager.start(
            endpoints: [
                ClientEndpoint.h3(
                    url: "https://127.0.0.1:9",
                    recoveryBaseURL: "http://127.0.0.1:8765",
                    certificateSha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
                ),
                ClientEndpoint.h2(
                    url: "http://127.0.0.1:8766",
                    recoveryBaseURL: "http://127.0.0.1:8765"
                ),
            ],
            bearerToken: "token",
            mobileSessionHeader: "mobile-session"
        )

        #expect(snapshot.phase == .connecting)
        #expect(snapshot.endpointUrl.isEmpty)
        #expect(snapshot.transportFallbackReason.isEmpty)
        _ = try manager.stop()
    }

    @Test
    func mobileProjectionBuildsSnapshotFromStateMinis() throws {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: 10,
            sessions: [
                stateMini(sessionID: "thread-2", seq: 7, revision: "rev-7", title: "queued"),
                stateMini(sessionID: threadID, seq: 9, revision: "rev-9", title: "current"),
            ],
            serverTime: serverTime
        )

        #expect(projection.hasSnapshot)
        #expect(projection.snapshot.hasRevision)
        #expect(projection.snapshot.revision == "rev-9")
        #expect(projection.snapshot.host.lastSyncedAt == serverTime)
        #expect(projection.snapshot.sessions.contains { session in
            session.id == threadID && session.title == "current"
        })
    }

    @Test
    func mobileProjectionAppliesPendingCommandsFromRust() throws {
        let projection = try reduceStateMinisMobileSnapshotWithPendingCommands(
            latestSeq: 11,
            sessions: [
                stateMini(sessionID: threadID, seq: 11, revision: "rev-11", title: "pending")
            ],
            pendingCommands: [
                pendingCommand(
                    kind: .setSessionMode,
                    threadID: threadID,
                    preset: "max-turns-2"
                ),
                pendingCommand(
                    kind: .setSiriCurrentSession,
                    threadID: threadID,
                    assistantSurface: "codex"
                ),
            ],
            serverTime: serverTime
        )
        let session = try #require(projection.snapshot.sessions.first)
        let globalSettings = projection.snapshot.globalSettings

        #expect(projection.hasSnapshot)
        #expect(session.hasEffectiveMode)
        #expect(session.effectiveMode == "max-turns-2")
        #expect(globalSettings.hasSiriCurrentSessionId)
        #expect(globalSettings.siriCurrentSessionId == threadID)
        #expect(globalSettings.hasSiriCurrentAssistantSurface)
        #expect(globalSettings.siriCurrentAssistantSurface == "codex")
    }

    @Test
    func mobileProjectionBucketsCodexCompatibleClientsUnderCodex() throws {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: 12,
            sessions: [
                stateMini(
                    sessionID: "thread-cursor",
                    surface: "cursor",
                    seq: 12,
                    revision: "rev-12",
                    title: "cursor"
                ),
                stateMini(sessionID: threadID, seq: 11, revision: "rev-11", title: "codex"),
            ],
            serverTime: serverTime
        )
        let codexSessions = try #require(
            projection.snapshot.surfaceSessions.first { $0.surface == "codex" }?.sessions
        )

        #expect(projection.hasSnapshot)
        #expect(!projection.snapshot.sessions.isEmpty)
        #expect(projection.snapshot.surfaceSessions.first { $0.surface == "cursor" } == nil)
        #expect(codexSessions.count == 2)
        #expect(codexSessions[0].id == "thread-cursor")
        #expect(codexSessions[0].assistantClient == "cursor")
    }

    @Test
    func siriProjectionKeepsDuplicateSessionIDsAcrossSurfaces() throws {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: 14,
            sessions: [
                stateMini(
                    sessionID: "thread-main",
                    surface: "codex",
                    seq: 8,
                    revision: "rev-codex",
                    title: "codex"
                ),
                stateMini(
                    sessionID: "thread-main",
                    surface: "devin",
                    seq: 14,
                    revision: "rev-devin",
                    title: "devin"
                ),
                stateMini(
                    sessionID: "grok-thread",
                    surface: "grok-build",
                    seq: 10,
                    revision: "rev-grok",
                    title: "grok"
                ),
            ],
            serverTime: serverTime
        )

        let siriProjection = try reduceSiriSessionEntities(
            snapshot: projection.snapshot,
            assistantSurfaceOrder: ["codex", "devin", "grok-build"]
        )

        #expect(
            siriProjection.entries.map { "\($0.surface):\($0.sessionIndex)" }
                == ["devin:0", "grok-build:0", "codex:0"]
        )
    }

    @Test
    func cachedSnapshotFailureDoesNotReportConnected() throws {
        let projection = try reduceSnapshotLoadFailure(
            mappedErrorState: "offline",
            currentConnectionState: "connected",
            hasUsableSnapshot: true,
            hasServerHealth: true,
            hasReachedBaseUrl: true
        )

        #expect(projection.connectionState == "offline")
        #expect(!projection.preservedConnectedState)
        #expect(projection.shouldClearRouteState)
        #expect(projection.shouldSuppressError)
    }

    @Test
    func sessionManagerStartsWithoutSwiftStateMutationHooks() throws {
        let manager = try temporarySessionManager()

        let stateSnapshot = try manager.stateSnapshot()
        let localSnapshot = try manager.localSnapshot()

        #expect(stateSnapshot.latestSeq == 0)
        #expect(stateSnapshot.stateMinis.isEmpty)
        #expect(localSnapshot.latestSeq == 0)
        #expect(localSnapshot.sessions.isEmpty)
    }

    private func stateMini(
        sessionID: String,
        surface: String = "codex",
        seq: Int64,
        revision: String,
        title: String
    ) -> ClientStateMini {
        ClientStateMini(
            sessionId: sessionID,
            assistantSurface: surface,
            seq: seq,
            revision: revision,
            payloadJson: #"{"id":"\#(sessionID)","sessionId":"\#(sessionID)","assistantSurface":"\#(surface)","title":"\#(title)","ref":"\#(sessionID)","status":"active","lastActivityAtMs":\#(seq)}"#
        )
    }

    private func pendingCommand(
        kind: ClientPendingCommandKind,
        threadID: String,
        preset: String = "",
        assistantSurface: String = ""
    ) -> ClientPendingCommand {
        ClientPendingCommand(
            kind: kind,
            clientMutationId: "swift-test-\(kind)-\(threadID)",
            threadId: threadID,
            preset: preset,
            assistantSurface: assistantSurface,
            promptIntent: "",
            prompt: "",
            notificationId: "",
            notificationTargetIds: [],
            archived: false,
            attemptCount: 0
        )
    }

    private func temporarySessionManager() throws -> LooperClientCoreSessionManager {
        let directoryURL = FileManager.default.temporaryDirectory.appendingPathComponent(
            "looper-client-core-tests-\(UUID().uuidString)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return try LooperClientCoreSessionManager(
            fileURL: directoryURL.appendingPathComponent("state-minis.json")
        )
    }
}

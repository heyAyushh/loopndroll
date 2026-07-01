import Foundation
import LooperClientCore
import LooperCompanionCore
import Testing
@testable import Looper

@Suite("CompanionSessionMiniLocalFirstTests")
struct CompanionSessionMiniLocalFirstTests {
    init() {
        // Surface selection persists across app launches by design; tests must not
        // inherit a surface persisted by an earlier test in the same process.
        CompanionSnapshotStateStore.clearPersistedAssistantSurfaceForTesting()
    }

    private enum Constants {
        static let cachedThreadID = "cached-thread"
        static let fallbackThreadID = "fallback-thread"
        static let timestamp = "2026-06-24T00:00:00Z"
        static let heartbeatTimestamp = "2026-06-24T00:00:15Z"
        static let preAckLocalPaintProbeNanoseconds: UInt64 = 20_000_000
        static let delayedModeDrainProbeNanoseconds: UInt64 = 300_000_000
        static let slowQuickActionHandlerNanoseconds: UInt64 = 250_000_000
        static let quickActionSubmitBudgetNanoseconds: UInt64 = 100_000_000
        static let assistantSurfaceAckPollNanoseconds: UInt64 = 10_000_000
        static let assistantSurfaceAckPollAttempts = 20
    }

    @MainActor
    @Test
    func testAppModelRestoresCachedSessionMinisBeforeNetwork() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 7,
            records: [
                Self.miniRecord(session: cachedSession, seq: 7, revision: "mini-revision-7"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())

        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini")
        #expect(model.viewState.activeSessions.map { $0.id } == [Constants.cachedThreadID])
        #expect(model.connectionState == .connecting)
        #expect(model.viewState.connectivityHeadline == "Looper")
        #expect(model.viewState.connectivityStatusLabel == "Local")
        #expect(model.viewState.connectionRoutePresentation == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testConnectionCardUsesLocalStateWhileStreamCatchesUp() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 9,
            records: [
                Self.miniRecord(session: cachedSession, seq: 9, revision: "mini-revision-9"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        model.connectionState = .connecting
        #expect(model.viewState.connectivityHeadline == "Looper")
        #expect(model.viewState.connectivityStatusLabel == "Local")
        #expect(model.viewState.connectivitySummary == "Showing local sessions while Looper reconnects.")
        #expect(model.viewState.deviceHubAccessStatusLabel == "Local")
        #expect(model.viewState.deviceHubAPIStatusLabel == "Local")

        model.connectionState = .offline
        #expect(model.viewState.connectivityHeadline == "Looper")
        #expect(model.viewState.connectivityStatusLabel == "Local")
        #expect(model.viewState.connectivitySummary == "Showing local sessions; commands will retry when Looper reconnects.")
        #expect(model.viewState.deviceHubAccessStatusLabel == "Local")
        #expect(model.viewState.deviceHubAPIStatusLabel == "Local")
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testConnectedCardDoesNotExposeStreamOrSnapshotAge() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 9,
            records: [
                Self.miniRecord(session: cachedSession, seq: 9, revision: "mini-revision-9"),
            ]
        )
        var networkSnapshot = Self.networkSnapshot()
        networkSnapshot.workStatus = MobileWorkStatusSummary(
            goalCount: 1,
            runningGoalCount: 1,
            automationCount: 0,
            activeAutomationCount: 0,
            coveredAutomationCount: 0,
            runningGoals: [
                MobileWorkStatusGoal(
                    id: "goal-1",
                    title: "Keep realtime honest",
                    status: "running",
                    targetThreadId: Constants.cachedThreadID,
                    targetKnown: true,
                    updatedAtMs: nil,
                    tokensUsed: nil,
                    tokenBudget: nil,
                    timeUsedSeconds: nil
                ),
            ],
            activeAutomations: []
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: networkSnapshot)
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.snapshotState.applySnapshot(networkSnapshot)

        model.realtimeServerTime = Constants.heartbeatTimestamp
        model.realtimeLatestSeq = 10
        model.connectionState = .connected
        model.activeSessionRouteBaseURL = URL(string: "http://192.168.2.10:8766")
        model.serverHealth = CompanionServerHealth(
            ok: true,
            baseURL: "http://stale-http-route.local:8765",
            baseURLs: ["http://stale-http-route.local:8765"],
            serverTime: Constants.heartbeatTimestamp
        )

        #expect(model.realtimeServerTime == Constants.heartbeatTimestamp)
        #expect(!model.viewState.connectivitySummary.localizedCaseInsensitiveContains("stream"))
        #expect(!model.viewState.connectivitySummary.contains("synced "))
        #expect(!model.viewState.connectivitySummary.contains("running goal"))
        #expect(!model.viewState.connectivitySummary.contains("API running"))
        #expect(!model.viewState.connectivitySummary.contains("stale-http-route"))
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testSurfaceEmptyStateUsesLocalSnapshotDuringReconnect() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 13,
            records: [
                Self.miniRecord(session: cachedSession, seq: 13, revision: "mini-revision-13"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        _ = model.selectAssistantSurface(.claudeCode)
        try await Self.waitForSelectedAssistantSurface(.claudeCode, model: model)
        model.connectionState = .connecting
        #expect(model.viewState.sessionsUnavailableTitle == "No Claude Code Sessions")
        #expect(model.viewState.sessionsUnavailableSystemImage == "tray")
        #expect(
            model.viewState.sessionsEmptyDescription ==
                "Claude Code sessions appear here separately from Codex when Claude is running on your Mac."
        )

        model.connectionState = .offline
        #expect(model.viewState.sessionsUnavailableTitle == "No Claude Code Sessions")
        #expect(model.viewState.sessionsUnavailableSystemImage == "tray")
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testCurrentSiriSessionSelectionAppliesAfterRuntimeAccept() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 10,
            records: [
                Self.miniRecord(session: cachedSession, seq: 10, revision: "mini-revision-10"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        await model.markCurrentSiriSession(cachedSession)

        #expect(model.snapshot?.globalSettings.siriCurrentSessionId == Constants.cachedThreadID)
        #expect(model.snapshot?.globalSettings.siriCurrentAssistantSurface == .codex)
        #expect(model.snapshot?.globalSettings.siriCurrentUpdatedAtMs != nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @Test
    func testRealtimeEndpointResolverPassesH3HealthMetadataToClientCore() throws {
        let endpoints = CompanionRealtimeEndpointResolver.endpoints(
            configuredBaseURLs: [try #require(URL(string: "http://127.0.0.1:8765"))],
            health: CompanionServerHealth(
                ok: true,
                baseURL: "http://100.95.2.4:8765",
                baseURLs: ["http://100.95.2.4:8765", "http://192.168.1.33:8765"],
                grpcBaseURL: "http://100.95.2.4:8766",
                grpcBaseURLs: ["http://192.168.1.33:8766"],
                grpcH3BaseURL: "https://100.95.2.4:8766",
                grpcH3BaseURLs: ["https://192.168.1.33:8766"],
                grpcH3CertificateSha256: "sha256:pin",
                serverTime: Constants.timestamp
            )
        )

        // Endpoints run through the shared attemptability filter, which on the
        // simulator prioritizes loopback (and on-device drops it entirely).
        #expect(endpoints.map(\.transport) == [.h3, .h3, .h2, .h2, .h2])
        #expect(endpoints.map(\.url) == [
            "https://100.95.2.4:8766",
            "https://192.168.1.33:8766",
            "http://127.0.0.1:8766",
            "http://100.95.2.4:8766",
            "http://192.168.1.33:8766",
        ])
        #expect(endpoints[0].recoveryBaseUrl == "http://100.95.2.4:8765")
        #expect(endpoints[1].recoveryBaseUrl == "http://192.168.1.33:8765")
        #expect(endpoints[0].h3CertificateSha256 == "sha256:pin")
        #expect(endpoints[0].h3CertificateSpkiSha256.isEmpty)
    }

    @Test
    func testRealtimeEndpointResolverKeepsH2OnlyHealthUsable() throws {
        let data = try JSONSerialization.data(withJSONObject: [
            "ok": true,
            "baseURL": "http://100.95.2.4:8765",
            "baseURLs": ["http://100.95.2.4:8765"],
            "grpcBaseURL": "http://100.95.2.4:8766",
            "grpcBaseURLs": ["http://192.168.1.33:8766"],
            "serverTime": Constants.timestamp,
        ])
        let health = try JSONDecoder().decode(CompanionServerHealth.self, from: data)
        let endpoints = CompanionRealtimeEndpointResolver.endpoints(
            configuredBaseURLs: [try #require(URL(string: "http://127.0.0.1:8765"))],
            health: health
        )

        #expect(endpoints.map(\.transport) == [.h2, .h2, .h2])
        #expect(endpoints.map(\.url) == [
            "http://127.0.0.1:8766",
            "http://100.95.2.4:8766",
            "http://192.168.1.33:8766",
        ])
        #expect(endpoints.allSatisfy { $0.h3CertificateSha256.isEmpty })
    }

    @Test
    func testDeadH3LiveH2FallbackFixtureLeavesRustEnoughRouteData() throws {
        let health = CompanionServerHealth(
            ok: true,
            baseURL: "http://127.0.0.1:8765",
            baseURLs: ["http://127.0.0.1:8765"],
            grpcBaseURL: "http://127.0.0.1:8766",
            grpcBaseURLs: [],
            grpcH3BaseURL: "https://127.0.0.1:9",
            grpcH3BaseURLs: [],
            grpcH3CertificateSha256: "sha256:0000000000000000000000000000000000000000000000000000000000000000",
            serverTime: Constants.timestamp
        )

        let endpoints = CompanionRealtimeEndpointResolver.endpoints(
            configuredBaseURLs: [],
            health: health
        )

        #expect(endpoints.map(\.transport) == [.h3, .h2])
        #expect(endpoints.map(\.url) == [
            "https://127.0.0.1:9",
            "http://127.0.0.1:8766",
        ])
        #expect(endpoints.map(\.recoveryBaseUrl) == [
            "http://127.0.0.1:8765",
            "http://127.0.0.1:8765",
        ])
        #expect(endpoints[0].h3CertificateSha256.hasPrefix("sha256:"))
        #expect(endpoints[1].h3CertificateSha256.isEmpty)
    }

    @MainActor
    @Test
    func testAssistantSurfaceSwitchPaintsBeforeRuntimeAcceptWithoutSnapshotLoad() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: cachedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        #expect(model.selectAssistantSurface(.devin))

        #expect(model.viewState.selectedAssistantSurface == .devin)
        #expect(Self.pendingCommands(in: runtime, kind: .setAssistantSurface).isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testAssistantSurfaceSwitchUsesCachedSurfaceMinisImmediately() async throws {
        let codexSession = Self.sessionSummary(
            id: "codex-thread",
            title: "Codex Mini",
            ref: "C1",
            status: .active
        )
        let zedSession = Self.sessionSummary(
            id: "zed-thread",
            title: "Zed Mini",
            ref: "Z1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 10,
            records: [
                Self.miniRecord(
                    session: codexSession,
                    assistantSurface: .codex,
                    seq: 10,
                    revision: "codex-revision"
                ),
                Self.miniRecord(
                    session: zedSession,
                    assistantSurface: .zed,
                    seq: 9,
                    revision: "zed-revision"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        #expect(model.viewState.selectedAssistantSurface == .codex)
        #expect(model.viewState.activeSessions.map(\.id) == ["codex-thread"])
        #expect(model.viewState.allSessions.map(\.id).sorted() == ["codex-thread", "zed-thread"])

        #expect(model.selectAssistantSurface(.zed))

        #expect(service.loadSnapshotCallCount == 0)
        #expect(model.viewState.selectedAssistantSurface == .zed)
        #expect(model.viewState.activeSessions.map(\.id) == ["zed-thread"])
        #expect(model.viewState.allSessions.map(\.id).sorted() == ["codex-thread", "zed-thread"])
        #expect(model.viewState.assistantSurface(for: "zed-thread") == .zed)
        #expect(Self.pendingCommands(in: runtime, kind: .setAssistantSurface).isEmpty)
    }

    @MainActor
    @Test
    func testRapidAssistantSurfaceSwitchUsesLastLocalSelectionAndCanonicalIndex() async throws {
        let codexSession = Self.sessionSummary(
            id: "codex-thread",
            title: "Codex Mini",
            ref: "C1",
            status: .active
        )
        let claudeSession = Self.sessionSummary(
            id: "claude-thread",
            title: "Claude Mini",
            ref: "CL1",
            status: .active
        )
        let devinSession = Self.sessionSummary(
            id: "devin-thread",
            title: "Devin Mini",
            ref: "D1",
            status: .active
        )
        let grokSession = Self.sessionSummary(
            id: "grok-thread",
            title: "Grok Mini",
            ref: "G1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 14,
            records: [
                Self.miniRecord(
                    session: codexSession,
                    assistantSurface: .codex,
                    seq: 14,
                    revision: "codex-revision"
                ),
                Self.miniRecord(
                    session: claudeSession,
                    assistantSurface: .claudeCode,
                    seq: 13,
                    revision: "claude-revision"
                ),
                Self.miniRecord(
                    session: devinSession,
                    assistantSurface: .devin,
                    seq: 12,
                    revision: "devin-revision"
                ),
                Self.miniRecord(
                    session: grokSession,
                    assistantSurface: .grokBuild,
                    seq: 11,
                    revision: "grok-revision"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        _ = model.selectAssistantSurface(.claudeCode)
        _ = model.selectAssistantSurface(.devin)
        _ = model.selectAssistantSurface(.grokBuild)
        #expect(model.selectAssistantSurface(.codex))

        #expect(model.viewState.selectedAssistantSurface == .codex)
        #expect(model.viewState.activeSessions.map(\.id) == ["codex-thread"])
        #expect(model.viewState.allSessions.map(\.id).sorted() == [
            "claude-thread",
            "codex-thread",
            "devin-thread",
            "grok-thread",
        ])
        #expect(model.viewState.assistantSurface(for: "codex-thread") == .codex)
        #expect(model.viewState.assistantSurface(for: "claude-thread") == .claudeCode)
        #expect(model.viewState.assistantSurface(for: "devin-thread") == .devin)
        #expect(model.viewState.assistantSurface(for: "grok-thread") == .grokBuild)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(Self.pendingCommands(in: runtime, kind: .setAssistantSurface).isEmpty)
    }

    @MainActor
    @Test
    func testAssistantSurfaceSwitchPreservesLocalMiniSourceWithoutCommandsOrReload() async throws {
        let latestSeq: Int64 = 30
        let codexSession = Self.sessionSummary(
            id: "codex-thread",
            title: "Codex Mini",
            ref: "C1",
            status: .active,
            effectiveMode: .awaitReply
        )
        let devinSession = Self.sessionSummary(
            id: "devin-thread",
            title: "Devin Mini",
            ref: "D1",
            status: .active,
            effectiveMode: .awaitReply
        )
        let zedSession = Self.sessionSummary(
            id: "zed-thread",
            title: "Zed Mini",
            ref: "Z1",
            status: .active,
            effectiveMode: .awaitReply
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: latestSeq,
            records: [
                Self.miniRecord(
                    session: codexSession,
                    assistantSurface: .codex,
                    seq: latestSeq,
                    revision: "codex-revision"
                ),
                Self.miniRecord(
                    session: devinSession,
                    assistantSurface: .devin,
                    seq: latestSeq - 1,
                    revision: "devin-revision"
                ),
                Self.miniRecord(
                    session: zedSession,
                    assistantSurface: .zed,
                    seq: latestSeq - 2,
                    revision: "zed-revision"
                ),
            ]
        )
        let localMiniSourceIDs = try Self.localMiniSourceIDs(in: runtime)
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        #expect(localMiniSourceIDs == ["codex-thread", "devin-thread", "zed-thread"])
        #expect(try Self.localMiniSourceIDs(in: runtime) == localMiniSourceIDs)
        #expect(try #require(model.snapshot).sessionsAcrossSurfaces.map(\.id).sorted() == localMiniSourceIDs)
        #expect(model.viewState.selectedAssistantSurface == .codex)
        #expect(model.viewState.activeSessions.map(\.id) == ["codex-thread"])
        #expect(model.viewState.allSessions.map(\.id).sorted() == localMiniSourceIDs)

        #expect(model.selectAssistantSurface(.devin))

        #expect(service.loadSnapshotCallCount == 0)
        #expect(service.loadServerHealthCallCount == 0)
        #expect(model.viewState.selectedAssistantSurface == .devin)
        #expect(model.viewState.activeSessions.map(\.id) == ["devin-thread"])
        #expect(model.viewState.allSessions.map(\.id).sorted() == localMiniSourceIDs)
        #expect(try Self.localMiniSourceIDs(in: runtime) == localMiniSourceIDs)
        #expect(try #require(model.snapshot).sessionsAcrossSurfaces.map(\.id).sorted() == localMiniSourceIDs)
        #expect(Self.pendingCommands(in: runtime, kind: .setAssistantSurface).isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(service.loadServerHealthCallCount == 0)
    }

    @MainActor
    @Test
    func testAuthoritativeSmallerSnapshotRemovesOmittedLocalMini() throws {
        let codexSession = Self.sessionSummary(
            id: "codex-thread",
            title: "Codex Mini",
            ref: "C1",
            status: .active
        )
        let removedSession = Self.sessionSummary(
            id: "removed-thread",
            title: "Removed Mini",
            ref: "R1",
            status: .active
        )
        let store = CompanionSnapshotStateStore()
        let localSnapshot = Self.mobileSnapshot(
            revision: "local-revision",
            sessions: [codexSession, removedSession],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [codexSession, removedSession],
            ]
        )
        let authoritativeSnapshot = Self.mobileSnapshot(
            revision: "authoritative-shrink-revision",
            sessions: [codexSession],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [codexSession],
            ]
        )

        store.applySnapshot(localSnapshot)
        store.applySnapshot(authoritativeSnapshot)

        #expect(store.allSessionSections.active.map(\.id) == ["codex-thread"])
        #expect(store.snapshot?.sessionsAcrossSurfaces.map(\.id) == ["codex-thread"])
        #expect(store.snapshot?.session(withID: "removed-thread") == nil)
    }

    @MainActor
    @Test
    func testCappedNetworkSnapshotFailsClosedWithoutExplicitPartialSignal() throws {
        let localSessionCount = 300
        let cappedNetworkSessionCount = 250
        let sessionsPerSurface = localSessionCount / CompanionAssistantSurface.allCases.count
        let localSurfaceSessions = Dictionary(
            uniqueKeysWithValues: CompanionAssistantSurface.allCases.map { surface in
                (
                    surface.rawValue,
                    (0..<sessionsPerSurface).map { index in
                        Self.sessionSummary(
                            id: "local-\(surface.rawValue)-\(index)",
                            title: "Local \(surface.rawValue) \(index)",
                            ref: "L\(index)",
                            status: index.isMultiple(of: 2) ? .active : .waiting
                        )
                    }
                )
            }
        )
        let localSessions = CompanionAssistantSurface.allCases.flatMap { surface in
            localSurfaceSessions[surface.rawValue] ?? []
        }
        let cappedNetworkSessions = (0..<cappedNetworkSessionCount).map { index in
            Self.sessionSummary(
                id: "network-codex-\(index)",
                title: "Network Codex \(index)",
                ref: "N\(index)",
                status: .active
            )
        }
        let store = CompanionSnapshotStateStore()
        let localSnapshot = Self.mobileSnapshot(
            revision: "local-revision",
            sessions: localSessions,
            surfaceSessions: localSurfaceSessions
        )
        let cappedNetworkSnapshot = Self.mobileSnapshot(
            revision: "network-capped-revision",
            sessions: cappedNetworkSessions,
            surfaceSessions: [CompanionAssistantSurface.codex.rawValue: cappedNetworkSessions]
        )

        store.applySnapshot(localSnapshot)
        store.applySnapshot(cappedNetworkSnapshot)

        #expect(store.allSessionSections.active.count == cappedNetworkSessionCount)
        #expect(store.sessionSections.active.count == cappedNetworkSessionCount)
        #expect(store.snapshot?.sessions.count == cappedNetworkSessionCount)
        #expect(store.sessions(for: .devin).isEmpty)
        #expect(store.sessions(for: .zed).isEmpty)
        #expect(store.sessions(for: .grokBuild).isEmpty)

        store.applyVisibleAssistantSurface(.devin)

        #expect(store.selectedAssistantSurface == .devin)
        #expect(store.sessionSections.active.isEmpty)
        #expect(store.snapshot?.sessions.isEmpty == true)
    }

    @MainActor
    @Test
    func testAssistantSurfaceSwitchAppliesAfterRuntimeAccept() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: cachedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        _ = model.selectAssistantSurface(.devin)
        try await Self.waitForSelectedAssistantSurface(.devin, model: model)

        #expect(model.viewState.selectedAssistantSurface == .devin)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(model.connectionState == .connecting)
        #expect(model.errorMessage == nil)
        #expect(model.viewState.connectivityHeadline == "Looper")
    }

    @MainActor
    @Test
    func testRapidAssistantSurfaceSwitchesResolveToLatestSelection() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: cachedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        #expect(model.selectAssistantSurface(.claudeCode))
        #expect(model.selectAssistantSurface(.devin))
        #expect(model.viewState.selectedAssistantSurface == .devin)

        #expect(Self.pendingCommands(in: runtime, kind: .setAssistantSurface).isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testHundredAssistantSurfaceSwitchesStayViewOnly() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: cachedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let surfaces = CompanionAssistantSurface.allCases
        var didSelectLatestSurface = false

        for index in 0..<100 {
            let surface = surfaces[index % surfaces.count]
            didSelectLatestSurface = model.selectAssistantSurface(surface)
            try await Task.sleep(for: .milliseconds(20))
        }

        #expect(didSelectLatestSurface)
        #expect(model.viewState.selectedAssistantSurface == .zed)

        #expect(Self.pendingCommands(in: runtime, kind: .setAssistantSurface).isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testSiriAndSettingsStateChangesOnlyAfterAcceptedCommand() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: cachedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        let didMarkCurrent = await model.markCurrentSiriSession(cachedSession)
        let didSetDefault = await model.setSiriDefaultSession(cachedSession)
        let didSavePrompt = await model.saveDefaultPrompt("Continue safely")
        let didSelectSurface = model.selectAssistantSurface(.devin)

        #expect(didMarkCurrent)
        #expect(didSetDefault)
        #expect(didSavePrompt)
        #expect(didSelectSurface)
        #expect(model.snapshot?.globalSettings.siriCurrentSessionId == Constants.cachedThreadID)
        #expect(model.snapshot?.globalSettings.siriDefaultSessionId == Constants.cachedThreadID)
        #expect(model.snapshot?.globalSettings.defaultPrompt == "Continue safely")
        #expect(model.viewState.selectedAssistantSurface == .devin)
        #expect(
            runtime.pendingCommands().map(\.kind) == [
                .setSiriCurrentSession,
                .setSiriDefaultSession,
                .saveDefaultPrompt,
            ]
        )
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testAcceptedSettingsCommandsProjectWithoutSourceSnapshot() async throws {
        CompanionSnapshotCache.clear()
        let runtime = try Self.temporarySessionRuntime()
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let targetSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )

        let didMarkCurrent = await model.markCurrentSiriSession(targetSession)
        let didSetDefault = await model.setSiriDefaultSession(targetSession)
        let didSavePrompt = await model.saveDefaultPrompt("Continue safely")

        #expect(didMarkCurrent)
        #expect(didSetDefault)
        #expect(didSavePrompt)
        #expect(model.snapshot?.globalSettings.siriCurrentSessionId == Constants.cachedThreadID)
        #expect(model.snapshot?.globalSettings.siriDefaultSessionId == Constants.cachedThreadID)
        #expect(model.snapshot?.globalSettings.defaultPrompt == "Continue safely")
        #expect(model.snapshot?.sessions.isEmpty == true)
        #expect(
            runtime.pendingCommands().map(\.kind) == [
                .setSiriCurrentSession,
                .setSiriDefaultSession,
                .saveDefaultPrompt,
            ]
        )
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testSiriClientReadsAcceptedDefaultSessionFromPendingClientCoreCommand() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: cachedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let client = LooperSiriSessionClient(
            service: service,
            sessionRuntime: runtime
        )

        try await client.saveDefaultSiriSession(
            LooperSessionEntity(session: cachedSession, assistantSurface: .codex)
        )
        let defaultEntity = try await client.defaultSiriSessionEntity()

        #expect(defaultEntity.sessionID == Constants.cachedThreadID)
        #expect(defaultEntity.assistantSurfaceRawValue == CompanionAssistantSurface.codex.rawValue)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testSiriDetailUsesEntitySurfaceFromLocalCoreSnapshot() async throws {
        let sharedThreadID = "shared-thread"
        var codexSession = Self.sessionSummary(
            id: sharedThreadID,
            title: "Codex Older Mini",
            ref: "C1",
            status: .active
        )
        codexSession.effectiveMode = .maxTurns1
        var devinSession = Self.sessionSummary(
            id: sharedThreadID,
            title: "Devin Latest Mini",
            ref: "D1",
            status: .active
        )
        devinSession.effectiveMode = .awaitReply
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 12,
            records: [
                Self.miniRecord(
                    session: codexSession,
                    assistantSurface: .codex,
                    seq: 8,
                    revision: "codex-revision-8"
                ),
                Self.miniRecord(
                    session: devinSession,
                    assistantSurface: .devin,
                    seq: 12,
                    revision: "devin-revision-12"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let client = LooperSiriSessionClient(
            service: service,
            sessionRuntime: runtime
        )

        let detail = try await client.loadSessionDetail(
            for: LooperSessionEntity(session: devinSession, assistantSurface: .devin)
        )

        #expect(detail.title == "Devin Latest Mini")
        #expect(detail.effectiveMode == .awaitReply)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testSiriDetailResolvesFromLocalMiniWhenHTTPUnavailableAndMarksDegradedContent() async throws {
        var localSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Fresh Local Mini",
            ref: "L1",
            status: .active
        )
        localSession.assistantPreview = "Local mini preview"
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 20,
            records: [
                Self.miniRecord(
                    session: localSession,
                    assistantSurface: .codex,
                    seq: 20,
                    revision: "local-mini-revision-20"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.loadSnapshotError = URLError(.notConnectedToInternet)
        let client = LooperSiriSessionClient(
            service: service,
            sessionRuntime: runtime
        )
        let identifier = LooperSessionEntityIdentifier(
            assistantSurface: .codex,
            sessionID: Constants.cachedThreadID
        )

        let entity = try #require(try await client.entities(for: [identifier.rawValue]).first)
        let detail = try await client.loadSessionDetail(for: entity)

        #expect(entity.title == "Fresh Local Mini")
        #expect(detail.title == "Fresh Local Mini")
        #expect(detail.assistantPreview == "Local mini preview")
        #expect(detail.contentStatus == .localMiniOnly)
        #expect(detail.isContentDegraded)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testPendingSiriOpenUsesRequestedLocalSurface() async throws {
        let sharedThreadID = "shared-open-thread"
        let codexSession = Self.sessionSummary(
            id: sharedThreadID,
            title: "Codex Open Mini",
            ref: "C1",
            status: .active
        )
        let devinSession = Self.sessionSummary(
            id: sharedThreadID,
            title: "Devin Open Mini",
            ref: "D1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 12,
            records: [
                Self.miniRecord(
                    session: codexSession,
                    assistantSurface: .codex,
                    seq: 8,
                    revision: "codex-open-revision-8"
                ),
                Self.miniRecord(
                    session: devinSession,
                    assistantSurface: .devin,
                    seq: 12,
                    revision: "devin-open-revision-12"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        try LooperSiriOpenSessionRequestStore.save(
            LooperSiriOpenSessionRequest(
                sessionID: sharedThreadID,
                assistantSurfaceRawValue: CompanionAssistantSurface.devin.rawValue
            )
        )
        defer {
            _ = LooperSiriOpenSessionRequestStore.drain()
        }

        await model.continueFromPendingSiriOpenSessionRequest()

        #expect(model.viewState.selectedAssistantSurface == .devin)
        #expect(model.pendingOpenSessionID == sharedThreadID)
        #expect(model.viewState.detail(for: sharedThreadID)?.title == "Devin Open Mini")
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testPendingSiriOpenDoesNotPublishMissingSessionWithoutLocalTruth() async throws {
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 14,
            records: [
                Self.miniRecord(
                    session: Self.sessionSummary(
                        id: Constants.cachedThreadID,
                        title: "Cached Mini",
                        ref: "C1",
                        status: .active
                    ),
                    assistantSurface: .codex,
                    seq: 14,
                    revision: "cached-open-revision-14"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let missingThreadID = "missing-thread"
        try LooperSiriOpenSessionRequestStore.save(
            LooperSiriOpenSessionRequest(
                sessionID: missingThreadID,
                assistantSurfaceRawValue: CompanionAssistantSurface.codex.rawValue
            )
        )
        defer {
            _ = LooperSiriOpenSessionRequestStore.drain()
        }

        await model.continueFromPendingSiriOpenSessionRequest()

        #expect(model.pendingOpenSessionID == nil)
        #expect(model.viewState.detail(for: missingThreadID) == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testQuickActionOpenDoesNotPublishMissingSessionWithoutLocalTruth() async throws {
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 16,
            records: [
                Self.miniRecord(
                    session: Self.sessionSummary(
                        id: Constants.cachedThreadID,
                        title: "Cached Mini",
                        ref: "C1",
                        status: .active
                    ),
                    assistantSurface: .codex,
                    seq: 16,
                    revision: "cached-quick-action-revision-16"
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let missingThreadID = "missing-quick-action-thread"

        await model.performQuickAction(.openSession, sessionID: missingThreadID)

        #expect(model.pendingOpenSessionID == nil)
        #expect(model.viewState.detail(for: missingThreadID) == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testCachedSnapshotAppliesPendingDetailAndListCommands() async throws {
        let archivedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let deletedSession = Self.sessionSummary(
            id: "deleted-thread",
            title: "Deleted Mini",
            ref: "D1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: archivedSession, seq: 8, revision: "mini-revision-8"),
                Self.miniRecord(session: deletedSession, seq: 8, revision: "mini-revision-8"),
            ]
        )

        try await runtime.setMode(threadID: Constants.cachedThreadID, preset: .maxTurns2)
        try await runtime.setSessionArchived(threadID: Constants.cachedThreadID, archived: true)
        try await runtime.deleteSession(threadID: deletedSession.id)
        let snapshot = try #require(try runtime.cachedSnapshot())
        let detail = try #require(snapshot.session(withID: Constants.cachedThreadID))

        #expect(detail.effectiveMode == .maxTurns2)
        #expect(detail.isArchived)
        #expect(detail.status == .archived)
        #expect(snapshot.session(withID: deletedSession.id) == nil)
        #expect(snapshot.sessionsAcrossSurfaces.map(\.id) == [Constants.cachedThreadID])
    }

    @MainActor
    @Test
    func testConfiguredRouteDoesNotRenderAsConnectedBeforeLiveSessionEndpoint() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 9,
            records: [
                Self.miniRecord(session: cachedSession, seq: 9, revision: "mini-revision-9"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.configuredBaseURL = "http://100.95.2.4:8765"
        model.reachedBaseURL = URL(string: "http://192.168.2.10:8765")
        model.connectionState = .connected

        #expect(model.viewState.connectionRoutePresentation == nil)

        model.activeSessionRouteBaseURL = URL(string: "http://100.95.2.4:8766")

        let presentation = try #require(model.viewState.connectionRoutePresentation)
        #expect(presentation.route == .tailscale)
        #expect(presentation.title == "Tailscale")
    }

    @MainActor
    @Test
    func testLiveSessionStreamWinsOverSnapshotTimeout() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 9,
            records: [
                Self.miniRecord(session: cachedSession, seq: 9, revision: "mini-revision-9"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.loadSnapshotError = URLError(.timedOut)
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let liveRoute = try #require(URL(string: "http://100.95.2.4:8766"))
        model.connectionState = .connected
        model.realtimeStreamIsLive = true
        model.activeSessionRouteBaseURL = liveRoute

        await model.loadSnapshot()

        #expect(model.connectionState == .connected)
        #expect(model.errorMessage == nil)
        #expect(model.activeConnectionRouteBaseURL == liveRoute)
        #expect(model.viewState.connectionRoutePresentation?.route == .tailscale)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testLiveSessionStreamWinsOverSuccessfulHttpSnapshot() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.connectionState = .connected
        model.realtimeStreamIsLive = true
        model.realtimeLatestSeq = 11

        await model.loadSnapshot()

        #expect(model.connectionState == .connected)
        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini")
        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID) == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testPullRefreshRecoversFreshMinisWhenSessionStreamIsLive() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let recoveredSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Recovered Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 13,
            records: [
                Self.miniRecord(session: recoveredSession, seq: 13, revision: "mini-revision-13"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.realtimeStreamIsLive = true
        model.realtimeLatestSeq = 12
        model.snapshot = Self.mobileSnapshot(
            revision: "mini-revision-12",
            sessions: [cachedSession]
        )

        await model.reconcileLocalSessionState(reason: .sessionsPullRefresh)

        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Recovered Mini")
        #expect(model.realtimeLatestSeq == 13)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testFallbackTimerDoesNotReplayLocalCacheWhenSessionStreamIsLive() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 14,
            records: [
                Self.miniRecord(session: cachedSession, seq: 14, revision: "mini-revision-14"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.realtimeStreamIsLive = true
        model.snapshot = nil

        await model.reconcileLocalSessionState(reason: .fallbackTimer)

        #expect(model.snapshot == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testRepeatedLiveHeartbeatDoesNotInvalidateModelWhenUnchanged() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 14,
            records: [
                Self.miniRecord(session: cachedSession, seq: 14, revision: "mini-revision-14"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let route = try #require(URL(string: "http://192.168.2.10:8766"))

        #expect(model.applyRealtimeStreamLiveness(
            serverTime: Constants.heartbeatTimestamp,
            latestSeq: 14,
            isLive: true,
            endpointURL: route
        ))
        #expect(model.realtimeStreamIsLive)
        #expect(model.realtimeLatestSeq == 14)
        #expect(model.activeConnectionRouteBaseURL == route)
        #expect(model.connectionState == .connected)

        #expect(!model.applyRealtimeStreamLiveness(
            serverTime: Constants.heartbeatTimestamp,
            latestSeq: 14,
            isLive: true,
            endpointURL: route
        ))

        #expect(model.applyRealtimeStreamLiveness(
            serverTime: Constants.heartbeatTimestamp,
            latestSeq: 15,
            isLive: true,
            endpointURL: route
        ))
        #expect(model.realtimeLatestSeq == 15)
    }

    @MainActor
    @Test
    func testStaleLivenessCannotResurrectConnectedState() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 15,
            records: [
                Self.miniRecord(session: cachedSession, seq: 15, revision: "mini-revision-15"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let route = try #require(URL(string: "http://192.168.2.10:8766"))

        #expect(model.applyRealtimeStreamLiveness(
            serverTime: Constants.heartbeatTimestamp,
            latestSeq: 15,
            isLive: false,
            endpointURL: nil
        ))
        #expect(!model.realtimeStreamIsLive)
        #expect(model.connectionState == .connecting)
        #expect(model.activeConnectionRouteBaseURL == nil)

        #expect(!model.applyRealtimeStreamLiveness(
            serverTime: Constants.timestamp,
            latestSeq: 14,
            isLive: true,
            endpointURL: route
        ))
        #expect(!model.realtimeStreamIsLive)
        #expect(model.connectionState == .connecting)
        #expect(model.activeConnectionRouteBaseURL == nil)
        #expect(model.realtimeServerTime == Constants.heartbeatTimestamp)
        #expect(model.realtimeLatestSeq == 15)
    }

    @MainActor
    @Test
    func testStreamRestartLivenessClearsLiveRouteWithoutLoweringSeq() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 15,
            records: [
                Self.miniRecord(session: cachedSession, seq: 15, revision: "mini-revision-15"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let route = try #require(URL(string: "http://192.168.2.10:8766"))

        #expect(model.applyRealtimeStreamLiveness(
            serverTime: Constants.heartbeatTimestamp,
            latestSeq: 15,
            isLive: true,
            endpointURL: route
        ))

        let restart = CompanionSessionMiniController.restartLivenessUpdate()
        #expect(model.applyRealtimeStreamLiveness(
            serverTime: restart.serverTime,
            latestSeq: restart.latestSeq,
            isLive: restart.isLive,
            endpointURL: restart.endpointURL
        ))

        #expect(!model.realtimeStreamIsLive)
        #expect(model.connectionState == .connecting)
        #expect(model.activeConnectionRouteBaseURL == nil)
        #expect(model.realtimeLatestSeq == 15)
    }

    @MainActor
    @Test
    func testDeviceHubAPIStatusDoesNotUseStaleHealthWithoutLiveStream() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 5,
            records: [
                Self.miniRecord(session: cachedSession, seq: 5, revision: "mini-revision-5"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.connectionState = .connecting
        model.serverHealth = CompanionServerHealth(
            ok: true,
            baseURL: "http://127.0.0.1:8765",
            baseURLs: ["http://127.0.0.1:8765"],
            serverTime: Constants.timestamp
        )

        #expect(model.viewState.deviceHubAPIStatusLabel == "Local")

        model.realtimeStreamIsLive = true
        model.connectionState = .connected
        #expect(model.viewState.deviceHubAPIStatusLabel == "Running")
    }

    @MainActor
    @Test
    func testOlderStateMiniCacheCannotReplayOverNewerRenderedState() async throws {
        let staleSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Stale Mini",
            ref: "C1",
            status: .active
        )
        let freshSession = Self.sessionSummary(
            id: "fresh-thread",
            title: "Fresh Mini",
            ref: "F1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: staleSession, seq: 8, revision: "stale-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let freshSnapshot = Self.mobileSnapshot(
            revision: "fresh-revision-20",
            sessions: [freshSession]
        )
        model.realtimeLatestSeq = 20
        model.snapshot = freshSnapshot

        await model.reconcileLocalSessionState(reason: .sessionsPullRefresh)

        #expect(model.realtimeLatestSeq == 20)
        #expect(model.snapshot?.session(withID: freshSession.id)?.title == "Fresh Mini")
        #expect(model.snapshot?.session(withID: Constants.cachedThreadID) == nil)
        #expect(model.viewState.activeSessions.map(\.id) == [freshSession.id])
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testDetailSwitchOpenUsesProjectionBeforeStaleBroaderSummary() async throws {
        let sessionID = "detail-thread"
        let liveReplyTime = "2026-06-24T00:00:45Z"
        var staleSession = Self.sessionSummary(
            id: sessionID,
            title: "Stale Row Summary",
            ref: "D1",
            status: .active
        )
        staleSession.assistantPreview = "stale row preview"
        staleSession.lastMessageAt = Constants.timestamp
        var freshSession = Self.sessionSummary(
            id: sessionID,
            title: "Fresh Detail Summary",
            ref: "D1",
            status: .active
        )
        freshSession.assistantPreview = "fresh local preview"
        freshSession.lastMessageAt = "2026-06-24T00:00:30Z"
        let staleExtraSession = Self.sessionSummary(
            id: "stale-extra-thread",
            title: "Cached Devin Extra",
            ref: "X1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(
                    session: staleSession,
                    assistantSurface: .codex,
                    seq: 8,
                    revision: "stale-detail-revision-8"
                ),
                Self.miniRecord(
                    session: staleExtraSession,
                    assistantSurface: .devin,
                    seq: 8,
                    revision: "stale-devin-revision-8"
                ),
            ],
            latestReplies: [
                Self.latestReply(
                    sessionID: sessionID,
                    messageID: "message-live",
                    text: "Newest projected reply",
                    latestSeq: 21,
                    serverTime: liveReplyTime
                ),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let route = SessionDetailRoute(
            sessionID: sessionID,
            assistantSurface: .codex
        )

        model.realtimeLatestSeq = 20
        model.snapshot = Self.mobileSnapshot(
            revision: "fresh-detail-revision-20",
            sessions: [freshSession],
            surfaceSessions: [CompanionAssistantSurface.codex.rawValue: [freshSession]]
        )

        await model.reconcileLocalSessionState(reason: .sessionsPullRefresh)

        #expect(model.viewState.session(withID: sessionID, assistantSurface: .codex)?.title == "Fresh Detail Summary")
        #expect(model.viewState.session(withID: staleExtraSession.id, assistantSurface: .devin)?.title == "Cached Devin Extra")
        #expect(model.viewState.allSessions.map(\.id).sorted() == [sessionID, staleExtraSession.id])
        for index in 0..<20 {
            let surface: CompanionAssistantSurface = index.isMultiple(of: 2) ? .devin : .codex
            _ = model.selectAssistantSurface(surface)
            let presentation = model.viewState.detailPresentation(for: route)
            #expect(presentation.title == "Fresh Detail Summary")
            #expect(presentation.latestAssistantReply == "Newest projected reply")
            #expect(presentation.lastMessageAt == liveReplyTime)
        }
        #expect(service.loadSnapshotCallCount == 0)
        #expect(service.loadServerHealthCallCount == 0)
    }

    @MainActor
    @Test
    func testDetailPresentationShowsCachedAssistantPreviewUntilLiveReplyProjectionArrives() async throws {
        var cachedSession = Self.sessionSummary(
            id: "preview-thread",
            title: "Preview-backed Detail",
            ref: "P1",
            status: .active
        )
        cachedSession.assistantPreview = "Cached assistant reply from mini"
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 4,
            records: [
                Self.miniRecord(
                    session: cachedSession,
                    seq: 4,
                    revision: "preview-revision-4"
                ),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(
                service: SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
            ),
            sessionRuntime: runtime
        )
        let route = SessionDetailRoute(
            sessionID: cachedSession.id,
            assistantSurface: .codex
        )

        model.snapshot = Self.mobileSnapshot(
            revision: "preview-snapshot",
            sessions: [cachedSession],
            surfaceSessions: [CompanionAssistantSurface.codex.rawValue: [cachedSession]]
        )

        #expect(model.viewState.detailPresentation(for: route).latestAssistantReply == "Cached assistant reply from mini")
    }

    @MainActor
    @Test
    func testOlderStateMiniCacheCannotReplayAfterVisibleSnapshotReset() async throws {
        let staleSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Stale Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 8,
            records: [
                Self.miniRecord(session: staleSession, seq: 8, revision: "stale-revision-8"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.realtimeLatestSeq = 20
        model.snapshot = nil

        await model.reconcileLocalSessionState(reason: .sessionsPullRefresh)

        #expect(model.realtimeLatestSeq == 20)
        #expect(model.snapshot == nil)
        #expect(model.viewState.activeSessions.isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testReconnectingSessionMiniTruthWinsOverSuccessfulHttpSnapshot() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 12,
            records: [
                Self.miniRecord(session: cachedSession, seq: 12, revision: "mini-revision-12"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.connectionState = .connecting
        model.realtimeStreamIsLive = false

        await model.loadSnapshot()

        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini")
        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID) == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testHttpSnapshotFailureRestoresLocalMinisAfterVisibleSnapshotReset() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 14,
            records: [
                Self.miniRecord(session: cachedSession, seq: 14, revision: "mini-revision-14"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.loadSnapshotError = SessionMiniLocalFirstServiceSpy.ServiceError.promptFailed
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.snapshot = nil
        model.connectionState = .connecting
        model.realtimeLatestSeq = 0

        await model.loadSnapshot()

        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini")
        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID) == nil)
        #expect(model.viewState.connectivityStatusLabel == "Local")
        #expect(model.errorMessage == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testKnownSessionCursorDoesNotFillEmptyProjectionFromHttpSnapshot() async throws {
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 12,
            records: []
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.connectionState = .connecting
        model.realtimeStreamIsLive = false

        await model.loadSnapshot()

        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID) == nil)
        #expect(model.viewState.activeSessions.isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testRoutePreferenceSwitchClearsStaleRouteUntilCoreReportsReplacement() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 9,
            records: [
                Self.miniRecord(session: cachedSession, seq: 9, revision: "mini-revision-9"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let liveLANRoute = try #require(URL(string: "http://192.168.2.10:8766"))
        let previousBaseURL = UserDefaults.standard.object(
            forKey: CompanionConfiguration.apiBaseURLOverrideKey
        )
        let previousRoutePreference = UserDefaults.standard.object(
            forKey: CompanionConfiguration.connectionRoutePreferenceKey
        )
        defer {
            Self.restoreUserDefaultsValue(previousBaseURL, key: CompanionConfiguration.apiBaseURLOverrideKey)
            Self.restoreUserDefaultsValue(
                previousRoutePreference,
                key: CompanionConfiguration.connectionRoutePreferenceKey
            )
            model.stopSessionRuntimeSync()
        }

        CompanionConfiguration.storeBaseURLString(
            """
            http://192.168.2.10:8765
            http://100.95.2.4:8765
            """
        )
        CompanionConfiguration.storeConnectionRoutePreference(.tailscale)
        model.connectionState = .connected
        model.realtimeStreamIsLive = true
        model.activeSessionRouteBaseURL = liveLANRoute

        await model.connectionCoordinatorApplyRoutePreference()

        #expect(model.connectionState == .connecting)
        #expect(!model.realtimeStreamIsLive)
        #expect(model.activeConnectionRouteBaseURL == nil)
        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini")
        #expect(model.viewState.connectivityStatusLabel == "Connecting")
        #expect(service.loadSnapshotCallCount == 0)
    }

    @Test
    func testStoredRemoteRoutePreferenceFallsBackToTailscale() {
        let previousRoutePreference = UserDefaults.standard.object(
            forKey: CompanionConfiguration.connectionRoutePreferenceKey
        )
        defer {
            Self.restoreUserDefaultsValue(
                previousRoutePreference,
                key: CompanionConfiguration.connectionRoutePreferenceKey
            )
        }

        UserDefaults.standard.set(
            CompanionConnectionRoutePreference.remote.rawValue,
            forKey: CompanionConfiguration.connectionRoutePreferenceKey
        )

        #expect(CompanionConfiguration.connectionRoutePreference() == .tailscale)
    }

    @MainActor
    @Test
    func testMalformedMiniCacheDoesNotApplyLegacySnapshotFallback() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let storeFileURL = try Self.temporaryStoreFileURL()
        try Self.seedMalformedMiniCache(at: storeFileURL)
        let runtime = try CompanionSessionRuntime(fileURL: storeFileURL)

        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        #expect(model.snapshot == nil)

        await model.loadSnapshot()
        #expect(model.snapshot == nil)
        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID) == nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testOptimisticCommandsUseClientMutationIDsWithoutSnapshotRefresh() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.startSessionRuntimeSyncIfNeeded()

        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        let didSend = await model.sendSessionPrompt("ship it", to: Constants.cachedThreadID)
        let didApplyMode = await modeTask.value
        try await Task.sleep(for: .milliseconds(200))

        #expect(didApplyMode)
        #expect(didSend)
        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.effectiveMode == .maxTurns2)
        #expect(service.loadSnapshotCallCount == 0)
        let modeCommand = try Self.pendingCommand(in: runtime, kind: .setSessionMode)
        let promptCommand = try Self.pendingCommand(in: runtime, kind: .sendSessionPrompt)
        #expect(modeCommand.clientMutationID.hasPrefix("mode-"))
        #expect(promptCommand.clientMutationID.hasPrefix("prompt-"))
        #expect(modeCommand.clientMutationID != promptCommand.clientMutationID)
    }

    @MainActor
    @Test
    func testPromptBehindPendingModeUsesRustCoreCommandsWithoutSnapshotRefresh() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.startSessionRuntimeSyncIfNeeded()

        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        try await Task.sleep(nanoseconds: Constants.preAckLocalPaintProbeNanoseconds)
        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.effectiveMode == .maxTurns2)

        let promptTask = model.beginSendSessionPrompt("ship it", to: Constants.cachedThreadID)
        let didSendPrompt = await promptTask.value
        let didApplyMode = await modeTask.value

        #expect(didSendPrompt)
        #expect(didApplyMode)
        #expect(service.loadSnapshotCallCount == 0)
        var modeCommand = try Self.pendingCommand(in: runtime, kind: .setSessionMode)
        let promptCommand = try Self.pendingCommand(in: runtime, kind: .sendSessionPrompt)
        #expect(modeCommand.clientMutationID.hasPrefix("mode-"))
        #expect(promptCommand.clientMutationID.hasPrefix("prompt-"))
        #expect(modeCommand.clientMutationID != promptCommand.clientMutationID)

        try await Task.sleep(nanoseconds: Constants.delayedModeDrainProbeNanoseconds)
        modeCommand = try Self.pendingCommand(in: runtime, kind: .setSessionMode)
        #expect(Self.pendingCommands(in: runtime, kind: .setSessionMode).count == 1)
        #expect(modeCommand.attemptCount == 1)
    }

    @MainActor
    @Test
    func testOfflinePromptBehindModeDoesNotFallbackToSnapshotRefresh() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        model.startSessionRuntimeSyncIfNeeded()

        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        try await Task.sleep(nanoseconds: Constants.preAckLocalPaintProbeNanoseconds)
        let promptTask = model.beginSendSessionPrompt("ship it", to: Constants.cachedThreadID)
        let didSendPrompt = await promptTask.value
        let didApplyMode = await modeTask.value

        #expect(didSendPrompt)
        #expect(didApplyMode)
        #expect(service.loadSnapshotCallCount == 0)
        let modeCommand = try Self.pendingCommand(in: runtime, kind: .setSessionMode)
        let promptCommand = try Self.pendingCommand(in: runtime, kind: .sendSessionPrompt)
        #expect(modeCommand.clientMutationID.hasPrefix("mode-"))
        #expect(promptCommand.clientMutationID.hasPrefix("prompt-"))
    }

    @MainActor
    @Test
    func testNotificationReplyUsesDurableAckCommandWithoutSnapshotRefresh() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .stopped
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 12,
            records: [
                Self.miniRecord(session: cachedSession, seq: 12, revision: "mini-revision-12"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let notificationID = "notif-cached-1"
        let clientMutationID = SessionMiniLocalFirstServiceSpy.notificationReplyMutationID(
            notificationID: notificationID
        )

        await model.performQuickAction(
            .reply,
            sessionID: Constants.cachedThreadID,
            prompt: "continue from notification",
            notificationID: notificationID
        )

        #expect(service.loadSnapshotCallCount == 0)
        let pendingCommand = try Self.pendingCommand(in: runtime, kind: .submitNotificationReply)
        #expect(pendingCommand.clientMutationID == clientMutationID)
        #expect(pendingCommand.threadID == Constants.cachedThreadID)
        #expect(pendingCommand.notificationID == notificationID)
        #expect(pendingCommand.prompt == "continue from notification")
        #expect(pendingCommand.attemptCount == 1)
    }

    @MainActor
    @Test
    func testNotificationReplyPersistsBeforeHandlerAndDedupesOfflineRetry() async throws {
        let runtime = try Self.temporarySessionRuntime()
        let notificationID = "notif-offline-1"
        let clientMutationID = SessionMiniLocalFirstServiceSpy.notificationReplyMutationID(
            notificationID: notificationID
        )
        let center = SessionQuickActionCenter(
            sessionRuntime: runtime
        )

        await center.submit(
            SessionQuickActionRequest(
                action: .reply,
                sessionID: Constants.cachedThreadID,
                prompt: "offline reply",
                notificationID: notificationID
            )
        )

        var pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.threadID == Constants.cachedThreadID)
        #expect(pendingCommands.first?.notificationID == notificationID)
        #expect(pendingCommands.first?.prompt == "offline reply")
        #expect(pendingCommands.first?.clientMutationID == clientMutationID)
        #expect(pendingCommands.first?.attemptCount == 0)

        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        await model.performQuickAction(
            .reply,
            sessionID: Constants.cachedThreadID,
            prompt: "offline reply",
            notificationID: notificationID
        )

        pendingCommands = runtime.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.attemptCount == 1)
        try runtime.enqueueNotificationReplyCommand(
            notificationID: notificationID,
            threadID: Constants.cachedThreadID,
            prompt: "offline reply",
            assistantSurface: nil,
            clientMutationID: clientMutationID
        )
        #expect(runtime.pendingCommands().count == 1)
    }

    @MainActor
    @Test
    func testNotificationReplyQuickActionSubmitDoesNotWaitForNetworkHandler() async throws {
        let runtime = try Self.temporarySessionRuntime()
        let notificationID = "notif-fast-completion-1"
        let center = SessionQuickActionCenter(
            sessionRuntime: runtime
        )
        center.registerHandler { _ in
            try? await Task.sleep(nanoseconds: Constants.slowQuickActionHandlerNanoseconds)
        }

        let startNanoseconds = DispatchTime.now().uptimeNanoseconds
        await center.submit(
            SessionQuickActionRequest(
                action: .reply,
                sessionID: Constants.cachedThreadID,
                prompt: "fast reply",
                notificationID: notificationID
            )
        )
        let elapsedNanoseconds = DispatchTime.now().uptimeNanoseconds - startNanoseconds

        #expect(elapsedNanoseconds < Constants.quickActionSubmitBudgetNanoseconds)
        #expect(runtime.pendingCommands().count == 1)
        #expect(runtime.pendingCommands().first?.notificationID == notificationID)
    }

    @MainActor
    @Test
    func testNotificationReplyDrainReturnsFalseWhenNothingQueued() async throws {
        let runtime = try Self.temporarySessionRuntime()
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        let didDrain = await model.drainPendingNotificationReplies()

        #expect(!didDrain)
        #expect(runtime.pendingCommands().isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testStreamRestartLivenessDoesNotReplayLocalSnapshot() {
        let restart = CompanionSessionMiniController.restartLivenessUpdate()

        #expect(restart.reason == "runtime-restart")
        #expect(restart.latestSeq == 0)
        #expect(restart.serverTime.isEmpty)
        #expect(!restart.isLive)
        #expect(restart.endpointURL == nil)
    }

    @MainActor
    @Test
    func testRuntimeUnavailableLivenessDoesNotReportConnected() async throws {
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 15,
            records: [
                Self.miniRecord(session: cachedSession, seq: 15, revision: "mini-revision-15"),
            ]
        )
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )
        let route = try #require(URL(string: "http://192.168.2.10:8766"))
        #expect(model.applyRealtimeStreamLiveness(
            serverTime: Constants.heartbeatTimestamp,
            latestSeq: 15,
            isLive: true,
            endpointURL: route
        ))

        let unavailable = CompanionSessionMiniController.runtimeUnavailableLivenessUpdate()
        #expect(model.applyRealtimeStreamLiveness(
            serverTime: unavailable.serverTime,
            latestSeq: unavailable.latestSeq,
            isLive: unavailable.isLive,
            endpointURL: unavailable.endpointURL
        ))

        #expect(unavailable.reason == "runtime-unavailable")
        #expect(!model.realtimeStreamIsLive)
        #expect(model.connectionState == .connecting)
        #expect(model.activeConnectionRouteBaseURL == nil)
        #expect(model.realtimeLatestSeq == 15)
    }

    @Test
    func testStateMiniRecoveryEmptyResultIsNotApplied() {
        #expect(!StateMiniRecoveryResult.empty.didApplySnapshot)
        #expect(!StateMiniRecoveryResult.failed("transport").didApplySnapshot)
        #expect(!StateMiniRecoveryResult.skipped.didApplySnapshot)
        #expect(StateMiniRecoveryResult.applied.didApplySnapshot)
    }

    @Test
    func testPartialGitRepositoryMetadataCacheDecodes() throws {
        let sessionID = "thread-partial-git"
        let payload: [String: Any] = [
            "id": sessionID,
            "sessionId": sessionID,
            "assistantSurface": CompanionAssistantSurface.codex.rawValue,
            "ref": "S22",
            "title": "Partial Git",
            "status": SessionStatus.active.rawValue,
            "lastUpdatedAt": Constants.timestamp,
            "lastActivityAt": Constants.timestamp,
            "isArchived": false,
            "canSendPrompt": true,
            "metadata": [
                "projectName": "looper",
                "gitRepository": [
                    "repositoryName": "looper",
                    "branch": "main",
                ],
            ],
        ]
        let payloadData = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        let runtime = try Self.temporarySessionRuntime(
            latestSeq: 22,
            records: [
                SessionMiniFixture(
                    sessionID: sessionID,
                    assistantSurface: CompanionAssistantSurface.codex.rawValue,
                    seq: 22,
                    revision: "mini-revision-22",
                    payloadJSON: String(decoding: payloadData, as: UTF8.self)
                ),
            ]
        )

        let snapshot = try #require(try runtime.cachedSnapshot())
        let session = try #require(snapshot.session(withID: sessionID))

        #expect(session.metadata.gitRepository?.repositoryName == "looper")
        #expect(session.metadata.gitRepository?.repositoryPath == "")
        #expect(session.metadata.gitRepository?.branch == "main")
    }

    private static func temporarySessionRuntime() throws -> CompanionSessionRuntime {
        try CompanionSessionRuntime(fileURL: temporaryStoreFileURL())
    }

    private static func pendingCommand(
        in runtime: CompanionSessionRuntime,
        kind: ClientPendingCommandKind
    ) throws -> CompanionSessionMiniPendingCommand {
        try #require(pendingCommands(in: runtime, kind: kind).first)
    }

    private static func pendingCommands(
        in runtime: CompanionSessionRuntime,
        kind: ClientPendingCommandKind
    ) -> [CompanionSessionMiniPendingCommand] {
        runtime.pendingCommands().filter { $0.kind == kind }
    }

    private static func localMiniSourceIDs(in runtime: CompanionSessionRuntime) throws -> [String] {
        try runtime.currentStateMiniSnapshot().sessions.map(\.sessionId).sorted()
    }

    private static func temporarySessionRuntime(
        latestSeq: Int64,
        records: [SessionMiniFixture],
        latestReplies: [SessionLatestReplyFixture] = []
    ) throws -> CompanionSessionRuntime {
        let fileURL = try temporaryStoreFileURL()
        try seedMiniCache(
            at: fileURL,
            latestSeq: latestSeq,
            records: records,
            latestReplies: latestReplies
        )
        return try CompanionSessionRuntime(fileURL: fileURL)
    }

    private static func temporaryStoreFileURL() throws -> URL {
        let directoryURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("looper-session-mini-local-first", isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return directoryURL.appendingPathComponent(CompanionSessionRuntime.defaultFileName)
    }

    private static func seedMalformedMiniCache(at fileURL: URL) throws {
        let payload: [String: Any] = [
            "latestSeq": 3,
            "sessions": [
                [
                    "sessionId": "bad-cache",
                    "assistantSurface": CompanionAssistantSurface.codex.rawValue,
                    "seq": 3,
                    "revision": "bad-mini",
                    "payloadJson": "{not-json",
                ],
            ],
            "pendingCommands": [],
            "serverTime": Constants.timestamp,
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        try data.write(to: fileURL, options: .atomic)
    }

    private static func restoreUserDefaultsValue(_ value: Any?, key: String) {
        if let value {
            UserDefaults.standard.set(value, forKey: key)
        } else {
            UserDefaults.standard.removeObject(forKey: key)
        }
    }

    private static func seedMiniCache(
        at fileURL: URL,
        latestSeq: Int64,
        records: [SessionMiniFixture],
        latestReplies: [SessionLatestReplyFixture] = []
    ) throws {
        let payload: [String: Any] = [
            "latestSeq": latestSeq,
            "sessions": records.map { record in
                [
                    "sessionId": record.sessionID,
                    "assistantSurface": record.assistantSurface,
                    "seq": record.seq,
                    "revision": record.revision,
                    "payloadJson": record.payloadJSON,
                ]
            },
            "latestReplies": latestReplies.map { reply in
                [
                    "session_id": reply.sessionID,
                    "message_id": reply.messageID,
                    "text": reply.text,
                    "latest_seq": reply.latestSeq,
                    "is_final": reply.isFinal,
                    "is_truncated": reply.isTruncated,
                    "server_time": reply.serverTime,
                ] as [String: Any]
            },
            "pendingCommands": [],
            "serverTime": Constants.timestamp,
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        try data.write(to: fileURL, options: .atomic)
    }

    private static func miniRecord(
        session: SessionSummary,
        assistantSurface: CompanionAssistantSurface = .codex,
        seq: Int64,
        revision: String
    ) throws -> SessionMiniFixture {
        let data = try JSONEncoder().encode(session)
        return SessionMiniFixture(
            sessionID: session.id,
            assistantSurface: assistantSurface.rawValue,
            seq: seq,
            revision: revision,
            payloadJSON: String(decoding: data, as: UTF8.self)
        )
    }

    private static func latestReply(
        sessionID: String,
        messageID: String,
        text: String,
        latestSeq: Int64,
        isFinal: Bool = true,
        isTruncated: Bool = false,
        serverTime: String
    ) -> SessionLatestReplyFixture {
        SessionLatestReplyFixture(
            sessionID: sessionID,
            messageID: messageID,
            text: text,
            latestSeq: latestSeq,
            isFinal: isFinal,
            isTruncated: isTruncated,
            serverTime: serverTime
        )
    }

    private static func networkSnapshot() -> MobileSnapshot {
        let session = sessionSummary(
            id: Constants.fallbackThreadID,
            title: "Network Fallback",
            ref: "N1",
            status: .active
        )
        return mobileSnapshot(
            revision: "network-revision",
            sessions: [session]
        )
    }

    private static func mobileSnapshot(
        revision: String,
        sessions: [SessionSummary],
        surfaceSessions: [String: [SessionSummary]]? = nil
    ) -> MobileSnapshot {
        return MobileSnapshot(
            revision: revision,
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: Constants.timestamp
            ),
            globalSettings: GlobalSettings(
                defaultPrompt: "Continue",
                globalMode: nil,
                scope: "global",
                notificationLabel: nil,
                completionCheckLabel: nil,
                completionCheckWaitForReply: false,
                assistantSurface: .codex
            ),
            sessions: sessions,
            surfaceSessions: surfaceSessions ?? [CompanionAssistantSurface.codex.rawValue: sessions],
            notifications: [],
            completionChecks: []
        )
    }

    @MainActor
    private static func waitForSelectedAssistantSurface(
        _ surface: CompanionAssistantSurface,
        model: CompanionAppModel
    ) async throws {
        for _ in 0..<Constants.assistantSurfaceAckPollAttempts {
            if model.viewState.selectedAssistantSurface == surface {
                return
            }
            try await Task.sleep(nanoseconds: Constants.assistantSurfaceAckPollNanoseconds)
        }
    }

    private static func sessionSummary(
        id: String,
        title: String,
        ref: String,
        status: SessionStatus,
        effectiveMode: SessionMode? = nil
    ) -> SessionSummary {
        SessionSummary(
            id: id,
            ref: ref,
            title: title,
            status: status,
            effectiveMode: effectiveMode,
            lastUpdatedAt: Constants.timestamp,
            lastActivityAt: Constants.timestamp,
            assistantPreview: "Ready",
            isArchived: false,
            canSendPrompt: true
        )
    }
}

private struct SessionMiniFixture: Equatable, Sendable {
    let sessionID: String
    let assistantSurface: String
    let seq: Int64
    let revision: String
    let payloadJSON: String
}

private struct SessionLatestReplyFixture: Equatable, Sendable {
    let sessionID: String
    let messageID: String
    let text: String
    let latestSeq: Int64
    let isFinal: Bool
    let isTruncated: Bool
    let serverTime: String
}

private final class SessionMiniLocalFirstServiceSpy: CompanionService, @unchecked Sendable {
    enum ServiceError: Error {
        case promptFailed
    }

    static func notificationReplyMutationID(notificationID: String) -> String {
        "notification-reply:\(notificationID.trimmingCharacters(in: .whitespacesAndNewlines))"
    }

    private let lock = NSLock()
    private let snapshot: MobileSnapshot
    var loadSnapshotError: Error?
    private(set) var loadSnapshotCallCount = 0
    private(set) var loadServerHealthCallCount = 0

    init(snapshot: MobileSnapshot) {
        self.snapshot = snapshot
    }

    func loadServerHealth() async throws -> CompanionServerHealth {
        incrementLoadServerHealthCallCount()
        return CompanionServerHealth(
            ok: true,
            baseURL: "",
            baseURLs: [],
            serverTime: "2026-06-24T00:00:00Z"
        )
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        incrementLoadSnapshotCallCount()
        if let loadSnapshotError {
            throw loadSnapshotError
        }

        return snapshot
    }

    func loadSessionDetail(id: String, surface _: CompanionAssistantSurface?) async throws -> SessionDetail {
        throw ServiceError.promptFailed
    }

    func setSessionArchived(id _: String, archived _: Bool) async throws -> MobileSnapshot {
        snapshot
    }

    func deleteSession(id _: String) async throws -> MobileSnapshot {
        snapshot
    }

    private func incrementLoadSnapshotCallCount() {
        lock.lock()
        defer { lock.unlock() }
        loadSnapshotCallCount += 1
    }

    private func incrementLoadServerHealthCallCount() {
        lock.lock()
        defer { lock.unlock() }
        loadServerHealthCallCount += 1
    }

    func muteSession(id _: String) async throws -> MobileSnapshot {
        snapshot
    }

    func saveDefaultPrompt(_: String) async throws -> MobileSnapshot {
        snapshot
    }

    func saveSiriDefaultSession(
        id _: String?,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot {
        snapshot
    }

    func registerPushDevice(
        _: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        throw ServiceError.promptFailed
    }

    func sendTestPush(installationID _: String) async throws -> RemotePushTestResponse {
        throw ServiceError.promptFailed
    }
}

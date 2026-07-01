import Foundation
import LooperClientCore
import Testing
@testable import Looper

@Suite("Session summary timing")
struct SessionSummaryTimingTests {
    init() {
        CompanionSnapshotStateStore.clearPersistedAssistantSurfaceForTesting()
    }

    private enum Constants {
        static let millisecondsPerSecond: TimeInterval = 1_000
        static let dateToleranceSeconds: TimeInterval = 0.000_001
        static let activityMilliseconds: Int64 = 1_781_596_920_321
        static let olderActivityMilliseconds: Int64 = 1_781_596_920_123
        static let messageMilliseconds: Int64 = 1_781_596_860_123
        static let largeSurfaceSessionCount = 1_500
        static let hostSyncTime = "2026-06-16T08:02:00Z"
        static let refreshedHostSyncTime = "2026-06-16T08:03:00Z"
        static let siriRuntimeStoreDirectoryName = "looper-siri-session-summary-timing"
    }

    private let decoder = JSONDecoder()

    @Test("Decoded millisecond fields drive displayed dates")
    func decodedMillisecondFieldsDriveDisplayedDates() throws {
        let session = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )

        #expect(session.lastActivityAtMs == Constants.activityMilliseconds)
        #expect(session.lastMessageAtMs == Constants.messageMilliseconds)
        #expect(
            isDate(
                session.lastActivityDate,
                equalToMilliseconds: Constants.activityMilliseconds
            )
        )
        #expect(
            isDate(
                session.lastMessageDate,
                equalToMilliseconds: Constants.messageMilliseconds
            )
        )
    }

    @Test("Freshness sorting uses raw millisecond activity")
    func freshnessSortingUsesRawMillisecondActivity() throws {
        let newer = try sessionSummary(
            id: "newer-thread",
            ref: "S2",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let older = try sessionSummary(
            id: "older-thread",
            ref: "S1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )

        #expect([older, newer].sortedBySessionFreshness().map(\.id) == ["newer-thread", "older-thread"])
    }

    @Test("Cross-surface lookup uses raw millisecond activity")
    func crossSurfaceLookupUsesRawMillisecondActivity() throws {
        let olderCodex = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let newerDevin = try sessionSummary(
            id: "thread-main",
            ref: "S2",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let snapshot = MobileSnapshot(
            revision: "revision-1",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: "2026-06-16T08:02:00Z"
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
            sessions: [olderCodex],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [olderCodex],
                CompanionAssistantSurface.devin.rawValue: [newerDevin],
            ],
            notifications: [],
            completionChecks: []
        )
        let index = SessionIndex(snapshot: snapshot)

        #expect(snapshot.sessionsAcrossSurfaces.map(\.ref) == ["S2"])
        #expect(snapshot.session(withID: "thread-main")?.ref == "S2")
        #expect(snapshot.assistantSurface(containingSessionID: "thread-main") == .devin)
        #expect(index.allSessions.map(\.ref) == ["S2"])
        #expect(index.session(withID: "thread-main")?.ref == "S2")
        #expect(index.assistantSurface(containingSessionID: "thread-main") == .devin)
        #expect(
            index.identity
                == "revision-1|1|thread-main:active:2026-06-16T08:02:00Z:2026-06-16T08:01:00Z::::goal-idle:0:visible"
        )
    }

    @MainActor
    @Test("Stable detail route ignores stale row summary preview")
    func stableDetailRouteIgnoresStaleRowSummaryPreview() throws {
        var staleCodexRow = try sessionSummary(
            id: "thread-main",
            ref: "C1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        staleCodexRow.assistantPreview = "stale row preview"
        var liveDevinSession = try sessionSummary(
            id: "thread-main",
            ref: "D1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        liveDevinSession.assistantPreview = "live detail preview"
        let snapshot = MobileSnapshot(
            revision: "revision-detail-route",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: "2026-06-16T08:02:00Z"
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
            sessions: [staleCodexRow],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [staleCodexRow],
                CompanionAssistantSurface.devin.rawValue: [liveDevinSession],
            ],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()
        store.applySnapshot(snapshot)

        let route = SessionDetailRoute(
            sessionID: "thread-main",
            assistantSurface: .devin
        )
        let routedDetail = try #require(
            store.detail(
                for: route.sessionID,
                assistantSurface: route.assistantSurface
            )
        )

        #expect(routedDetail.ref == "D1")
        #expect(routedDetail.assistantPreview == "live detail preview")
        #expect(routedDetail.latestAssistantMessage == "live detail preview")
        #expect(store.session(withID: route.sessionID, assistantSurface: route.assistantSurface)?.ref == "D1")
        #expect(store.detail(for: "thread-main")?.ref == "C1")
    }

    @Test("Latest reply uses local mini until newer text chunk arrives")
    func latestReplyUsesLocalMiniUntilNewerTextChunkArrives() throws {
        var summary = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        summary.assistantPreview = "latest local preview"
        let snapshot = MobileSnapshot(
            revision: "revision-latest-reply",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: "2026-06-16T08:02:00Z"
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
            sessions: [summary],
            surfaceSessions: [CompanionAssistantSurface.codex.rawValue: [summary]],
            notifications: [],
            completionChecks: []
        )
        var detail = SessionDetail(summary: summary, snapshot: snapshot)

        #expect(detail.latestAssistantMessage == "latest local preview")

        detail.applyLatestReplyProjection(
            projection(
                sessionID: "thread-main",
                text: "stale streamed reply",
                serverTime: "2026-06-16T08:00:00Z"
            )
        )
        #expect(detail.latestAssistantMessage == "latest local preview")

        detail.applyLatestReplyProjection(
            projection(
                sessionID: "thread-main",
                text: "new streamed reply",
                serverTime: "2026-06-16T08:01:01Z"
            )
        )
        #expect(detail.latestAssistantMessage == "new streamed reply")
    }

    @Test("Siri session entities use Rust projection ordering")
    func siriSessionEntitiesUseRustProjectionOrdering() async throws {
        let olderCodex = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let newerDevin = try sessionSummary(
            id: "thread-main",
            ref: "S2",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let middleGrok = try sessionSummary(
            id: "grok-thread",
            ref: "G1",
            activityMilliseconds: Constants.olderActivityMilliseconds + 1,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let archivedZed = try sessionSummary(
            id: "archived-thread",
            ref: "Z1",
            status: "archived",
            activityMilliseconds: Constants.activityMilliseconds + 1,
            messageMilliseconds: Constants.messageMilliseconds,
            isArchived: true
        )
        let codexSeq: Int64 = 8
        let grokSeq: Int64 = 10
        let zedSeq: Int64 = 12
        let devinSeq: Int64 = 14
        let localLatestSeq = devinSeq
        let runtimeStore = try temporarySessionRuntime(
            latestSeq: localLatestSeq,
            records: [
                try miniRecord(
                    session: olderCodex,
                    assistantSurface: .codex,
                    seq: codexSeq,
                    revision: "codex-revision-\(codexSeq)"
                ),
                try miniRecord(
                    session: newerDevin,
                    assistantSurface: .devin,
                    seq: devinSeq,
                    revision: "devin-revision-\(devinSeq)"
                ),
                try miniRecord(
                    session: middleGrok,
                    assistantSurface: .grokBuild,
                    seq: grokSeq,
                    revision: "grok-revision-\(grokSeq)"
                ),
                try miniRecord(
                    session: archivedZed,
                    assistantSurface: .zed,
                    seq: zedSeq,
                    revision: "zed-revision-\(zedSeq)"
                ),
            ]
        )
        defer { runtimeStore.cleanup() }
        let client = LooperSiriSessionClient(
            service: SnapshotOnlyCompanionService(snapshot: unusedServiceSnapshot()),
            sessionRuntime: runtimeStore.runtime
        )
        let entities = try await client.suggestedEntities()

        #expect(entities.map(\.ref) == ["S2", "G1", "S1"])
        #expect(entities.map(\.assistantSurfaceRawValue) == ["devin", "grok-build", "codex"])
        #expect(!entities.map(\.sessionID).contains("archived-thread"))
    }

    @Test("Siri default and current sessions use Rust projection")
    func siriDefaultAndCurrentSessionsUseRustProjection() async throws {
        let defaultCodex = try sessionSummary(
            id: "default-thread",
            ref: "C1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let currentDevin = try sessionSummary(
            id: "current-thread",
            ref: "D1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let defaultCodexSeq: Int64 = 8
        let currentDevinSeq: Int64 = 12
        let localLatestSeq = currentDevinSeq
        let runtimeStore = try temporarySessionRuntime(
            latestSeq: localLatestSeq,
            records: [
                try miniRecord(
                    session: defaultCodex,
                    assistantSurface: .codex,
                    seq: defaultCodexSeq,
                    revision: "codex-revision-\(defaultCodexSeq)"
                ),
                try miniRecord(
                    session: currentDevin,
                    assistantSurface: .devin,
                    seq: currentDevinSeq,
                    revision: "devin-revision-\(currentDevinSeq)"
                ),
            ]
        )
        defer { runtimeStore.cleanup() }
        try await runtimeStore.runtime.setSiriDefaultSession(
            threadID: "default-thread",
            assistantSurface: .codex
        )
        try await runtimeStore.runtime.setSiriCurrentSession(
            threadID: "current-thread",
            assistantSurface: nil
        )
        let client = LooperSiriSessionClient(
            service: SnapshotOnlyCompanionService(snapshot: unusedServiceSnapshot()),
            sessionRuntime: runtimeStore.runtime
        )

        let defaultEntity = try await client.defaultSiriSessionEntity()
        let currentEntity = try await client.currentSiriSessionEntity()

        #expect(defaultEntity.sessionID == "default-thread")
        #expect(defaultEntity.assistantSurfaceRawValue == "codex")
        #expect(currentEntity.sessionID == "current-thread")
        #expect(currentEntity.assistantSurfaceRawValue == "devin")
    }

    @Test("Siri current session falls back to Rust projected default")
    func siriCurrentSessionFallsBackToRustProjectedDefault() async throws {
        let defaultCodex = try sessionSummary(
            id: "default-thread",
            ref: "C1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let defaultCodexSeq: Int64 = 8
        let localLatestSeq = defaultCodexSeq
        let runtimeStore = try temporarySessionRuntime(
            latestSeq: localLatestSeq,
            records: [
                try miniRecord(
                    session: defaultCodex,
                    assistantSurface: .codex,
                    seq: defaultCodexSeq,
                    revision: "codex-revision-\(defaultCodexSeq)"
                ),
            ]
        )
        defer { runtimeStore.cleanup() }
        try await runtimeStore.runtime.setSiriDefaultSession(
            threadID: "default-thread",
            assistantSurface: .codex
        )
        try await runtimeStore.runtime.setSiriCurrentSession(
            threadID: "stale-thread",
            assistantSurface: .devin
        )
        let client = LooperSiriSessionClient(
            service: SnapshotOnlyCompanionService(snapshot: unusedServiceSnapshot()),
            sessionRuntime: runtimeStore.runtime
        )

        let currentEntity = try await client.currentSiriSessionEntity()

        #expect(currentEntity.sessionID == "default-thread")
        #expect(currentEntity.assistantSurfaceRawValue == "codex")
    }

    @Test("Mobile snapshot decodes Codex work status")
    func mobileSnapshotDecodesCodexWorkStatus() throws {
        let sessionPayload = sessionPayload(
            id: "thread-main",
            ref: "S1",
            status: "stopped",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds,
            goal: [
                "id": "goal-main",
                "title": "Ship Looper",
                "status": "pursuing",
                "lifecycle": "pursuing",
                "running": true,
                "updatedAtMs": Constants.activityMilliseconds,
            ]
        )
        let payload: [String: Any] = [
            "revision": "revision-1",
            "host": [
                "id": "host",
                "name": "Looper",
                "address": "http://127.0.0.1:8765",
                "isReachable": true,
                "lastSyncedAt": "2026-06-16T08:02:00Z",
            ],
            "globalSettings": [
                "assistantSurface": "codex",
            ],
            "sessions": [sessionPayload],
            "surfaceSessions": [
                "codex": [sessionPayload],
            ],
            "notifications": [],
            "completionChecks": [],
            "workStatus": [
                "goalCount": 1,
                "runningGoalCount": 1,
                "automationCount": 1,
                "activeAutomationCount": 1,
                "coveredAutomationCount": 1,
                "runningGoals": [
                    [
                        "id": "goal-main",
                        "title": "Ship Looper",
                        "status": "pursuing",
                        "targetThreadId": "thread-main",
                        "targetKnown": true,
                        "updatedAtMs": Constants.activityMilliseconds,
                    ],
                ],
                "activeAutomations": [
                    [
                        "id": "daily-review",
                        "kind": "heartbeat",
                        "name": "Daily Review",
                        "status": "ACTIVE",
                        "scheduleSummary": "HOURLY every 1",
                        "targetThreadId": "thread-main",
                        "targetKnown": true,
                        "controlPlaneCovered": true,
                    ],
                ],
            ],
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        let snapshot = try decoder.decode(MobileSnapshot.self, from: data)
        let sections = SessionSections(sessions: snapshot.sessions)

        #expect(snapshot.workStatus.runningGoalCount == 1)
        #expect(snapshot.workStatus.activeAutomationCount == 1)
        #expect(snapshot.workStatus.displaySummary == "1 running goal · 1 active automation")
        #expect(snapshot.workStatus.coverageSummary == "1/1 automations covered")
        #expect(snapshot.sessions[0].workStatusLabel == "Goal running")
        #expect(sections.running.map(\.id) == ["thread-main"])
        #expect(sections.stopped.isEmpty)
    }

    @Test("Server health decodes old H2-only realtime metadata")
    func serverHealthDecodesOldH2OnlyRealtimeMetadata() throws {
        let payload: [String: Any] = [
            "ok": true,
            "baseURL": "http://127.0.0.1:8765",
            "baseURLs": ["http://127.0.0.1:8765"],
            "grpcBaseURL": "http://127.0.0.1:8766",
            "grpcBaseURLs": ["http://127.0.0.1:8766"],
            "serverTime": "2026-06-16T08:02:00Z",
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        let health = try decoder.decode(CompanionServerHealth.self, from: data)

        #expect(health.grpcBaseURL == "http://127.0.0.1:8766")
        #expect(health.grpcBaseURLs == ["http://127.0.0.1:8766"])
        #expect(health.grpcH3BaseURL == "")
        #expect(health.grpcH3BaseURLs.isEmpty)
        #expect(health.grpcH3CertificateSha256 == nil)
    }

    @Test("Server health decodes H3 realtime metadata and malformed optional pin")
    func serverHealthDecodesH3RealtimeMetadataAndMalformedOptionalPin() throws {
        let payload: [String: Any?] = [
            "ok": true,
            "baseURL": "http://192.168.1.4:8765",
            "baseURLs": ["http://192.168.1.4:8765"],
            "grpcBaseURL": "http://192.168.1.4:8766",
            "grpcBaseURLs": ["http://192.168.1.4:8766"],
            "grpcH3BaseURL": "https://192.168.1.4:8766",
            "grpcH3BaseURLs": ["https://192.168.1.4:8766"],
            "grpcH3CertificateSha256": NSNull(),
            "serverTime": "2026-06-16T08:02:00Z",
            "tailscale": [
                "available": true,
                "running": true,
                "ipAddresses": ["100.95.2.4"],
                "baseURL": "http://100.95.2.4:8765",
                "grpcBaseURL": "http://100.95.2.4:8766",
                "grpcH3BaseURL": "https://100.95.2.4:8766",
                "health": [],
            ],
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        let health = try decoder.decode(CompanionServerHealth.self, from: data)

        #expect(health.grpcBaseURL == "http://192.168.1.4:8766")
        #expect(health.grpcBaseURLs == ["http://192.168.1.4:8766"])
        #expect(health.grpcH3BaseURL == "https://192.168.1.4:8766")
        #expect(health.grpcH3BaseURLs == ["https://192.168.1.4:8766"])
        #expect(health.grpcH3CertificateSha256 == nil)
        #expect(health.tailscale?.grpcH3BaseURL == "https://100.95.2.4:8766")
    }

    @Test("Server health ignores null optional H3 metadata and preserves H2")
    func serverHealthIgnoresNullOptionalH3MetadataAndPreservesH2() throws {
        let payload: [String: Any?] = [
            "ok": true,
            "baseURL": "http://192.168.1.4:8765",
            "baseURLs": ["http://192.168.1.4:8765"],
            "grpcBaseURL": "http://192.168.1.4:8766",
            "grpcBaseURLs": ["http://192.168.1.4:8766"],
            "grpcH3BaseURL": NSNull(),
            "grpcH3BaseURLs": NSNull(),
            "grpcH3CertificateSha256": NSNull(),
            "serverTime": "2026-06-16T08:02:00Z",
            "tailscale": [
                "available": true,
                "running": true,
                "ipAddresses": ["100.95.2.4"],
                "baseURL": "http://100.95.2.4:8765",
                "grpcBaseURL": "http://100.95.2.4:8766",
                "grpcH3BaseURL": NSNull(),
                "health": [],
            ],
        ]
        let data = try JSONSerialization.data(withJSONObject: payload)
        let health = try decoder.decode(CompanionServerHealth.self, from: data)

        #expect(health.ok)
        #expect(health.baseURL == "http://192.168.1.4:8765")
        #expect(health.baseURLs == ["http://192.168.1.4:8765"])
        #expect(health.grpcBaseURL == "http://192.168.1.4:8766")
        #expect(health.grpcBaseURLs == ["http://192.168.1.4:8766"])
        #expect(health.grpcH3BaseURL == "")
        #expect(health.grpcH3BaseURLs.isEmpty)
        #expect(health.grpcH3CertificateSha256 == nil)
        #expect(health.tailscale?.grpcBaseURL == "http://100.95.2.4:8766")
        #expect(health.tailscale?.grpcH3BaseURL == nil)
    }

    @Test("Server health ignores wrong-type optional H3 metadata and preserves H2")
    func serverHealthIgnoresWrongTypeOptionalH3MetadataAndPreservesH2() throws {
        let payload = """
        {
            "ok": true,
            "baseURL": "http://192.168.1.4:8765",
            "baseURLs": ["http://192.168.1.4:8765"],
            "grpcBaseURL": "http://192.168.1.4:8766",
            "grpcBaseURLs": ["http://192.168.1.4:8766"],
            "grpcH3BaseURL": 8766,
            "grpcH3BaseURLs": "https://192.168.1.4:8766",
            "grpcH3CertificateSha256": 12345,
            "serverTime": "2026-06-16T08:02:00Z",
            "tailscale": {
                "available": true,
                "running": true,
                "ipAddresses": ["100.95.2.4"],
                "baseURL": "http://100.95.2.4:8765",
                "grpcBaseURL": "http://100.95.2.4:8766",
                "grpcH3BaseURL": 8766,
                "health": []
            }
        }
        """
        let data = try #require(payload.data(using: .utf8))
        let health = try decoder.decode(CompanionServerHealth.self, from: data)

        #expect(health.ok)
        #expect(health.baseURL == "http://192.168.1.4:8765")
        #expect(health.baseURLs == ["http://192.168.1.4:8765"])
        #expect(health.grpcBaseURL == "http://192.168.1.4:8766")
        #expect(health.grpcBaseURLs == ["http://192.168.1.4:8766"])
        #expect(health.grpcH3BaseURL == "")
        #expect(health.grpcH3BaseURLs.isEmpty)
        #expect(health.grpcH3CertificateSha256 == nil)
        #expect(health.tailscale?.grpcBaseURL == "http://100.95.2.4:8766")
        #expect(health.tailscale?.grpcH3BaseURL == nil)
    }

    @Test("Blocked goal is visible as a needs-attention card status")
    func blockedGoalIsVisibleAsNeedsAttentionCardStatus() throws {
        let session = try sessionSummary(
            id: "blocked-thread",
            ref: "S3",
            status: "stopped",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds,
            goal: [
                "id": "goal-blocked",
                "title": "Unblock delivery",
                "status": "blocked",
                "lifecycle": "blocked",
                "running": false,
                "updatedAtMs": Constants.activityMilliseconds,
            ]
        )
        let sections = SessionSections(sessions: [session])

        #expect(session.hasBlockedGoal)
        #expect(session.workStatusLabel == "Goal blocked")
        #expect(session.workStatusSymbolName == "exclamationmark.octagon.fill")
        #expect(sections.needsAttention.map { $0.id } == ["blocked-thread"])
        #expect(sections.stopped.isEmpty)
    }

    @MainActor
    @Test("Snapshot state store owns visible surface projection")
    func snapshotStateStoreOwnsVisibleSurfaceProjection() throws {
        var codexSession = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        codexSession.effectiveMode = .maxTurns1
        codexSession.assistantPreview = "Codex ready"
        var devinSession = try sessionSummary(
            id: "thread-main",
            ref: "S2",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        devinSession.effectiveMode = .awaitReply
        devinSession.assistantPreview = "Devin ready"
        let snapshot = MobileSnapshot(
            revision: "revision-1",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: "2026-06-16T08:02:00Z"
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
            sessions: [codexSession],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [codexSession],
                CompanionAssistantSurface.devin.rawValue: [devinSession],
            ],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()

        #expect(store.applySnapshotResult(snapshot).didChangeVisibleSnapshot)
        #expect(!store.applySnapshotResult(snapshot).didChangeVisibleSnapshot)
        #expect(store.selectedAssistantSurface == .codex)
        #expect(store.sessionSections.active.map(\.ref) == ["S1"])
        #expect(store.detail(for: "thread-main")?.effectiveMode == .maxTurns1)
        #expect(store.detail(for: "thread-main")?.assistantPreview == "Codex ready")
        #expect(store.detail(for: "thread-main")?.latestAssistantMessage == "Codex ready")
        let visibleHostSyncTime = store.snapshot?.host.lastSyncedAt
        #expect(store.applyHostSyncTime("2026-06-16T08:03:00Z"))
        #expect(store.snapshot?.host.lastSyncedAt == visibleHostSyncTime)
        #expect(!store.selectAssistantSurface(.codex))

        #expect(store.selectAssistantSurface(.devin))
        #expect(store.selectedAssistantSurface == .devin)
        #expect(store.sessionSections.active.map(\.ref) == ["S2"])
        #expect(store.session(withID: "thread-main")?.ref == "S2")
        #expect(store.detail(for: "thread-main")?.effectiveMode == .awaitReply)
        #expect(store.detail(for: "thread-main")?.assistantPreview == "Devin ready")
        #expect(store.detail(for: "thread-main")?.latestAssistantMessage == "Devin ready")

        #expect(store.session(withID: "thread-main")?.effectiveMode == .awaitReply)
        #expect(store.detail(for: "thread-main")?.effectiveMode == .awaitReply)

        var refreshedDevinSession = devinSession
        refreshedDevinSession.assistantPreview = "Devin refreshed"
        var refreshedSnapshot = snapshot
        refreshedSnapshot.revision = "revision-2"
        refreshedSnapshot.globalSettings.assistantSurface = .codex
        refreshedSnapshot.surfaceSessions[CompanionAssistantSurface.devin.rawValue] = [refreshedDevinSession]

        store.applySnapshot(refreshedSnapshot)
        #expect(store.selectedAssistantSurface == .devin)
        #expect(store.sessionSections.active.map(\.ref) == ["S2"])
        #expect(store.detail(for: "thread-main")?.assistantPreview == "Devin refreshed")
        #expect(store.refreshDetail(for: "thread-main"))
        #expect(store.detail(for: "thread-main")?.latestAssistantMessage == "Devin refreshed")

        #expect(store.selectAssistantSurface(.codex))
        #expect(store.selectedAssistantSurface == .codex)
        #expect(store.sessionSections.active.map(\.ref) == ["S1"])
    }

    @MainActor
    @Test("Assistant surface switch keeps local freshness order immediately")
    func assistantSurfaceSwitchKeepsLocalFreshnessOrderImmediately() throws {
        let codexSession = try sessionSummary(
            id: "codex-thread",
            ref: "C1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let olderDevinSession = try sessionSummary(
            id: "older-devin-thread",
            ref: "D1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let newerDevinSession = try sessionSummary(
            id: "newer-devin-thread",
            ref: "D2",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let snapshot = MobileSnapshot(
            revision: "surface-switch-order",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: "2026-06-16T08:02:00Z"
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
            sessions: [codexSession],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [codexSession],
                CompanionAssistantSurface.devin.rawValue: [olderDevinSession, newerDevinSession],
            ],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()

        store.applySnapshot(snapshot)
        #expect(store.sessionSections.active.map(\.ref) == ["C1"])

        #expect(store.selectAssistantSurface(.devin))
        #expect(store.selectedAssistantSurface == .devin)
        #expect(store.sessionSections.active.map(\.ref) == ["D2", "D1"])
        #expect(store.sessionSections.running.map(\.ref) == ["D2", "D1"])
    }

    @MainActor
    @Test("Assistant surface switch uses cached local projections for large snapshots")
    func assistantSurfaceSwitchUsesCachedLocalProjectionsForLargeSnapshots() throws {
        let codexSessions = try (0..<Constants.largeSurfaceSessionCount).map { index in
            try sessionSummary(
                id: "codex-thread-\(index)",
                ref: "C\(index)",
                activityMilliseconds: Constants.activityMilliseconds - Int64(index),
                messageMilliseconds: Constants.messageMilliseconds
            )
        }
        let zedSession = try sessionSummary(
            id: "zed-thread",
            ref: "Z1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let claudeSession = try sessionSummary(
            id: "claude-thread",
            ref: "CL1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let grokSession = try sessionSummary(
            id: "grok-thread",
            ref: "G1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let snapshot = MobileSnapshot(
            revision: "large-surface-switch",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: Constants.hostSyncTime
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
            sessions: codexSessions,
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: codexSessions,
                CompanionAssistantSurface.zed.rawValue: [zedSession],
                CompanionAssistantSurface.claudeCode.rawValue: [claudeSession],
                CompanionAssistantSurface.grokBuild.rawValue: [grokSession],
            ],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()

        store.applySnapshot(snapshot)
        let sessionIndexIdentity = store.sessionIndexIdentity
        #expect(store.sessionSections.active.count == Constants.largeSurfaceSessionCount)
        #expect(store.applyHostSyncTime(Constants.refreshedHostSyncTime))
        #expect(store.snapshot?.host.lastSyncedAt == Constants.hostSyncTime)

        #expect(store.selectAssistantSurface(.zed))
        #expect(store.selectedAssistantSurface == .zed)
        #expect(store.sessionSections.active.map(\.id) == ["zed-thread"])
        #expect(store.detail(for: "zed-thread")?.ref == "Z1")
        #expect(store.sessionIndexIdentity == sessionIndexIdentity)
        #expect(store.snapshot?.host.lastSyncedAt == Constants.refreshedHostSyncTime)

        #expect(store.selectAssistantSurface(.claudeCode))
        #expect(store.selectedAssistantSurface == .claudeCode)
        #expect(store.sessionSections.active.map(\.id) == ["claude-thread"])
        #expect(store.sessionIndexIdentity == sessionIndexIdentity)

        #expect(store.selectAssistantSurface(.grokBuild))
        #expect(store.selectedAssistantSurface == .grokBuild)
        #expect(store.sessionSections.active.map(\.id) == ["grok-thread"])
        #expect(store.sessionIndexIdentity == sessionIndexIdentity)

        #expect(store.selectAssistantSurface(.codex))
        #expect(store.selectedAssistantSurface == .codex)
        #expect(store.sessionSections.active.count == Constants.largeSurfaceSessionCount)
        #expect(
            store.detail(for: "codex-thread-\(Constants.largeSurfaceSessionCount - 1)")?.ref
                == "C\(Constants.largeSurfaceSessionCount - 1)"
        )
        #expect(store.sessionIndexIdentity == sessionIndexIdentity)
    }

    @MainActor
    @Test("Surface fallback filters global sessions by assistant client")
    func surfaceFallbackFiltersGlobalSessionsByAssistantClient() throws {
        let codexSession = try sessionSummary(
            id: "codex-thread",
            ref: "C1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds,
            assistantClient: "codex"
        )
        let zedSession = try sessionSummary(
            id: "zed-thread",
            ref: "Z1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds,
            assistantClient: "zed"
        )
        let claudeSession = try sessionSummary(
            id: "claude-thread",
            ref: "CL1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds,
            assistantClient: "claude-code"
        )
        let snapshot = MobileSnapshot(
            revision: "global-surface-fallback",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: Constants.hostSyncTime
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
            sessions: [codexSession, zedSession, claudeSession],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()

        store.applySnapshot(snapshot)
        #expect(store.selectedAssistantSurface == .codex)
        #expect(store.sessionSections.active.map(\.id) == ["codex-thread"])
        #expect(store.allSessions.map(\.id) == [
            "codex-thread",
            "claude-thread",
            "zed-thread",
        ])

        #expect(store.selectAssistantSurface(.zed))
        #expect(store.selectedAssistantSurface == .zed)
        #expect(store.sessionSections.active.map(\.id) == ["zed-thread"])
    }

    @MainActor
    @Test("Cached assistant surface switch makes matching snapshot echo a no-op")
    func cachedAssistantSurfaceSwitchMakesMatchingSnapshotEchoNoOp() throws {
        let codexSession = try sessionSummary(
            id: "codex-thread",
            ref: "C1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let zedSession = try sessionSummary(
            id: "zed-thread",
            ref: "Z1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let snapshot = MobileSnapshot(
            revision: "cached-surface-echo",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: Constants.hostSyncTime
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
            sessions: [codexSession],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [codexSession],
                CompanionAssistantSurface.zed.rawValue: [zedSession],
            ],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()

        #expect(store.applySnapshotResult(snapshot).didChangeVisibleSnapshot)
        #expect(store.selectAssistantSurface(.zed))
        #expect(store.selectedAssistantSurface == .zed)
        #expect(store.sessionSections.active.map(\.id) == ["zed-thread"])

        let echo = store.applySnapshotResult(snapshot)

        #expect(!echo.didChangeVisibleSnapshot)
        #expect(store.selectedAssistantSurface == .zed)
        #expect(store.sessionSections.active.map(\.id) == ["zed-thread"])
    }

    @Test("Session row display identity includes assistant surface")
    func sessionRowDisplayIdentityIncludesAssistantSurface() throws {
        let session = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )

        let codexItem = SessionRowDisplayItem(
            session: session,
            assistantSurface: .codex
        )
        let devinItem = SessionRowDisplayItem(
            session: session,
            assistantSurface: .devin
        )

        #expect(codexItem.id == "codex:thread-main")
        #expect(devinItem.id == "devin:thread-main")
        #expect(codexItem.id != devinItem.id)
    }

    @Test("Search ignores stale Spotlight IDs absent from local minis")
    func searchIgnoresStaleSpotlightIDsAbsentFromLocalMinis() throws {
        let freshSession = try sessionSummary(
            id: "fresh-local-thread",
            ref: "L1",
            activityMilliseconds: Constants.activityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let staleSpotlightIdentifier = LooperSessionEntityIdentifier(
            assistantSurface: .codex,
            sessionID: "stale-http-thread"
        ).rawValue

        let results = SessionSearchResults(
            searchText: "thread",
            selectedScope: .sessions,
            allSessions: [freshSession],
            needsAttentionSessions: [],
            runningSessions: [freshSession],
            stoppedSessions: [],
            archivedSessions: [],
            spotlightResultSessionIDs: [
                staleSpotlightIdentifier,
                "stale-http-thread",
            ]
        )

        #expect(results.visibleRunningSessions.map(\.id) == ["fresh-local-thread"])
        #expect(results.visibleStoppedSessions.isEmpty)
        #expect(results.visibleArchivedSessions.isEmpty)
        #expect(!results.visibleRunningSessions.map(\.id).contains("stale-http-thread"))
    }

    private func sessionSummary(
        id: String,
        ref: String,
        status: String = "active",
        activityMilliseconds: Int64,
        messageMilliseconds: Int64,
        isArchived: Bool = false,
        assistantClient: String? = nil,
        goal: [String: Any]? = nil
    ) throws -> SessionSummary {
        let payload = sessionPayload(
            id: id,
            ref: ref,
            status: status,
            activityMilliseconds: activityMilliseconds,
            messageMilliseconds: messageMilliseconds,
            isArchived: isArchived,
            assistantClient: assistantClient,
            goal: goal
        )
        let data = try JSONSerialization.data(withJSONObject: payload)
        return try decoder.decode(SessionSummary.self, from: data)
    }

    private func projection(
        sessionID: String,
        text: String,
        serverTime: String
    ) -> ClientSessionDetailProjection {
        ClientSessionDetailProjection(
            sessionId: sessionID,
            hasLatestReply: true,
            latestReply: ClientSessionLatestReply(
                sessionId: sessionID,
                messageId: "message-\(serverTime)",
                text: text,
                latestSeq: 1,
                isFinal: true,
                isTruncated: false,
                serverTime: serverTime
            )
        )
    }

    private func sessionPayload(
        id: String,
        ref: String,
        status: String = "active",
        activityMilliseconds: Int64,
        messageMilliseconds: Int64,
        isArchived: Bool = false,
        assistantClient: String? = nil,
        goal: [String: Any]? = nil
    ) -> [String: Any] {
        var payload: [String: Any] = [
            "id": id,
            "ref": ref,
            "title": id,
            "status": status,
            "lastUpdatedAt": "2026-06-16T08:02:00Z",
            "updatedAtMs": activityMilliseconds,
            "lastActivityAt": "2026-06-16T08:02:00Z",
            "lastActivityAtMs": activityMilliseconds,
            "lastMessageAt": "2026-06-16T08:01:00Z",
            "lastMessageAtMs": messageMilliseconds,
            "isArchived": isArchived,
            "metadata": [
                "kind": "project",
                "source": "vscode",
                "sourceDisplayName": "Codex",
                "taskKind": "implementation",
                "supportsSubagents": true,
                "spawn": [
                    "children": ["child-thread"],
                    "launchKind": "main",
                ],
            ],
        ]
        if let assistantClient {
            payload["assistantClient"] = assistantClient
        }
        if let goal {
            payload["goal"] = goal
        }
        return payload
    }

    private func temporarySessionRuntime(
        latestSeq: Int64,
        records: [SiriRuntimeMiniRecord]
    ) throws -> TemporarySiriRuntime {
        let store = try temporaryStoreFile()
        try seedMiniCache(at: store.fileURL, latestSeq: latestSeq, records: records)
        return TemporarySiriRuntime(
            runtime: try CompanionSessionRuntime(fileURL: store.fileURL),
            directoryURL: store.directoryURL
        )
    }

    private func temporaryStoreFile() throws -> (directoryURL: URL, fileURL: URL) {
        let directoryURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(Constants.siriRuntimeStoreDirectoryName, isDirectory: true)
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return (
            directoryURL,
            directoryURL.appendingPathComponent(CompanionSessionRuntime.defaultFileName)
        )
    }

    private func seedMiniCache(
        at fileURL: URL,
        latestSeq: Int64,
        records: [SiriRuntimeMiniRecord]
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
            "pendingCommands": [],
            "serverTime": Constants.hostSyncTime,
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        try data.write(to: fileURL, options: .atomic)
    }

    private func miniRecord(
        session: SessionSummary,
        assistantSurface: CompanionAssistantSurface,
        seq: Int64,
        revision: String
    ) throws -> SiriRuntimeMiniRecord {
        let data = try JSONEncoder().encode(session)
        return SiriRuntimeMiniRecord(
            sessionID: session.id,
            assistantSurface: assistantSurface.rawValue,
            seq: seq,
            revision: revision,
            payloadJSON: String(decoding: data, as: UTF8.self)
        )
    }

    private func unusedServiceSnapshot() -> MobileSnapshot {
        MobileSnapshot(
            revision: "unused-service-snapshot",
            host: HostSummary(
                id: "unused-host",
                name: "Unused",
                address: "http://127.0.0.1:8765",
                isReachable: false,
                lastSyncedAt: Constants.hostSyncTime
            ),
            globalSettings: GlobalSettings(
                defaultPrompt: "Unused",
                globalMode: nil,
                scope: "global",
                notificationLabel: nil,
                completionCheckLabel: nil,
                completionCheckWaitForReply: false,
                assistantSurface: .codex
            ),
            sessions: [],
            surfaceSessions: [:],
            notifications: [],
            completionChecks: []
        )
    }

    private func isDate(_ date: Date?, equalToMilliseconds milliseconds: Int64) -> Bool {
        guard let date else {
            return false
        }

        let expectedTimeInterval = TimeInterval(milliseconds) / Constants.millisecondsPerSecond
        return abs(date.timeIntervalSince1970 - expectedTimeInterval) < Constants.dateToleranceSeconds
    }
}

private enum SnapshotOnlyCompanionServiceError: Error {
    case unimplemented
}

private struct TemporarySiriRuntime {
    let runtime: CompanionSessionRuntime
    let directoryURL: URL

    func cleanup() {
        try? FileManager.default.removeItem(at: directoryURL)
    }
}

private struct SiriRuntimeMiniRecord: Equatable, Sendable {
    let sessionID: String
    let assistantSurface: String
    let seq: Int64
    let revision: String
    let payloadJSON: String
}

private struct SnapshotOnlyCompanionService: CompanionService {
    let snapshot: MobileSnapshot

    func loadServerHealth() async throws -> CompanionServerHealth {
        CompanionServerHealth(
            ok: true,
            baseURL: "http://127.0.0.1:8765",
            baseURLs: ["http://127.0.0.1:8765"],
            serverTime: "2026-06-16T08:02:00Z"
        )
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        snapshot
    }

    func loadSessionDetail(
        id _: String,
        surface _: CompanionAssistantSurface?
    ) async throws -> SessionDetail {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func setSessionArchived(id _: String, archived _: Bool) async throws -> MobileSnapshot {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func deleteSession(id _: String) async throws -> MobileSnapshot {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func muteSession(id _: String) async throws -> MobileSnapshot {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func saveDefaultPrompt(_: String) async throws -> MobileSnapshot {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func saveSiriDefaultSession(
        id _: String?,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func registerPushDevice(
        _: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }

    func sendTestPush(installationID _: String) async throws -> RemotePushTestResponse {
        throw SnapshotOnlyCompanionServiceError.unimplemented
    }
}

@Suite("Looper settings deep links")
struct LooperSettingsDeepLinkTests {
    @Test("Builds section settings URLs")
    func buildsSectionSettingsURLs() throws {
        let url = try #require(LooperSettingsDeepLink.url(for: .continuePrompt))

        #expect(url.absoluteString == "looper://settings/continuePrompt")
        #expect(LooperSettingsDeepLink.target(from: url) == .continuePrompt)
    }

    @Test("Parses query-form settings targets")
    func parsesQueryFormSettingsTargets() throws {
        let url = try #require(URL(string: "looper://settings?target=notificationRoutes"))

        #expect(LooperSettingsDeepLink.target(from: url) == .notificationRoutes)
    }

    @Test("Rejects session and connection links")
    func rejectsSessionAndConnectionLinks() throws {
        let sessionURL = try #require(URL(string: "looper://session/thread-main"))
        let connectionURL = try #require(URL(string: "looper://connect?baseURL=http://127.0.0.1:8765"))

        #expect(LooperSettingsDeepLink.target(from: sessionURL) == nil)
        #expect(LooperSettingsDeepLink.target(from: connectionURL) == nil)
    }
}

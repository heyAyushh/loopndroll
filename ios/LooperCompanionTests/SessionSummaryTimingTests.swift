import Foundation
import LooperClientCore
import Testing
@testable import Looper

@Suite("Session summary timing")
struct SessionSummaryTimingTests {
    private enum Constants {
        static let millisecondsPerSecond: TimeInterval = 1_000
        static let dateToleranceSeconds: TimeInterval = 0.000_001
        static let activityMilliseconds: Int64 = 1_781_596_920_321
        static let olderActivityMilliseconds: Int64 = 1_781_596_920_123
        static let messageMilliseconds: Int64 = 1_781_596_860_123
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
                CompanionAssistantSurface.grokBuild.rawValue: [middleGrok],
                CompanionAssistantSurface.zed.rawValue: [archivedZed],
            ],
            notifications: [],
            completionChecks: []
        )
        let client = LooperSiriSessionClient(
            service: SnapshotOnlyCompanionService(snapshot: snapshot)
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
        let snapshot = siriRoutingSnapshot(
            globalSettings: GlobalSettings(
                defaultPrompt: "Continue",
                globalMode: nil,
                scope: "global",
                notificationLabel: nil,
                completionCheckLabel: nil,
                completionCheckWaitForReply: false,
                assistantSurface: .codex,
                siriDefaultSessionId: "default-thread",
                siriDefaultAssistantSurface: .codex,
                siriCurrentSessionId: "current-thread",
                siriCurrentAssistantSurface: nil
            ),
            codexSessions: [defaultCodex],
            devinSessions: [currentDevin]
        )
        let client = LooperSiriSessionClient(
            service: SnapshotOnlyCompanionService(snapshot: snapshot)
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
        let snapshot = siriRoutingSnapshot(
            globalSettings: GlobalSettings(
                defaultPrompt: "Continue",
                globalMode: nil,
                scope: "global",
                notificationLabel: nil,
                completionCheckLabel: nil,
                completionCheckWaitForReply: false,
                assistantSurface: .codex,
                siriDefaultSessionId: "default-thread",
                siriDefaultAssistantSurface: .codex,
                siriCurrentSessionId: "stale-thread",
                siriCurrentAssistantSurface: .devin
            ),
            codexSessions: [defaultCodex],
            devinSessions: []
        )
        let client = LooperSiriSessionClient(
            service: SnapshotOnlyCompanionService(snapshot: snapshot)
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
        store.setDetail(
            SessionDetail(
                id: "thread-main",
                ref: "S2",
                title: "thread-main",
                status: .stopped,
                effectiveMode: .infinite,
                lastUpdatedAt: "2026-06-16T07:00:00Z",
                lastActivityAt: "2026-06-16T07:00:00Z",
                lastMessageAt: "2026-06-16T07:00:00Z",
                assistantPreview: "Old detail",
                latestAssistantMessage: nil,
                isArchived: true,
                notificationIds: [],
                completionCheckID: nil,
                completionCheckWaitForReply: false,
                availableNotifications: [],
                availableCompletionChecks: []
            ),
            for: "thread-main"
        )

        store.applySnapshot(snapshot)
        #expect(store.selectedAssistantSurface == .codex)
        #expect(store.sessionSections.active.map(\.ref) == ["S1"])
        #expect(store.detail(for: "thread-main")?.effectiveMode == .maxTurns1)
        #expect(store.detail(for: "thread-main")?.assistantPreview == "Codex ready")
        #expect(!store.selectAssistantSurface(.codex))

        #expect(store.selectAssistantSurface(.devin))
        #expect(store.selectedAssistantSurface == .devin)
        #expect(store.sessionSections.active.map(\.ref) == ["S2"])
        #expect(store.session(withID: "thread-main")?.ref == "S2")
        #expect(store.detail(for: "thread-main")?.effectiveMode == .awaitReply)
        #expect(store.detail(for: "thread-main")?.assistantPreview == "Devin ready")

        #expect(store.session(withID: "thread-main")?.effectiveMode == .awaitReply)
        #expect(store.detail(for: "thread-main")?.effectiveMode == .awaitReply)
    }

    private func sessionSummary(
        id: String,
        ref: String,
        status: String = "active",
        activityMilliseconds: Int64,
        messageMilliseconds: Int64,
        isArchived: Bool = false,
        goal: [String: Any]? = nil
    ) throws -> SessionSummary {
        let payload = sessionPayload(
            id: id,
            ref: ref,
            status: status,
            activityMilliseconds: activityMilliseconds,
            messageMilliseconds: messageMilliseconds,
            isArchived: isArchived,
            goal: goal
        )
        let data = try JSONSerialization.data(withJSONObject: payload)
        return try decoder.decode(SessionSummary.self, from: data)
    }

    private func siriRoutingSnapshot(
        globalSettings: GlobalSettings,
        codexSessions: [SessionSummary],
        devinSessions: [SessionSummary]
    ) -> MobileSnapshot {
        MobileSnapshot(
            revision: "siri-routing",
            host: HostSummary(
                id: "host",
                name: "Looper",
                address: "http://127.0.0.1:8765",
                isReachable: true,
                lastSyncedAt: "2026-06-16T08:02:00Z"
            ),
            globalSettings: globalSettings,
            sessions: codexSessions,
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: codexSessions,
                CompanionAssistantSurface.devin.rawValue: devinSessions,
            ],
            notifications: [],
            completionChecks: []
        )
    }

    private func sessionPayload(
        id: String,
        ref: String,
        status: String = "active",
        activityMilliseconds: Int64,
        messageMilliseconds: Int64,
        isArchived: Bool = false,
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
        if let goal {
            payload["goal"] = goal
        }
        return payload
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

    func saveSiriCurrentSession(
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

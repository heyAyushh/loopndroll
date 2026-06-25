import Foundation
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

        #expect(SessionSummary.isNewerOrLowerRef(leftSession: newer, rightSession: older))
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
        #expect(index.allSessions.map(\.ref) == ["S2"])
        #expect(index.session(withID: "thread-main")?.ref == "S2")
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
        let codexSession = try sessionSummary(
            id: "thread-main",
            ref: "S1",
            activityMilliseconds: Constants.olderActivityMilliseconds,
            messageMilliseconds: Constants.messageMilliseconds
        )
        let devinSession = try sessionSummary(
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
            sessions: [codexSession],
            surfaceSessions: [
                CompanionAssistantSurface.codex.rawValue: [codexSession],
                CompanionAssistantSurface.devin.rawValue: [devinSession],
            ],
            notifications: [],
            completionChecks: []
        )
        let store = CompanionSnapshotStateStore()

        store.applySnapshot(snapshot)
        #expect(store.selectedAssistantSurface == .codex)
        #expect(store.sessionSections.active.map(\.ref) == ["S1"])

        store.selectAssistantSurface(.devin)
        #expect(store.selectedAssistantSurface == .devin)
        #expect(store.sessionSections.active.map(\.ref) == ["S2"])
        #expect(store.session(withID: "thread-main")?.ref == "S2")
    }

    private func sessionSummary(
        id: String,
        ref: String,
        status: String = "active",
        activityMilliseconds: Int64,
        messageMilliseconds: Int64,
        goal: [String: Any]? = nil
    ) throws -> SessionSummary {
        let payload = sessionPayload(
            id: id,
            ref: ref,
            status: status,
            activityMilliseconds: activityMilliseconds,
            messageMilliseconds: messageMilliseconds,
            goal: goal
        )
        let data = try JSONSerialization.data(withJSONObject: payload)
        return try decoder.decode(SessionSummary.self, from: data)
    }

    private func sessionPayload(
        id: String,
        ref: String,
        status: String = "active",
        activityMilliseconds: Int64,
        messageMilliseconds: Int64,
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

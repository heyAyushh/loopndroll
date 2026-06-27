import Foundation
import LooperClientCore
import Testing
@testable import Looper

@Suite("CompanionSessionMiniLocalFirstTests")
struct CompanionSessionMiniLocalFirstTests {
    private enum Constants {
        static let cachedThreadID = "cached-thread"
        static let fallbackThreadID = "fallback-thread"
        static let timestamp = "2026-06-24T00:00:00Z"
        static let heartbeatTimestamp = "2026-06-24T00:00:15Z"
        static let preAckLocalPaintProbeNanoseconds: UInt64 = 20_000_000
        static let delayedModeDrainProbeNanoseconds: UInt64 = 300_000_000
        static let slowQuickActionHandlerNanoseconds: UInt64 = 250_000_000
        static let quickActionSubmitBudgetNanoseconds: UInt64 = 100_000_000
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
        #expect(model.viewState.connectivityHeadline != "Connecting to your Mac")
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
        #expect(model.viewState.connectivityHeadline != "Connecting to your Mac")
        #expect(model.viewState.connectivityStatusLabel == "Local")
        #expect(model.viewState.connectivitySummary == "Showing local sessions; live connection is not ready.")
        #expect(model.viewState.deviceHubAccessStatusLabel == "Local")
        #expect(model.viewState.deviceHubAPIStatusLabel == "Local")

        model.connectionState = .offline
        #expect(model.viewState.connectivityHeadline != "Mac connection offline")
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
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionRuntime: runtime
        )

        model.realtimeServerTime = Constants.heartbeatTimestamp
        model.realtimeLatestSeq = 10
        model.connectionState = .connected
        model.activeSessionRouteBaseURL = URL(string: "http://192.168.2.10:8766")

        #expect(model.realtimeServerTime == Constants.heartbeatTimestamp)
        #expect(!model.viewState.connectivitySummary.localizedCaseInsensitiveContains("stream"))
        #expect(!model.viewState.connectivitySummary.contains("synced "))
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

        model.selectAssistantSurface(.claudeCode)
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
    func testCurrentSiriSessionSelectionIsLocalOnly() async throws {
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

        model.markCurrentSiriSession(cachedSession)

        #expect(model.snapshot?.globalSettings.siriCurrentSessionId == Constants.cachedThreadID)
        #expect(model.snapshot?.globalSettings.siriCurrentAssistantSurface == .codex)
        #expect(model.snapshot?.globalSettings.siriCurrentUpdatedAtMs != nil)
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testAssistantSurfaceSwitchIsLocalOnly() async throws {
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

        model.selectAssistantSurface(.devin)

        #expect(model.viewState.selectedAssistantSurface == .devin)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(model.connectionState == .connecting)
        #expect(model.errorMessage == nil)
        #expect(model.viewState.connectivityHeadline != "Mac connection offline")
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
        #expect(service.loadSnapshotCallCount == 1)
    }

    @MainActor
    @Test
    func testMalformedMiniCacheFallsBackAndOutboxKeepsOfflinePrompt() async throws {
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
        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID)?.title == "Network Fallback")

        let didSend = await model.sendSessionPrompt("continue", to: Constants.fallbackThreadID)

        #expect(didSend)
        let pendingCommand = try Self.pendingCommand(in: runtime, kind: .sendSessionPrompt)
        #expect(pendingCommand.threadID == Constants.fallbackThreadID)
        #expect(pendingCommand.prompt == "continue")
        #expect(pendingCommand.attemptCount == 1)
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

    private static func temporarySessionRuntime(
        latestSeq: Int64,
        records: [SessionMiniFixture]
    ) throws -> CompanionSessionRuntime {
        let fileURL = try temporaryStoreFileURL()
        try seedMiniCache(at: fileURL, latestSeq: latestSeq, records: records)
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

    private static func seedMiniCache(
        at fileURL: URL,
        latestSeq: Int64,
        records: [SessionMiniFixture]
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
            "serverTime": Constants.timestamp,
        ]
        let data = try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])
        try data.write(to: fileURL, options: .atomic)
    }

    private static func miniRecord(
        session: SessionSummary,
        seq: Int64,
        revision: String
    ) throws -> SessionMiniFixture {
        let data = try JSONEncoder().encode(session)
        return SessionMiniFixture(
            sessionID: session.id,
            assistantSurface: CompanionAssistantSurface.codex.rawValue,
            seq: seq,
            revision: revision,
            payloadJSON: String(decoding: data, as: UTF8.self)
        )
    }

    private static func networkSnapshot() -> MobileSnapshot {
        let session = sessionSummary(
            id: Constants.fallbackThreadID,
            title: "Network Fallback",
            ref: "N1",
            status: .active
        )
        return MobileSnapshot(
            revision: "network-revision",
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
            sessions: [session],
            surfaceSessions: [CompanionAssistantSurface.codex.rawValue: [session]],
            notifications: [],
            completionChecks: []
        )
    }

    private static func sessionSummary(
        id: String,
        title: String,
        ref: String,
        status: SessionStatus
    ) -> SessionSummary {
        SessionSummary(
            id: id,
            ref: ref,
            title: title,
            status: status,
            effectiveMode: nil,
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

    init(snapshot: MobileSnapshot) {
        self.snapshot = snapshot
    }

    func loadServerHealth() async throws -> CompanionServerHealth {
        CompanionServerHealth(
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

import Foundation
import LooperRealtime
import Testing
@testable import Looper

@Suite("CompanionSessionMiniLocalFirstTests")
struct CompanionSessionMiniLocalFirstTests {
    private enum Constants {
        static let cachedThreadID = "cached-thread"
        static let fallbackThreadID = "fallback-thread"
        static let timestamp = "2026-06-24T00:00:00Z"
        static let delayedModeDrainProbeNanoseconds: UInt64 = 300_000_000
        static let slowQuickActionHandlerNanoseconds: UInt64 = 250_000_000
        static let quickActionSubmitBudgetNanoseconds: UInt64 = 100_000_000
    }

    @MainActor
    @Test
    func testAppModelRestoresCachedSessionMinisBeforeNetwork() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let store = try Self.temporaryMiniStore()
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 7,
            records: [
                Self.miniRecord(session: cachedSession, seq: 7, revision: "mini-revision-7"),
            ]
        )

        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini")
        #expect(model.activeSessions.map { $0.id } == [Constants.cachedThreadID])
        #expect(service.loadSnapshotCallCount == 0)
    }

    @MainActor
    @Test
    func testMalformedMiniCacheFallsBackAndOutboxKeepsFailedCommand() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.promptError = SessionMiniLocalFirstServiceSpy.ServiceError.promptFailed
        let storeFileURL = try Self.temporaryStoreFileURL()
        try Self.seedMalformedMiniCache(at: storeFileURL)
        let store = try CompanionSessionMiniLocalStore(fileURL: storeFileURL)

        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )
        #expect(model.snapshot == nil)

        await model.loadSnapshot()
        #expect(model.snapshot?.session(withID: Constants.fallbackThreadID)?.title == "Network Fallback")

        let didSend = await model.sendSessionPrompt("continue", to: Constants.fallbackThreadID)

        #expect(!didSend)
        let pendingCommands = store.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.threadID == Constants.fallbackThreadID)
        #expect(pendingCommands.first?.clientMutationID.isEmpty == false)
        #expect(pendingCommands.first?.attemptCount == 1)

        try store.enqueuePromptCommand(
            threadID: Constants.fallbackThreadID,
            prompt: "continue",
            assistantSurface: .codex,
            clientMutationID: pendingCommands.first?.clientMutationID ?? ""
        )
        #expect(store.pendingCommands().count == 1)
    }

    @Test
    func testStateMiniSynchronizerUsesRustCoreStoreAndPreservesOutbox() async throws {
        let store = try Self.temporaryMiniStore()
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 5,
            records: [
                Self.miniRecord(session: cachedSession, seq: 5, revision: "mini-revision-5"),
            ]
        )
        try store.enqueuePromptCommand(
            threadID: Constants.cachedThreadID,
            prompt: "continue",
            assistantSurface: .codex,
            clientMutationID: "mutation-outbox"
        )
        let staleSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Stale Mini",
            ref: "C1",
            status: .active
        )
        let streamedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Streamed Mini",
            ref: "C1",
            status: .active
        )
        let transport = StateMiniDeltaTransport(deltas: [
            try Self.miniDelta(session: staleSession, seq: 4, revision: "mini-revision-4"),
            try Self.miniDelta(session: streamedSession, seq: 6, revision: "mini-revision-6"),
        ])
        let synchronizer = LooperRealtimeStateMiniSynchronizer(store: store, transport: transport)

        let result = await synchronizer.runOneCycle { _ in }

        #expect(result == .streamEnded(latestSeq: 6))
        #expect(await transport.requestedAfterSeq() == 5)
        #expect(try store.cachedSnapshot()?.session(withID: Constants.cachedThreadID)?.title == "Streamed Mini")
        #expect(store.pendingCommands().map(\.clientMutationID) == ["mutation-outbox"])
    }

    @MainActor
    @Test
    func testOptimisticCommandsUseClientMutationIDsWithoutSnapshotRefresh() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let store = try Self.temporaryMiniStore()
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        await model.applyMode(.maxTurns2, to: Constants.cachedThreadID)
        let didSend = await model.sendSessionPrompt("ship it", to: Constants.cachedThreadID)
        try await Task.sleep(for: .milliseconds(200))

        #expect(didSend)
        #expect(model.snapshot?.session(withID: Constants.cachedThreadID)?.effectiveMode == .maxTurns2)
        #expect(service.modeClientMutationIDs.count == 1)
        #expect(service.promptClientMutationIDs.count == 1)
        #expect(service.modeClientMutationIDs.first?.isEmpty == false)
        #expect(service.promptClientMutationIDs.first?.isEmpty == false)
        #expect(service.modeClientMutationIDs.first != service.promptClientMutationIDs.first)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(store.pendingCommands().isEmpty)
    }

    @MainActor
    @Test
    func testPromptBehindPendingModeUsesBatchCommandWhenSupported() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.isModePromptBatchSupported = true
        service.modeResponseDelayNanoseconds = 200_000_000
        let store = try Self.temporaryMiniStore()
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        let promptTask = model.beginSendSessionPrompt("ship it", to: Constants.cachedThreadID)
        let didSendPrompt = await promptTask.value
        let didApplyMode = await modeTask.value

        #expect(didSendPrompt)
        #expect(didApplyMode)
        #expect(service.batchModeClientMutationIDs.count == 1)
        #expect(service.batchPromptClientMutationIDs.count == 1)
        #expect(service.batchModeClientMutationIDs.first != service.batchPromptClientMutationIDs.first)
        #expect(service.modeClientMutationIDs.isEmpty)
        #expect(service.promptClientMutationIDs.isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(store.pendingCommands().isEmpty)

        try await Task.sleep(nanoseconds: Constants.delayedModeDrainProbeNanoseconds)
        #expect(service.modeClientMutationIDs.isEmpty)
    }

    @MainActor
    @Test
    func testFailedModePromptBatchDoesNotFallbackToUnaryPrompt() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.isModePromptBatchSupported = true
        service.modePromptBatchError = SessionMiniLocalFirstServiceSpy.ServiceError.promptFailed
        service.modeResponseDelayNanoseconds = 200_000_000
        let store = try Self.temporaryMiniStore()
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 11,
            records: [
                Self.miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        let promptTask = model.beginSendSessionPrompt("ship it", to: Constants.cachedThreadID)
        let didSendPrompt = await promptTask.value
        let didApplyMode = await modeTask.value

        #expect(!didSendPrompt)
        #expect(didApplyMode)
        #expect(service.batchModeClientMutationIDs.count == 1)
        #expect(service.batchPromptClientMutationIDs.count == 1)
        #expect(service.promptClientMutationIDs.isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(store.pendingCommands().count == 1)
    }

    @MainActor
    @Test
    func testNotificationReplyUsesDurableAckCommandWithoutSnapshotRefresh() async throws {
        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        let store = try Self.temporaryMiniStore()
        let cachedSession = Self.sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .stopped
        )
        try store.replace(
            latestSeq: 12,
            records: [
                Self.miniRecord(session: cachedSession, seq: 12, revision: "mini-revision-12"),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )
        let notificationID = "notif-cached-1"
        let clientMutationID = SessionQuickActionRequest.notificationReplyClientMutationID(
            notificationID: notificationID
        )

        await model.performQuickAction(
            .reply,
            sessionID: Constants.cachedThreadID,
            prompt: "continue from notification",
            notificationID: notificationID,
            clientMutationID: clientMutationID
        )

        #expect(service.notificationReplyClientMutationIDs == [clientMutationID])
        #expect(service.notificationReplyIDs == [notificationID])
        #expect(service.promptClientMutationIDs.isEmpty)
        #expect(service.loadSnapshotCallCount == 0)
        #expect(store.pendingCommands().isEmpty)
    }

    @MainActor
    @Test
    func testNotificationReplyPersistsBeforeHandlerAndDedupesOfflineRetry() async throws {
        let store = try Self.temporaryMiniStore()
        let notificationID = "notif-offline-1"
        let clientMutationID = SessionQuickActionRequest.notificationReplyClientMutationID(
            notificationID: notificationID
        )
        let center = SessionQuickActionCenter(localStore: store)

        await center.submit(
            SessionQuickActionRequest(
                action: .reply,
                sessionID: Constants.cachedThreadID,
                prompt: "offline reply",
                notificationID: notificationID
            )
        )

        var pendingCommands = store.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.threadID == Constants.cachedThreadID)
        #expect(pendingCommands.first?.notificationID == notificationID)
        #expect(pendingCommands.first?.prompt == "offline reply")
        #expect(pendingCommands.first?.clientMutationID == clientMutationID)
        #expect(pendingCommands.first?.attemptCount == 0)

        let service = SessionMiniLocalFirstServiceSpy(snapshot: Self.networkSnapshot())
        service.promptError = SessionMiniLocalFirstServiceSpy.ServiceError.promptFailed
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )
        await model.performQuickAction(
            .reply,
            sessionID: Constants.cachedThreadID,
            prompt: "offline reply",
            notificationID: notificationID,
            clientMutationID: clientMutationID
        )

        pendingCommands = store.pendingCommands()
        #expect(pendingCommands.count == 1)
        #expect(pendingCommands.first?.kind == .submitNotificationReply)
        #expect(pendingCommands.first?.attemptCount == 1)
        #expect(service.notificationReplyClientMutationIDs.isEmpty)
        try store.enqueueNotificationReplyCommand(
            notificationID: notificationID,
            threadID: Constants.cachedThreadID,
            prompt: "offline reply",
            assistantSurface: nil,
            clientMutationID: clientMutationID
        )
        #expect(store.pendingCommands().count == 1)
    }

    @MainActor
    @Test
    func testNotificationReplyQuickActionSubmitDoesNotWaitForNetworkHandler() async throws {
        let store = try Self.temporaryMiniStore()
        let notificationID = "notif-fast-completion-1"
        let center = SessionQuickActionCenter(localStore: store)
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
        #expect(store.pendingCommands().count == 1)
        #expect(store.pendingCommands().first?.notificationID == notificationID)
    }

    private static func temporaryMiniStore() throws -> CompanionSessionMiniLocalStore {
        try CompanionSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
    }

    private static func temporaryStoreFileURL() throws -> URL {
        let directoryURL = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .appendingPathComponent(
                ".test-artifacts/session-mini-local-first/\(UUID().uuidString)",
                isDirectory: true
            )
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return directoryURL.appendingPathComponent(CompanionSessionMiniLocalStore.defaultFileName)
    }

    private static func seedMalformedMiniCache(at fileURL: URL) throws {
        let store = try LooperRealtimeLocalStore(fileURL: fileURL)
        try store.replace(
            with: LooperRealtimeStateMiniSnapshot(
                latestSeq: 3,
                sessions: [
                    LooperRealtimeStateMini(
                        sessionID: "bad-cache",
                        assistantSurface: CompanionAssistantSurface.codex.rawValue,
                        seq: 3,
                        revision: "bad-mini",
                        payloadJSON: "{not-json"
                    ),
                ],
                serverTime: Constants.timestamp
            )
        )
    }

    private static func miniRecord(
        session: SessionSummary,
        seq: Int64,
        revision: String
    ) throws -> CompanionSessionMiniRecord {
        let data = try JSONEncoder().encode(session)
        return CompanionSessionMiniRecord(
            sessionID: session.id,
            assistantSurface: CompanionAssistantSurface.codex.rawValue,
            seq: seq,
            revision: revision,
            payloadJSON: String(decoding: data, as: UTF8.self)
        )
    }

    private static func miniDelta(
        session: SessionSummary,
        seq: Int64,
        revision: String
    ) throws -> LooperRealtimeStateMiniDelta {
        let record = try miniRecord(session: session, seq: seq, revision: revision)
        let mini = LooperRealtimeStateMini(
            sessionID: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJSON: record.payloadJSON
        )
        return LooperRealtimeStateMiniDelta(
            seq: seq,
            latestSeq: seq,
            entityID: session.id,
            kind: "session_mini",
            revision: revision,
            serverTime: Constants.timestamp,
            session: mini,
            sessionID: mini.sessionID,
            assistantSurface: mini.assistantSurface,
            sessions: [mini]
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

private actor StateMiniDeltaTransport: LooperRealtimeStateMiniSyncTransport {
    private let deltas: [LooperRealtimeStateMiniDelta]
    private var afterSeq: Int64?

    init(deltas: [LooperRealtimeStateMiniDelta]) {
        self.deltas = deltas
    }

    func requestedAfterSeq() -> Int64? {
        afterSeq
    }

    func getStateMiniSnapshot() async throws -> LooperRealtimeStateMiniSnapshot {
        LooperRealtimeStateMiniSnapshot(latestSeq: 0, sessions: [], serverTime: nil)
    }

    func streamStateMinis(
        afterSeq: Int64,
        onDelta: @escaping @Sendable (LooperRealtimeStateMiniDelta) async throws -> Void
    ) async throws {
        self.afterSeq = afterSeq
        for delta in deltas {
            try await onDelta(delta)
        }
    }
}

private final class SessionMiniLocalFirstServiceSpy: CompanionService, @unchecked Sendable {
    enum ServiceError: Error {
        case promptFailed
    }

    private let lock = NSLock()
    private let snapshot: MobileSnapshot
    private(set) var loadSnapshotCallCount = 0
    private(set) var modeClientMutationIDs: [String] = []
    private(set) var promptClientMutationIDs: [String] = []
    private(set) var batchModeClientMutationIDs: [String] = []
    private(set) var batchPromptClientMutationIDs: [String] = []
    private(set) var notificationReplyClientMutationIDs: [String] = []
    private(set) var notificationReplyIDs: [String] = []
    var isModePromptBatchSupported = false
    var modeResponseDelayNanoseconds: UInt64 = 0
    var promptError: Error?
    var modePromptBatchError: Error?

    init(snapshot: MobileSnapshot) {
        self.snapshot = snapshot
    }

    var supportsModePromptBatch: Bool {
        isModePromptBatchSupported
    }

    func prepareRealtimeConnection() async {}

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
        return snapshot
    }

    func loadSessionDetail(id: String, surface _: CompanionAssistantSurface?) async throws -> SessionDetail {
        throw ServiceError.promptFailed
    }

    func setSessionMode(
        id: String,
        preset: SessionMode?,
        clientMutationID: String
    ) async throws -> CompanionSessionModeResult {
        if modeResponseDelayNanoseconds > 0 {
            try await Task.sleep(nanoseconds: modeResponseDelayNanoseconds)
        }
        try Task.checkCancellation()
        appendModeClientMutationID(clientMutationID)
        return .accepted(mode: preset, serverTime: nil, clientMutationID: clientMutationID)
    }

    func setSessionArchived(id _: String, archived _: Bool) async throws -> MobileSnapshot {
        snapshot
    }

    func deleteSession(id _: String) async throws -> MobileSnapshot {
        snapshot
    }

    func sendSessionPrompt(
        id _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> CompanionPromptSendResult {
        if let promptError {
            throw promptError
        }

        appendPromptClientMutationID(clientMutationID)
        return .accepted(
            promptID: "prompt-1",
            dispatchKind: "resume",
            clientMutationID: clientMutationID
        )
    }

    func sendSessionPromptAfterMode(
        id _: String,
        modePreset: SessionMode?,
        modeClientMutationID: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?,
        promptClientMutationID: String
    ) async throws -> CompanionModePromptBatchResult {
        appendBatchModeClientMutationID(modeClientMutationID)
        appendBatchPromptClientMutationID(promptClientMutationID)
        if let modePromptBatchError {
            throw modePromptBatchError
        }
        return CompanionModePromptBatchResult(
            mode: .accepted(
                mode: modePreset,
                serverTime: nil,
                clientMutationID: modeClientMutationID
            ),
            prompt: .accepted(
                promptID: "prompt-1",
                dispatchKind: "resume",
                clientMutationID: promptClientMutationID
            )
        )
    }

    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> LooperRealtimeNotificationReplyResponse {
        if let promptError {
            throw promptError
        }

        appendNotificationReply(notificationID: notificationID, clientMutationID: clientMutationID)
        return LooperRealtimeNotificationReplyResponse(
            accepted: true,
            dispatchKind: "resume",
            promptID: "prompt-1",
            serverTime: nil,
            clientMutationID: clientMutationID,
            ackSeq: 0,
            entityID: sessionID,
            revision: "",
            idempotentReplay: false,
            notificationID: notificationID
        )
    }

    private func incrementLoadSnapshotCallCount() {
        lock.lock()
        defer { lock.unlock() }
        loadSnapshotCallCount += 1
    }

    private func appendModeClientMutationID(_ clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        modeClientMutationIDs.append(clientMutationID)
    }

    private func appendPromptClientMutationID(_ clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        promptClientMutationIDs.append(clientMutationID)
    }

    private func appendBatchModeClientMutationID(_ clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        batchModeClientMutationIDs.append(clientMutationID)
    }

    private func appendBatchPromptClientMutationID(_ clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        batchPromptClientMutationIDs.append(clientMutationID)
    }

    private func appendNotificationReply(notificationID: String, clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        notificationReplyIDs.append(notificationID)
        notificationReplyClientMutationIDs.append(clientMutationID)
    }

    func muteSession(id _: String) async throws -> MobileSnapshot {
        snapshot
    }

    func saveDefaultPrompt(_: String) async throws -> MobileSnapshot {
        snapshot
    }

    func saveAssistantSurface(_: CompanionAssistantSurface) async throws -> MobileSnapshot {
        snapshot
    }

    func saveSiriDefaultSession(
        id _: String?,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot {
        snapshot
    }

    func saveSiriCurrentSession(
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

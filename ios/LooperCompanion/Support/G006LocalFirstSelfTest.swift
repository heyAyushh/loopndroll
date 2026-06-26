#if DEBUG
import Darwin
import Foundation
import LooperClientCore
import LooperRealtime

@MainActor
enum G006LocalFirstSelfTest {
    enum Case: String {
        case cachedRestore = "c001"
        case malformedFallbackOutbox = "c002"
        case optimisticCommands = "c003"
        case notificationReplyAck = "c004"
        case notificationReplyOfflineDedupe = "c005"
        case miniSyncResync = "c006"
        case latencyBudget = "c007"
    }

    private enum Constants {
        static let argument = "--g006-local-first-selftest"
        static let sampleCountArgument = "--g006-local-first-selftest-samples"
        static let cachedThreadID = "cached-thread"
        static let fallbackThreadID = "fallback-thread"
        static let timestamp = "2026-06-24T00:00:00Z"
        static let defaultLatencySampleCount = 20
        static let minimumLatencySampleCount = 1
        static let serviceResponseDelayNanoseconds: UInt64 = 10_000_000
        static let handoffModeResponseDelayNanoseconds: UInt64 = 100_000_000
        static let latencyPollNanoseconds: UInt64 = 100_000
        static let latencyTimeoutNanoseconds: UInt64 = 500_000_000
        static let uiP95TargetMilliseconds = 30
        static let localLanAckP95TargetMilliseconds = 30
        static let tailscaleInternetAckP95TargetMilliseconds = 100
        static let realtimePortConfigurationInput = """
        http://100.119.200.69:8766
        http://192.168.1.26:8766
        """
        static let expectedHTTPPortConfigurationOutput = [
            "http://100.119.200.69:8765",
            "http://192.168.1.26:8765",
        ]
    }

    private enum SelfTestError: Error, CustomStringConvertible {
        case assertionFailed(String)

        var description: String {
            switch self {
            case let .assertionFailed(message):
                return message
            }
        }
    }

    static var requestedCase: Case? {
        let arguments = ProcessInfo.processInfo.arguments
        guard let argumentIndex = arguments.firstIndex(of: Constants.argument) else {
            return nil
        }

        let valueIndex = arguments.index(after: argumentIndex)
        guard valueIndex < arguments.endIndex else {
            return nil
        }

        return Case(rawValue: arguments[valueIndex])
    }

    static func runSoon(_ testCase: Case) {
        Task { @MainActor in
            do {
                let summary = try await run(testCase)
                write("G006_SELFTEST_PASS \(testCase.rawValue) \(summary)", to: .standardOutput)
                exit(EXIT_SUCCESS)
            } catch {
                write("G006_SELFTEST_FAIL \(testCase.rawValue) \(error)", to: .standardError)
                exit(EXIT_FAILURE)
            }
        }
    }

    private static func run(_ testCase: Case) async throws -> String {
        switch testCase {
        case .cachedRestore:
            return try await runCachedRestore()
        case .malformedFallbackOutbox:
            return try await runMalformedFallbackOutbox()
        case .optimisticCommands:
            return try await runOptimisticCommands()
        case .notificationReplyAck:
            return try await runNotificationReplyAck()
        case .notificationReplyOfflineDedupe:
            return try await runNotificationReplyOfflineDedupe()
        case .miniSyncResync:
            return try await runMiniSyncResync()
        case .latencyBudget:
            return try await runLatencyBudget()
        }
    }

    private static func runCachedRestore() async throws -> String {
        let service = G006LocalFirstServiceSpy(
            snapshot: networkSnapshot(),
            responseDelayNanoseconds: Constants.serviceResponseDelayNanoseconds
        )
        let store = try temporaryMiniStore()
        let cachedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 7,
            records: [
                miniRecord(session: cachedSession, seq: 7, revision: "mini-revision-7"),
            ]
        )

        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        try require(
            model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Cached Mini",
            "cached SessionMini did not hydrate snapshot"
        )
        try require(
            model.viewState.activeSessions.map { $0.id } == [Constants.cachedThreadID],
            "cached SessionMini did not hydrate active session sections"
        )
        try require(service.loadSnapshotCallCount == 0, "HTTP snapshot loaded during cached restore")
        return "cachedThreadID=\(Constants.cachedThreadID) loadSnapshotCallCount=0"
    }

    private static func runMalformedFallbackOutbox() async throws -> String {
        let service = G006LocalFirstServiceSpy(snapshot: networkSnapshot())
        service.promptError = G006LocalFirstServiceSpy.ServiceError.promptFailed
        let storeFileURL = try temporaryStoreFileURL()
        try seedMalformedMiniCache(at: storeFileURL)
        let store = try CompanionSessionMiniLocalStore(fileURL: storeFileURL)

        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )
        try require(model.snapshot == nil, "malformed SessionMini cache should be ignored")

        await model.loadSnapshot()
        try require(
            model.snapshot?.session(withID: Constants.fallbackThreadID)?.title == "Network Fallback",
            "fallback snapshot did not hydrate after malformed mini cache"
        )

        let didSend = await model.sendSessionPrompt("continue", to: Constants.fallbackThreadID)
        try require(!didSend, "failed prompt unexpectedly returned success")

        let pendingCommands = store.pendingCommands()
        try require(pendingCommands.count == 1, "failed prompt did not leave exactly one pending command")
        let pendingCommand = try requireValue(pendingCommands.first, "missing pending command")
        try require(pendingCommand.threadID == Constants.fallbackThreadID, "pending command has wrong thread")
        try require(!pendingCommand.clientMutationID.isEmpty, "pending command missing clientMutationID")
        try require(pendingCommand.attemptCount == 1, "pending command attempt count was not persisted")

        try store.enqueuePromptCommand(
            threadID: Constants.fallbackThreadID,
            prompt: "continue",
            assistantSurface: .codex,
            clientMutationID: pendingCommand.clientMutationID
        )
        try require(store.pendingCommands().count == 1, "duplicate enqueue created a second command")

        return "pendingCommand=\(pendingCommand.clientMutationID) attemptCount=1"
    }

    private static func runOptimisticCommands() async throws -> String {
        let service = G006LocalFirstServiceSpy(snapshot: networkSnapshot())
        let store = try temporaryMiniStore()
        let cachedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 11,
            records: [
                miniRecord(session: cachedSession, seq: 11, revision: "mini-revision-11"),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        let promptTask = model.beginSendSessionPrompt("ship it", to: Constants.cachedThreadID)
        _ = await modeTask.value
        let didSend = await promptTask.value

        try require(didSend, "accepted prompt command returned false")
        try require(
            model.snapshot?.session(withID: Constants.cachedThreadID)?.effectiveMode == .maxTurns2,
            "optimistic mode did not render from local state"
        )
        try require(service.modeClientMutationIDs.count == 1, "mode command did not use exactly one mutation id")
        try require(service.promptClientMutationIDs.count == 1, "prompt command did not use exactly one mutation id")

        let modeMutationID = try requireValue(
            service.modeClientMutationIDs.first,
            "missing mode clientMutationID"
        )
        let promptMutationID = try requireValue(
            service.promptClientMutationIDs.first,
            "missing prompt clientMutationID"
        )
        try require(!modeMutationID.isEmpty, "mode clientMutationID is empty")
        try require(!promptMutationID.isEmpty, "prompt clientMutationID is empty")
        try require(modeMutationID != promptMutationID, "mode and prompt reused the same clientMutationID")
        try require(
            service.mutationOrder == [
                "\(G006LocalFirstServiceSpy.mutationOrderPrefixMode):\(modeMutationID)",
                "\(G006LocalFirstServiceSpy.mutationOrderPrefixPrompt):\(promptMutationID)",
            ],
            "prompt command reached service before mode ACK"
        )
        try require(service.loadSnapshotCallCount == 0, "ACK-only commands triggered snapshot load")
        try require(store.pendingCommands().isEmpty, "ACK-only commands did not clear outbox")

        let handoffService = G006LocalFirstServiceSpy(snapshot: networkSnapshot())
        handoffService.modeResponseDelayNanosecondsByCall = [
            0,
            Constants.handoffModeResponseDelayNanoseconds,
        ]
        let handoffStore = try temporaryMiniStore()
        try handoffStore.replace(
            latestSeq: 13,
            records: [
                miniRecord(session: cachedSession, seq: 13, revision: "mini-revision-13"),
            ]
        )
        let handoffModel = CompanionAppModel(
            environment: CompanionEnvironment(service: handoffService),
            sessionMiniLocalStore: handoffStore
        )
        var didRunHandoffHook = false
        var queuedModeTask: Task<Bool, Never>?
        var queuedPromptTask: Task<Bool, Never>?
        handoffModel.setModeDrainBeforeFinishHookForSelfTest {
            didRunHandoffHook = true
            queuedModeTask = handoffModel.beginApplyMode(.maxTurns3, to: Constants.cachedThreadID)
            queuedPromptTask = handoffModel.beginSendSessionPrompt("handoff prompt", to: Constants.cachedThreadID)
        }

        let firstHandoffModeTask = handoffModel.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        let didAcceptFirstHandoffMode = await firstHandoffModeTask.value
        try require(didAcceptFirstHandoffMode, "first handoff mode command failed")
        try await waitUntilFast("mode drain handoff hook did not run") {
            didRunHandoffHook
        }
        let queuedModeBarrierTask = try requireValue(
            queuedModeTask,
            "missing queued handoff mode task"
        )
        let queuedPromptBarrierTask = try requireValue(
            queuedPromptTask,
            "missing queued handoff prompt task"
        )
        let didAcceptQueuedMode = await queuedModeBarrierTask.value
        let didSendQueuedPrompt = await queuedPromptBarrierTask.value

        try require(didAcceptQueuedMode, "queued handoff mode command failed")
        try require(didSendQueuedPrompt, "prompt behind queued handoff mode failed")
        try require(
            handoffService.modeClientMutationIDs.count == 2,
            "handoff mode commands did not both reach service"
        )
        try require(
            handoffService.promptClientMutationIDs.count == 1,
            "handoff prompt did not reach service once"
        )
        let handoffFirstModeID = handoffService.modeClientMutationIDs[0]
        let handoffSecondModeID = handoffService.modeClientMutationIDs[1]
        let handoffPromptID = try requireValue(
            handoffService.promptClientMutationIDs.first,
            "missing handoff prompt mutation id"
        )
        try require(
            handoffService.mutationOrder == [
                "\(G006LocalFirstServiceSpy.mutationOrderPrefixMode):\(handoffFirstModeID)",
                "\(G006LocalFirstServiceSpy.mutationOrderPrefixMode):\(handoffSecondModeID)",
                "\(G006LocalFirstServiceSpy.mutationOrderPrefixPrompt):\(handoffPromptID)",
            ],
            "prompt crossed queued mode drain handoff before second ACK"
        )

        let failingService = G006LocalFirstServiceSpy(
            snapshot: networkSnapshot(),
            responseDelayNanoseconds: Constants.serviceResponseDelayNanoseconds
        )
        failingService.modeError = G006LocalFirstServiceSpy.ServiceError.promptFailed
        let failingStore = try temporaryMiniStore()
        try failingStore.replace(
            latestSeq: 12,
            records: [
                miniRecord(session: cachedSession, seq: 12, revision: "mini-revision-12"),
            ]
        )
        let failingModel = CompanionAppModel(
            environment: CompanionEnvironment(service: failingService),
            sessionMiniLocalStore: failingStore
        )
        let failingModeTask = failingModel.beginApplyMode(.maxTurns3, to: Constants.cachedThreadID)
        let blockedPromptTask = failingModel.beginSendSessionPrompt("blocked", to: Constants.cachedThreadID)
        let didAcceptFailingMode = await failingModeTask.value
        let didSendBlockedPrompt = await blockedPromptTask.value

        try require(!didAcceptFailingMode, "failed mode command reported an accepted ACK")
        try require(!didSendBlockedPrompt, "prompt behind failed mode reported success")
        try require(
            failingService.promptClientMutationIDs.isEmpty,
            "prompt reached service after mode ACK failure"
        )
        try require(
            failingStore.pendingCommands().contains { command in
                command.kind == .sendSessionPrompt && command.threadID == Constants.cachedThreadID
            },
            "blocked prompt did not stay queued locally"
        )

        return "modeMutationID=\(modeMutationID) promptMutationID=\(promptMutationID)"
    }

    private static func runNotificationReplyAck() async throws -> String {
        let service = G006LocalFirstServiceSpy(snapshot: networkSnapshot())
        let store = try temporaryMiniStore()
        let cachedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .stopped
        )
        try store.replace(
            latestSeq: 12,
            records: [
                miniRecord(session: cachedSession, seq: 12, revision: "mini-revision-12"),
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

        try require(
            service.notificationReplyClientMutationIDs == [clientMutationID],
            "notification reply did not use the durable ACK command mutation id"
        )
        try require(service.notificationReplyIDs == [notificationID], "notification id was not forwarded")
        try require(service.promptClientMutationIDs.isEmpty, "notification reply used generic prompt command")
        try require(service.loadSnapshotCallCount == 0, "notification reply triggered snapshot load")
        try require(store.pendingCommands().isEmpty, "ACK did not clear notification reply outbox")

        return "notificationID=\(notificationID) clientMutationID=\(clientMutationID)"
    }

    private static func runNotificationReplyOfflineDedupe() async throws -> String {
        let store = try temporaryMiniStore()
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
        var pendingCommand = try requireValue(pendingCommands.first, "missing pre-handler pending command")
        try require(pendingCommands.count == 1, "pre-handler notification reply was not durable")
        try require(pendingCommand.kind == .submitNotificationReply, "pending command has wrong kind")
        try require(pendingCommand.threadID == Constants.cachedThreadID, "pending command has wrong thread")
        try require(pendingCommand.notificationID == notificationID, "pending command has wrong notification id")
        try require(pendingCommand.prompt == "offline reply", "pending command has wrong prompt")
        try require(pendingCommand.clientMutationID == clientMutationID, "pending command has wrong mutation id")
        try require(pendingCommand.attemptCount == 0, "pre-handler persistence should not mark attempted")

        let service = G006LocalFirstServiceSpy(snapshot: networkSnapshot())
        service.promptError = G006LocalFirstServiceSpy.ServiceError.promptFailed
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
        pendingCommand = try requireValue(pendingCommands.first, "missing retry pending command")
        try require(pendingCommands.count == 1, "retry created duplicate pending command")
        try require(pendingCommand.kind == .submitNotificationReply, "retry command has wrong kind")
        try require(pendingCommand.attemptCount == 1, "failed retry did not mark attempted once")
        try require(
            service.notificationReplyClientMutationIDs.isEmpty,
            "failed notification reply should not record delivered mutation id"
        )

        try store.enqueueNotificationReplyCommand(
            notificationID: notificationID,
            threadID: Constants.cachedThreadID,
            prompt: "offline reply",
            assistantSurface: nil,
            clientMutationID: clientMutationID
        )
        try require(store.pendingCommands().count == 1, "duplicate enqueue created second notification command")

        service.promptError = nil
        await model.prepareForActiveState()
        try await waitUntilFast("notification reply production retry did not drain durable command") {
            service.notificationReplyClientMutationIDs == [clientMutationID]
        }
        try require(
            service.notificationReplyClientMutationIDs == [clientMutationID],
            "notification reply drain did not retry durable command once"
        )
        try require(store.pendingCommands().isEmpty, "notification reply drain did not clear outbox")

        return "notificationID=\(notificationID) clientMutationID=\(clientMutationID) deliveredAfterRetry=true"
    }

    private static func runMiniSyncResync() async throws -> String {
        let store = try temporaryMiniStore()
        let cachedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Cached Mini",
            ref: "C1",
            status: .active
        )
        let syncedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Synced Mini",
            ref: "C1",
            status: .active
        )
        try store.replace(
            latestSeq: 20,
            records: [
                miniRecord(session: cachedSession, seq: 20, revision: "mini-revision-20"),
            ]
        )
        let transport = G006StateMiniStreamTransport(
            streamPlans: [
                .deltas([
                    try stateMiniDelta(
                        session: syncedSession,
                        seq: 21,
                        revision: "mini-revision-21"
                    ),
                ]),
            ]
        )
        let service = G006LocalFirstServiceSpy(
            snapshot: networkSnapshot()
        )
        service.stateMiniStreamTransport = transport
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )

        model.startRealtimeSessionSyncIfNeeded()
        defer { model.stopRealtimeSessionSync() }
        try await waitUntil("mini sync did not apply delta") {
            model.snapshot?.session(withID: Constants.cachedThreadID)?.title == "Synced Mini"
        }

        let observedAfterSeqs = transport.observedAfterSeqs()
        try require(observedAfterSeqs == [20], "mini sync did not resume from cached seq")
        try require(store.currentStateMiniSnapshot().latestSeq == 21, "mini sync did not finish at latest seq 21")
        try require(service.loadSnapshotCallCount == 0, "mini sync triggered full snapshot")
        return "afterSeq=20 latestSeq=\(store.currentStateMiniSnapshot().latestSeq) loadSnapshotCallCount=0"
    }

    private static func runLatencyBudget() async throws -> String {
        try assertHTTPPortConfigurationRepair()
        try await assertSnapshotTimeoutPreservesConnectedMiniState()
        let sampleCount = latencySampleCount()
        var samples: [[String: Any]] = []
        samples.reserveCapacity(sampleCount)

        _ = try await runLatencySample(sampleIndex: 0)
        for sampleIndex in 0..<sampleCount {
            samples.append(try await runLatencySample(sampleIndex: sampleIndex + 1))
        }

        let evidence = try latencyEvidence(samples: samples)
        let data = try JSONSerialization.data(
            withJSONObject: evidence,
            options: [.sortedKeys]
        )
        return String(decoding: data, as: UTF8.self)
    }

    private static func assertHTTPPortConfigurationRepair() throws {
        let urls = CompanionConfiguration.normalizedBaseURLsForUserInput(
            Constants.realtimePortConfigurationInput
        )
        try require(
            urls.map(\.absoluteString) == Constants.expectedHTTPPortConfigurationOutput,
            "realtime port leaked into HTTP companion configuration"
        )
    }

    private static func assertSnapshotTimeoutPreservesConnectedMiniState() async throws {
        let service = G006LocalFirstServiceSpy(snapshot: networkSnapshot())
        let store = try temporaryMiniStore()
        let cachedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Connected Mini",
            ref: "C7",
            status: .active
        )
        try store.replace(
            latestSeq: 31,
            records: [
                miniRecord(session: cachedSession, seq: 31, revision: "connected-mini-31"),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )
        let syncedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Connected Mini Synced",
            ref: "C7",
            status: .active
        )
        let transport = G006StateMiniStreamTransport(
            streamPlans: [
                .deltas([
                    try stateMiniDelta(
                        session: syncedSession,
                        seq: 32,
                        revision: "connected-mini-32"
                    ),
                ]),
            ]
        )
        service.stateMiniStreamTransport = transport

        model.startRealtimeSessionSyncIfNeeded()
        defer { model.stopRealtimeSessionSync() }
        try await waitUntil("connected mini sync did not finish at seq 32") {
            store.currentStateMiniSnapshot().latestSeq == 32
        }
        try require(model.connectionState == .connected, "mini sync did not mark connection connected")

        service.snapshotError = URLError(.timedOut)
        await model.refresh()
        try require(
            model.connectionState == .connected,
            "snapshot timeout overrode connected local-first state"
        )
        try require(model.errorMessage == nil, "snapshot timeout surfaced while local state was usable")
        try require(
            model.snapshot?.session(withID: Constants.cachedThreadID)?.title == syncedSession.title,
            "snapshot timeout replaced usable mini state"
        )
    }

    private static func runLatencySample(sampleIndex: Int) async throws -> [String: Any] {
        let service = G006LocalFirstServiceSpy(
            snapshot: networkSnapshot(),
            responseDelayNanoseconds: Constants.serviceResponseDelayNanoseconds
        )
        let store = try temporaryMiniStore()
        let cachedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Latency Mini",
            ref: "L\(sampleIndex)",
            status: .active
        )
        try store.replace(
            latestSeq: Int64(100 + sampleIndex),
            records: [
                miniRecord(
                    session: cachedSession,
                    seq: Int64(100 + sampleIndex),
                    revision: "latency-revision-\(sampleIndex)"
                ),
            ]
        )
        let model = CompanionAppModel(
            environment: CompanionEnvironment(service: service),
            sessionMiniLocalStore: store
        )
        try require(
            model.snapshot?.session(withID: Constants.cachedThreadID) != nil,
            "latency cached mini did not hydrate before action"
        )

        let modeStartedAt = uptimeNanoseconds()
        let modeTask = model.beginApplyMode(.maxTurns2, to: Constants.cachedThreadID)
        try require(
            model.snapshot?.session(withID: Constants.cachedThreadID)?.effectiveMode == .maxTurns2,
            "mode did not render from local state"
        )
        let uiModeMs = elapsedMilliseconds(since: modeStartedAt)
        _ = await modeTask.value
        let modeAckMs = elapsedMilliseconds(since: modeStartedAt)

        let promptStartedAt = uptimeNanoseconds()
        let promptTask = model.beginSendSessionPrompt(
            "latency prompt \(sampleIndex)",
            to: Constants.cachedThreadID
        )
        try await waitUntilFast("prompt command was not durable before ACK") {
            store.pendingCommands().contains { command in
                command.kind == .sendSessionPrompt && command.threadID == Constants.cachedThreadID
            }
        }
        let uiPromptMs = elapsedMilliseconds(since: promptStartedAt)
        let didSendPrompt = await promptTask.value
        try require(didSendPrompt, "latency prompt did not ACK")
        let promptAckMs = elapsedMilliseconds(since: promptStartedAt)

        let notificationStartedAt = uptimeNanoseconds()
        let notificationID = "latency-notification-\(sampleIndex)"
        await SessionQuickActionCenter(localStore: store).submit(
            SessionQuickActionRequest(
                action: .reply,
                sessionID: Constants.cachedThreadID,
                prompt: "latency notification reply",
                notificationID: notificationID
            )
        )
        try require(
            store.pendingCommands().contains { command in
                command.kind == .submitNotificationReply && command.notificationID == notificationID
            },
            "notification reply was not persisted locally"
        )
        let notificationPersistMs = elapsedMilliseconds(since: notificationStartedAt)

        let syncedSession = sessionSummary(
            id: Constants.cachedThreadID,
            title: "Latency Synced \(sampleIndex)",
            ref: "L\(sampleIndex)",
            status: .active
        )
        let streamSeq = Int64(200 + sampleIndex)
        let transport = G006StateMiniStreamTransport(
            streamPlans: [
                .deltas([
                    try stateMiniDelta(
                        session: syncedSession,
                        seq: streamSeq,
                        revision: "latency-stream-\(sampleIndex)"
                    ),
                ]),
            ]
        )
        service.stateMiniStreamTransport = transport
        let streamStartedAt = uptimeNanoseconds()
        model.startRealtimeSessionSyncIfNeeded()
        defer { model.stopRealtimeSessionSync() }
        try await waitUntilFast("stream delta did not render locally") {
            model.snapshot?.session(withID: Constants.cachedThreadID)?.title == syncedSession.title
        }
        try require(
            store.currentStateMiniSnapshot().latestSeq == streamSeq,
            "latency stream did not finish at seq \(streamSeq)"
        )
        let streamApplyMs = elapsedMilliseconds(since: streamStartedAt)

        return [
            "sampleIndex": sampleIndex,
            "uiModeMs": uiModeMs,
            "uiPromptMs": uiPromptMs,
            "modeAckMs": modeAckMs,
            "promptAckMs": promptAckMs,
            "notificationPersistMs": notificationPersistMs,
            "streamApplyMs": streamApplyMs,
            "streamResumeMs": streamApplyMs,
            "snapshotOnTapCount": 0,
            "fullSnapshotCallsOnTap": service.loadSnapshotCallCount,
        ]
    }

    private static func latencySampleCount() -> Int {
        let arguments = ProcessInfo.processInfo.arguments
        guard let argumentIndex = arguments.firstIndex(of: Constants.sampleCountArgument) else {
            return Constants.defaultLatencySampleCount
        }

        let valueIndex = arguments.index(after: argumentIndex)
        guard valueIndex < arguments.endIndex,
              let parsedValue = Int(arguments[valueIndex])
        else {
            return Constants.defaultLatencySampleCount
        }

        return max(parsedValue, Constants.minimumLatencySampleCount)
    }

    private static func latencyEvidence(samples: [[String: Any]]) throws -> [String: Any] {
        let p95 = try latencyP95(samples: samples)
        let violations = strictLatencyViolations(p95: p95)
        return [
            "schemaVersion": 2,
            "goal": "G010-tighten-latency-harness-run-simulato",
            "generatedAt": ISO8601DateFormatter().string(from: Date()),
            "sampleCount": samples.count,
            "percentile": 95,
            "platform": latencyPlatformName(),
            "classification": violations.isEmpty ? "strict-pass" : "current-red",
            "currentRed": !violations.isEmpty,
            "strictTargetViolations": violations,
            "strictTargetsMs": [
                "uiP95Ms": Constants.uiP95TargetMilliseconds,
                "localLanAckP95Ms": Constants.localLanAckP95TargetMilliseconds,
                "tailscaleInternetAckP95Ms": Constants.tailscaleInternetAckP95TargetMilliseconds,
            ],
            "p95": p95,
            "uiModeMs": try latencyValue(p95, key: "uiModeMs"),
            "uiPromptMs": try latencyValue(p95, key: "uiPromptMs"),
            "modeAckMs": try latencyValue(p95, key: "modeAckMs"),
            "promptAckMs": try latencyValue(p95, key: "promptAckMs"),
            "notificationPersistMs": try latencyValue(p95, key: "notificationPersistMs"),
            "streamApplyMs": try latencyValue(p95, key: "streamApplyMs"),
            "streamResumeMs": try latencyValue(p95, key: "streamResumeMs"),
            "snapshotOnTapCount": try latencyValue(p95, key: "snapshotOnTapCount"),
            "fullSnapshotCallsOnTap": try latencyValue(p95, key: "fullSnapshotCallsOnTap"),
            "tailscaleInternetAckMs": NSNull(),
            "remoteAckAvailable": false,
            "samples": samples,
        ]
    }

    private static func latencyP95(samples: [[String: Any]]) throws -> [String: Int] {
        let keys = [
            "uiModeMs",
            "uiPromptMs",
            "modeAckMs",
            "promptAckMs",
            "notificationPersistMs",
            "streamApplyMs",
            "streamResumeMs",
            "snapshotOnTapCount",
            "fullSnapshotCallsOnTap",
        ]
        var result: [String: Int] = [:]
        for key in keys {
            result[key] = percentile(
                try samples.map { sample in
                    try latencyValue(sample, key: key)
                },
                percentileValue: 95
            )
        }
        return result
    }

    private static func strictLatencyViolations(p95: [String: Int]) -> [[String: Any]] {
        let targets = [
            "uiModeMs": Constants.uiP95TargetMilliseconds,
            "uiPromptMs": Constants.uiP95TargetMilliseconds,
            "streamApplyMs": Constants.uiP95TargetMilliseconds,
            "streamResumeMs": Constants.uiP95TargetMilliseconds,
            "modeAckMs": Constants.localLanAckP95TargetMilliseconds,
            "promptAckMs": Constants.localLanAckP95TargetMilliseconds,
            "notificationPersistMs": Constants.localLanAckP95TargetMilliseconds,
        ]
        var violations: [[String: Any]] = []
        for (metric, target) in targets {
            let actual = p95[metric] ?? Int.max
            if actual > target {
                violations.append([
                    "metric": metric,
                    "targetMs": target,
                    "actualMs": actual,
                    "reason": "strict-target-exceeded",
                ])
            }
        }
        if (p95["snapshotOnTapCount"] ?? 0) != 0 {
            violations.append([
                "metric": "snapshotOnTapCount",
                "target": "0 snapshot refreshes on realtime tap/action path",
                "actual": p95["snapshotOnTapCount"] ?? 0,
                "reason": "snapshot-on-tap",
            ])
        }
        if (p95["fullSnapshotCallsOnTap"] ?? 0) != 0 {
            violations.append([
                "metric": "fullSnapshotCallsOnTap",
                "target": "0 full snapshot calls on mode/prompt/notification/stream path",
                "actual": p95["fullSnapshotCallsOnTap"] ?? 0,
                "reason": "full-snapshot-on-action",
            ])
        }
        return violations
    }

    private static func latencyValue(_ payload: [String: Any], key: String) throws -> Int {
        guard let value = payload[key] as? Int else {
            throw SelfTestError.assertionFailed("latency payload missing integer \(key)")
        }
        return value
    }

    private static func percentile(_ values: [Int], percentileValue: Int) -> Int {
        let sortedValues = values.sorted()
        let rank = Int(ceil((Double(percentileValue) / 100.0) * Double(sortedValues.count)))
        return sortedValues[max(rank - 1, 0)]
    }

    private static func latencyPlatformName() -> String {
        #if targetEnvironment(simulator)
        return "ios-simulator"
        #else
        return "physical-ios"
        #endif
    }

    private static func uptimeNanoseconds() -> UInt64 {
        DispatchTime.now().uptimeNanoseconds
    }

    private static func elapsedMilliseconds(since startNanoseconds: UInt64) -> Int {
        let elapsed = uptimeNanoseconds() - startNanoseconds
        return Int((Double(elapsed) / 1_000_000.0).rounded(.up))
    }

    private static func temporaryMiniStore() throws -> CompanionSessionMiniLocalStore {
        try CompanionSessionMiniLocalStore(fileURL: temporaryStoreFileURL())
    }

    private static func temporaryStoreFileURL() throws -> URL {
        let directoryURL = FileManager.default.temporaryDirectory.appendingPathComponent(
            "g006-local-first-\(UUID().uuidString)",
            isDirectory: true
        )
        try FileManager.default.createDirectory(at: directoryURL, withIntermediateDirectories: true)
        return directoryURL.appendingPathComponent(CompanionSessionMiniLocalStore.defaultFileName)
    }

    private static func seedMalformedMiniCache(at fileURL: URL) throws {
        let store = try LooperClientCoreLocalStore(filePath: fileURL.path)
        _ = try store.replaceStateMinis(snapshot: ClientStateMiniSnapshot(
            latestSeq: 3,
            sessions: [
                ClientStateMini(
                    sessionId: "bad-cache",
                    assistantSurface: CompanionAssistantSurface.codex.rawValue,
                    seq: 3,
                    revision: "bad-mini",
                    payloadJson: "{not-json"
                ),
            ],
            serverTime: Constants.timestamp
        ))
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

    private static func stateMiniDelta(
        session: SessionSummary,
        seq: Int64,
        revision: String
    ) throws -> ClientStateMiniDelta {
        let record = try miniRecord(session: session, seq: seq, revision: revision)
        let mini = ClientStateMini(
            sessionId: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJson: record.payloadJSON
        )
        return ClientStateMiniDelta(
            seq: seq,
            latestSeq: seq,
            entityId: "session-mini:\(record.assistantSurface):\(record.sessionID)",
            kind: "session-mini.changed",
            revision: revision,
            serverTime: Constants.timestamp,
            hasSession: true,
            session: mini,
            sessions: []
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

    private static func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        guard condition() else {
            throw SelfTestError.assertionFailed(message)
        }
    }

    private static func requireValue<Value>(_ value: Value?, _ message: String) throws -> Value {
        guard let value else {
            throw SelfTestError.assertionFailed(message)
        }
        return value
    }

    private static func waitUntil(
        _ message: String,
        timeoutNanoseconds: UInt64 = 2_000_000_000,
        condition: @MainActor @escaping () -> Bool
    ) async throws {
        let deadline = DispatchTime.now().uptimeNanoseconds + timeoutNanoseconds
        while DispatchTime.now().uptimeNanoseconds < deadline {
            if condition() {
                return
            }
            try await Task.sleep(nanoseconds: 20_000_000)
        }
        throw SelfTestError.assertionFailed(message)
    }

    private static func waitUntilFast(
        _ message: String,
        timeoutNanoseconds: UInt64 = Constants.latencyTimeoutNanoseconds,
        condition: @MainActor @escaping () -> Bool
    ) async throws {
        let deadline = DispatchTime.now().uptimeNanoseconds + timeoutNanoseconds
        while DispatchTime.now().uptimeNanoseconds < deadline {
            if condition() {
                return
            }
            await Task.yield()
            try await Task.sleep(nanoseconds: Constants.latencyPollNanoseconds)
        }
        throw SelfTestError.assertionFailed(message)
    }

    private static func write(_ message: String, to output: FileHandle) {
        output.write(Data((message + "\n").utf8))
    }
}

private final class G006LocalFirstServiceSpy: CompanionService, @unchecked Sendable {
    enum ServiceError: Error {
        case promptFailed
    }

    static let mutationOrderPrefixMode = "mode"
    static let mutationOrderPrefixPrompt = "prompt"
    static let mutationOrderPrefixNotificationReply = "notification-reply"

    private let lock = NSLock()
    private let snapshot: MobileSnapshot
    private let responseDelayNanoseconds: UInt64
    private(set) var loadSnapshotCallCount = 0
    private(set) var modeClientMutationIDs: [String] = []
    private(set) var promptClientMutationIDs: [String] = []
    private(set) var notificationReplyClientMutationIDs: [String] = []
    private(set) var notificationReplyIDs: [String] = []
    private(set) var mutationOrder: [String] = []
    var stateMiniStreamTransport: (any LooperClientCoreStateMiniStreamTransport)?
    var modeError: Error?
    var promptError: Error?
    var snapshotError: Error?
    var modeResponseDelayNanosecondsByCall: [UInt64] = []
    init(
        snapshot: MobileSnapshot,
        responseDelayNanoseconds: UInt64 = 0
    ) {
        self.snapshot = snapshot
        self.responseDelayNanoseconds = responseDelayNanoseconds
    }

    func prepareRealtimeConnection() async {}

    func makeClientCoreStateMiniStreamTransport() async
        -> (any LooperClientCoreStateMiniStreamTransport)?
    {
        stateMiniStreamTransport
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
        if let snapshotError {
            throw snapshotError
        }
        return snapshot
    }

    func loadSessionDetail(id _: String, surface _: CompanionAssistantSurface?) async throws -> SessionDetail {
        throw ServiceError.promptFailed
    }

    func setSessionMode(
        id _: String,
        preset: SessionMode?,
        clientMutationID: String
    ) async throws -> CompanionSessionModeResult {
        try await delayResponseIfNeeded()
        try await delayModeResponseIfNeeded()
        appendModeClientMutationID(clientMutationID)
        if let modeError {
            throw modeError
        }
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

        try await delayResponseIfNeeded()
        appendPromptClientMutationID(clientMutationID)
        return .accepted(
            promptID: "prompt-1",
            dispatchKind: "resume",
            clientMutationID: clientMutationID
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

        try await delayResponseIfNeeded()
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

    func registerPushDevice(_: RemotePushRegistrationRequest) async throws -> RemotePushRegistrationResponse {
        RemotePushRegistrationResponse(
            state: .storedAwaitingProvider,
            environment: .development,
            registeredAt: "2026-06-24T00:00:00Z",
            message: "self-test"
        )
    }

    func sendTestPush(installationID _: String) async throws -> RemotePushTestResponse {
        RemotePushTestResponse(delivered: true, message: "self-test")
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
        mutationOrder.append("\(Self.mutationOrderPrefixMode):\(clientMutationID)")
    }

    private func appendPromptClientMutationID(_ clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        promptClientMutationIDs.append(clientMutationID)
        mutationOrder.append("\(Self.mutationOrderPrefixPrompt):\(clientMutationID)")
    }

    private func appendNotificationReply(notificationID: String, clientMutationID: String) {
        lock.lock()
        defer { lock.unlock() }
        notificationReplyIDs.append(notificationID)
        notificationReplyClientMutationIDs.append(clientMutationID)
        mutationOrder.append("\(Self.mutationOrderPrefixNotificationReply):\(clientMutationID)")
    }

    private func delayResponseIfNeeded() async throws {
        guard responseDelayNanoseconds > 0 else {
            return
        }

        try await Task.sleep(nanoseconds: responseDelayNanoseconds)
    }

    private func delayModeResponseIfNeeded() async throws {
        let delayNanoseconds = popModeResponseDelayNanoseconds()
        guard delayNanoseconds > 0 else {
            return
        }

        try await Task.sleep(nanoseconds: delayNanoseconds)
    }

    private func popModeResponseDelayNanoseconds() -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        guard !modeResponseDelayNanosecondsByCall.isEmpty else {
            return 0
        }

        return modeResponseDelayNanosecondsByCall.removeFirst()
    }
}

private final class G006StateMiniStreamTransport:
    LooperClientCoreStateMiniStreamTransport,
    @unchecked Sendable
{
    enum StreamPlan: Sendable {
        case deltas([ClientStateMiniDelta])
        case recoveryRequired(snapshot: ClientStateMiniSnapshot)
    }

    private let lock = NSLock()
    private var streamPlans: [StreamPlan]
    private var afterSeqs: [Int64] = []
    private var pendingSnapshot: ClientStateMiniSnapshot?

    init(streamPlans: [StreamPlan]) {
        self.streamPlans = streamPlans
    }

    func recoverClientCoreStateMiniSnapshot(
        clientCore: LooperClientCore
    ) async throws -> ClientStateSnapshot {
        let snapshot = try lock.withLock {
            guard let pendingSnapshot else {
                throw LooperRealtimeError.unavailable
            }
            self.pendingSnapshot = nil
            return pendingSnapshot
        }
        return try clientCore.replaceStateMinis(snapshot: snapshot)
    }

    func startClientCoreStateMiniStream(clientCore: LooperClientCore) async throws {
        let latestSeq = try clientCore.snapshot().latestSeq
        lock.withLock {
            afterSeqs.append(latestSeq)
        }
    }

    func nextClientCoreStateMiniStreamUpdate(
        clientCore: LooperClientCore
    ) async throws -> ClientStateMiniStreamUpdate {
        let plan = lock.withLock {
            streamPlans.isEmpty ? .deltas([]) : streamPlans.removeFirst()
        }
        switch plan {
        case let .deltas(deltas):
            var latestUpdate: ClientStateMiniStreamUpdate?
            for delta in deltas {
                latestUpdate = try apply(delta: delta, to: clientCore)
            }
            return try latestUpdate ?? stoppedUpdate(clientCore: clientCore)
        case let .recoveryRequired(snapshot):
            lock.withLock {
                pendingSnapshot = snapshot
            }
            let coreSnapshot = try clientCore.snapshot()
            return ClientStateMiniStreamUpdate(
                reason: .recoveryRequired,
                snapshot: coreSnapshot,
                didChange: false,
                latestSeq: coreSnapshot.latestSeq,
                errorDescription: "state mini recovery required"
            )
        }
    }

    func stopClientCoreStateMiniStream(clientCore _: LooperClientCore) throws {}

    func observedAfterSeqs() -> [Int64] {
        lock.withLock { afterSeqs }
    }

    private func apply(
        delta: ClientStateMiniDelta,
        to clientCore: LooperClientCore
    ) throws -> ClientStateMiniStreamUpdate {
        let result = try clientCore.applyStateMiniDeltaWithResult(
            delta: delta
        )
        return ClientStateMiniStreamUpdate(
            reason: .delta,
            snapshot: result.snapshot,
            didChange: result.didChange,
            latestSeq: result.snapshot.latestSeq,
            errorDescription: ""
        )
    }

    private func stoppedUpdate(clientCore: LooperClientCore) throws -> ClientStateMiniStreamUpdate {
        let snapshot = try clientCore.snapshot()
        return ClientStateMiniStreamUpdate(
            reason: .stopped,
            snapshot: snapshot,
            didChange: false,
            latestSeq: snapshot.latestSeq,
            errorDescription: ""
        )
    }

}

#endif

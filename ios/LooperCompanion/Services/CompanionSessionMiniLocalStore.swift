import Foundation
import LooperClientCore

private enum CompanionSessionMiniSyncReason {
    static let delta = "delta"
    static let heartbeat = "heartbeat"
}

struct CompanionSessionMiniPendingCommand: Equatable, Sendable {
    let kind: ClientPendingCommandKind
    let clientMutationID: String
    let threadID: String
    let assistantSurface: String?
    let notificationID: String?
    let prompt: String?
    let attemptCount: Int
}

struct CompanionClientCoreMobileSnapshotStreamResult: Sendable {
    let update: CompanionSessionMiniSyncUpdate?
    let liveness: CompanionSessionMiniLivenessUpdate?
    let shouldStop: Bool
    let debugMessage: String
}

struct CompanionSessionMiniSyncUpdate: Sendable {
    let reason: String
    let latestSeq: Int64
    let endpointURL: URL?
    let snapshot: MobileSnapshot
}

struct CompanionSessionMiniLivenessUpdate: Sendable {
    let reason: String
    let latestSeq: Int64
    let serverTime: String
    let isLive: Bool
    let endpointURL: URL?
}

typealias CompanionSessionMiniSyncUpdateHandler = @MainActor @Sendable (
    CompanionSessionMiniSyncUpdate
) -> Void

typealias CompanionSessionMiniLivenessUpdateHandler = @MainActor @Sendable (
    CompanionSessionMiniLivenessUpdate
) -> Void

typealias CompanionSessionMiniSyncDebugHandler = @MainActor @Sendable (String) -> Void

struct CompanionSessionRuntimeStartConfiguration: Sendable {
    typealias EndpointResolver = @Sendable () async throws -> [URL]

    let bearerToken: String?
    let endpointResolver: EndpointResolver
}

private final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    fileprivate unowned let sessionManager: LooperClientCoreSessionManager
    private let decoder = JSONDecoder()

    fileprivate init(sessionManager: LooperClientCoreSessionManager) {
        self.sessionManager = sessionManager
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        let localSnapshot = try currentStateMiniSnapshot()
        return try mobileSnapshot(
            latestSeq: localSnapshot.latestSeq,
            sessions: localSnapshot.sessions,
            serverTime: localSnapshot.serverTime
        )
    }

    func pendingCommands() -> [CompanionSessionMiniPendingCommand] {
        (try? sessionManager.localSnapshot().pendingCommands.map(CompanionSessionMiniPendingCommand.init)) ?? []
    }

    fileprivate static func defaultFileURL() throws -> URL {
        try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        .appendingPathComponent(defaultFileName)
    }

    private func mobileSnapshot(
        latestSeq: Int64,
        sessions minis: [ClientStateMini],
        serverTime: String?
    ) throws -> MobileSnapshot? {
        let projection = try reduceStateMinisMobileSnapshot(
            latestSeq: latestSeq,
            sessions: minis,
            serverTime: serverTime ?? ""
        )
        guard projection.hasSnapshot else {
            return nil
        }

        return try decoder.decode(MobileSnapshot.self, from: Data(projection.snapshotJson.utf8))
    }

    fileprivate func mobileSnapshot(from snapshot: ClientLocalStateSnapshot) throws -> MobileSnapshot? {
        try mobileSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            serverTime: snapshot.serverTime
        )
    }

}

final class CompanionSessionRuntime: @unchecked Sendable {
    static let defaultFileName = CompanionSessionMiniLocalStore.defaultFileName

    private let localStore: CompanionSessionMiniLocalStore
    private let decoder = JSONDecoder()
    private let sessionManager: LooperClientCoreSessionManager
    private let startConfigurationLock = NSLock()
    private var startConfiguration: CompanionSessionRuntimeStartConfiguration?

    init(fileURL: URL) throws {
        let sessionManager = try LooperClientCoreSessionManager(fileURL: fileURL)
        self.sessionManager = sessionManager
        self.localStore = CompanionSessionMiniLocalStore(sessionManager: sessionManager)
    }

    static func liveDefault() -> CompanionSessionRuntime? {
        do {
            return try CompanionSessionRuntime(fileURL: CompanionSessionMiniLocalStore.defaultFileURL())
        } catch {
            CompanionDiagnostics.record("session-runtime:unavailable error=\(error.localizedDescription)")
            return nil
        }
    }

    func configureStart(_ configuration: CompanionSessionRuntimeStartConfiguration) {
        startConfigurationLock.lock()
        startConfiguration = configuration
        startConfigurationLock.unlock()
    }

    @discardableResult
    private func start(
        endpoints: [ClientEndpoint],
        bearerToken: String?,
        mobileSessionHeader: String?
    ) throws -> ClientStateSnapshot {
        try sessionManager.start(
            endpoints: endpoints,
            bearerToken: bearerToken ?? "",
            mobileSessionHeader: mobileSessionHeader ?? ""
        )
    }

    @discardableResult
    func startIfNeeded(
        bearerToken: String?,
        mobileSessionHeader: String?,
        preferredRealtimeEndpointURLs: () async throws -> [URL]
    ) async throws -> ClientStateSnapshot? {
        let endpoints = try await preferredRealtimeEndpointURLs().map {
            ClientEndpoint(url: $0.absoluteString, lastGood: false)
        }
        guard !endpoints.isEmpty else {
            throw CompanionSessionRuntimeError.noRealtimeEndpoint
        }
        return try start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    func prepareSessionRuntime() async {
        do {
            try await startSessionRuntime()
            CompanionDiagnostics.record("session-runtime:warm-success")
        } catch {
            CompanionDiagnostics.record("session-runtime:warm-failed error=\(error.localizedDescription)")
        }
    }

    @discardableResult
    func stop() throws -> ClientStateSnapshot {
        try sessionManager.stop()
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        try localStore.cachedSnapshot()
    }

    func currentStateMiniSnapshot() throws -> ClientLocalStateSnapshot {
        try localStore.currentStateMiniSnapshot()
    }

    func recoverStateMiniSnapshot() async throws -> MobileSnapshot? {
        guard let startConfiguration = currentStartConfiguration() else {
            throw CompanionSessionRuntimeError.notConfigured
        }
        let endpoints = try await startConfiguration.endpointResolver().map {
            ClientEndpoint(url: $0.absoluteString, lastGood: false)
        }
        guard !endpoints.isEmpty else {
            throw CompanionSessionRuntimeError.noRealtimeEndpoint
        }
        let localSnapshot = try await sessionManager.recoverStateMiniSnapshot(
            endpoints: endpoints,
            bearerToken: startConfiguration.bearerToken ?? "",
            mobileSessionHeader: CompanionMobileSessionStore.loadValidHeaderValue() ?? ""
        )
        return try localStore.mobileSnapshot(from: localSnapshot)
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) throws -> String {
        let queued = try sessionManager.persistNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
        return queued.clientMutationId
    }

    func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) throws {
        _ = try sessionManager.persistNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
    }

    func pendingCommands() -> [CompanionSessionMiniPendingCommand] {
        localStore.pendingCommands()
    }

    func nextMobileSnapshotStreamResult()
        async throws -> CompanionClientCoreMobileSnapshotStreamResult
    {
        let streamUpdate = try await sessionManager.observeMobileSnapshotChange()
        let endpointURL = Self.endpointURL(from: try? sessionManager.stateSnapshot())
        let livenessUpdate = Self.livenessUpdate(
            from: streamUpdate,
            endpointURL: endpointURL
        )
        guard streamUpdate.hasSnapshot else {
            return CompanionClientCoreMobileSnapshotStreamResult(
                update: nil,
                liveness: livenessUpdate,
                shouldStop: streamUpdate.shouldStop,
                debugMessage: streamUpdate.debugMessage
            )
        }

        let snapshotData = Data(streamUpdate.snapshotJson.utf8)
        let snapshot: MobileSnapshot
        do {
            snapshot = try decoder.decode(MobileSnapshot.self, from: snapshotData)
        } catch {
            let debugMessage = Self.joinDebugMessages(
                streamUpdate.debugMessage,
                "session-mini:mobile-snapshot-decode-failed error=\(error.localizedDescription)"
            )
            CompanionDiagnostics.record(debugMessage)
            return CompanionClientCoreMobileSnapshotStreamResult(
                update: nil,
                liveness: livenessUpdate,
                shouldStop: streamUpdate.shouldStop,
                debugMessage: debugMessage
            )
        }
        return CompanionClientCoreMobileSnapshotStreamResult(
            update: CompanionSessionMiniSyncUpdate(
                reason: streamUpdate.syncReason,
                latestSeq: streamUpdate.latestSeq,
                endpointURL: endpointURL,
                snapshot: snapshot
            ),
            liveness: livenessUpdate,
            shouldStop: streamUpdate.shouldStop,
            debugMessage: streamUpdate.debugMessage
        )
    }

    func runStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onLiveness: @escaping CompanionSessionMiniLivenessUpdateHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async {
        defer {
            stopStateMiniStream()
        }

        do {
            try await drainStateMiniSync(
                onUpdate: onUpdate,
                onLiveness: onLiveness,
                onDebugMessage: onDebugMessage
            )
        } catch {
            await onDebugMessage(
                "session-mini:client-core-stream-failed error=\(error.localizedDescription)"
            )
        }
    }

    func stopStateMiniStream() {
        do {
            _ = try stop()
        } catch {
            CompanionDiagnostics.record(
                "session-mini:client-core-stream-stop-failed error=\(error.localizedDescription)"
            )
        }
    }

    @discardableResult
    func setMode(
        threadID: String,
        preset: SessionMode?
    ) async throws -> ClientSessionModeIntentResult {
        let result = try await sessionManager.setMode(
            threadID: threadID,
            preset: preset?.rawValue ?? ""
        )
        guard result.accepted else {
            CompanionDiagnostics.record("mode:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("mode:grpc-accepted id=\(threadID)")
        return result
    }

    @discardableResult
    func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        promptIntent: CompanionPromptIntent = .steer
    ) async throws -> ClientSessionPromptIntentResult {
        let result = try await sessionManager.sendPrompt(
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            promptIntent: promptIntent.rawValue
        )
        guard result.accepted else {
            CompanionDiagnostics.record("prompt:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record(
            "prompt:grpc-accepted id=\(threadID) kind=\(Self.dispatchKind(from: result.dispatchKind))"
        )
        return result
    }

    @discardableResult
    func setAssistantSurface(
        _ assistantSurface: CompanionAssistantSurface
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.setAssistantSurface(assistantSurface.rawValue)
        guard result.accepted else {
            CompanionDiagnostics.record("assistant-surface:grpc-invalid surface=\(assistantSurface.rawValue)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("assistant-surface:grpc-accepted surface=\(assistantSurface.rawValue)")
        return result
    }

    @discardableResult
    func setSiriCurrentSession(
        threadID: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.setSiriCurrentSession(
            threadID: threadID,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
        guard result.accepted else {
            CompanionDiagnostics.record("siri-current:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("siri-current:grpc-accepted id=\(threadID)")
        return result
    }

    @discardableResult
    func setSiriDefaultSession(
        threadID: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.setSiriDefaultSession(
            threadID: threadID,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
        guard result.accepted else {
            CompanionDiagnostics.record("siri-default:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("siri-default:grpc-accepted id=\(threadID)")
        return result
    }

    @discardableResult
    func saveDefaultPrompt(
        _ prompt: String
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.saveDefaultPrompt(prompt)
        guard result.accepted else {
            CompanionDiagnostics.record("default-prompt:grpc-invalid")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("default-prompt:grpc-accepted")
        return result
    }

    @discardableResult
    func setSessionArchived(
        threadID: String,
        archived: Bool
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.setSessionArchived(
            threadID: threadID,
            archived: archived
        )
        guard result.accepted else {
            CompanionDiagnostics.record("archive:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("archive:grpc-accepted id=\(threadID) archived=\(archived)")
        return result
    }

    @discardableResult
    func deleteSession(
        threadID: String
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.deleteSession(threadID: threadID)
        guard result.accepted else {
            CompanionDiagnostics.record("delete:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("delete:grpc-accepted id=\(threadID)")
        return result
    }

    @discardableResult
    func muteSession(
        threadID: String
    ) async throws -> ClientSessionCommandIntentResult {
        let result = try await sessionManager.muteSession(threadID: threadID)
        guard result.accepted else {
            CompanionDiagnostics.record("mute:grpc-invalid id=\(threadID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("mute:grpc-accepted id=\(threadID)")
        return result
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult {
        let result = try await sessionManager.submitNotificationReplyWithGeneratedMutation(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? ""
        )
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: threadID
        )
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult {
        let result = try await sessionManager.submitNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface?.rawValue ?? "",
            clientMutationID: clientMutationID
        )
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: threadID
        )
    }

    @discardableResult
    func drainNotificationReplyOutbox() async throws -> ClientNotificationReplyIntentResult {
        try await sessionManager.drainNotificationReplyOutbox()
    }

    func outboxDepth() throws -> UInt32 {
        try sessionManager.outboxDepth()
    }

    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult {
        if let outboxDepth = try? outboxDepth(), outboxDepth == 0 {
            throw ClientCoreError.NoPendingNotificationReply
        }

        await prepareSessionRuntime()
        let result = try await drainNotificationReplyOutbox()
        let notificationID = result.notificationId
        let sessionID = result.entityId
        guard result.accepted else {
            CompanionDiagnostics.record(
                "notification-reply:grpc-pending-invalid id=\(sessionID) notificationID=\(notificationID)"
            )
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record(
            "notification-reply:grpc-pending-accepted id=\(sessionID) notificationID=\(notificationID) kind=\(Self.dispatchKind(from: result.dispatchKind))"
        )
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: sessionID
        )
    }

    private func drainStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onLiveness: @escaping CompanionSessionMiniLivenessUpdateHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async throws {
        while !Task.isCancelled {
            let result = try await nextMobileSnapshotStreamResult()
            if let liveness = result.liveness {
                await onLiveness(liveness)
            }
            if let update = result.update {
                await onUpdate(update)
            }
            if !result.debugMessage.isEmpty {
                await onDebugMessage(result.debugMessage)
            }
            if result.shouldStop {
                return
            }
        }
    }

    private func startSessionRuntime() async throws {
        guard let startConfiguration = currentStartConfiguration() else {
            throw CompanionSessionRuntimeError.notConfigured
        }
        _ = try await startIfNeeded(
            bearerToken: startConfiguration.bearerToken,
            mobileSessionHeader: CompanionMobileSessionStore.loadValidHeaderValue() ?? ""
        ) {
            try await startConfiguration.endpointResolver()
        }
    }

    private func currentStartConfiguration() -> CompanionSessionRuntimeStartConfiguration? {
        startConfigurationLock.lock()
        defer { startConfigurationLock.unlock() }
        return startConfiguration
    }

    private static func notificationReplyResponse(
        from result: ClientNotificationReplyIntentResult,
        fallbackNotificationID: String,
        sessionID: String
    ) throws -> ClientNotificationReplyIntentResult {
        guard result.accepted else {
            CompanionDiagnostics.record(
                "notification-reply:grpc-invalid id=\(sessionID) notificationID=\(fallbackNotificationID)"
            )
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record(
            "notification-reply:grpc-accepted id=\(sessionID) notificationID=\(fallbackNotificationID) kind=\(dispatchKind(from: result.dispatchKind))"
        )
        return result
    }

    private static func dispatchKind(from value: String) -> String {
        nonEmpty(value) ?? "accepted"
    }

    private static func nonEmpty(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func joinDebugMessages(_ lhs: String, _ rhs: String) -> String {
        guard let left = nonEmpty(lhs) else {
            return rhs
        }
        return "\(left); \(rhs)"
    }

    private static func livenessUpdate(
        from streamUpdate: ClientMobileSnapshotStreamUpdate,
        endpointURL: URL?
    ) -> CompanionSessionMiniLivenessUpdate? {
        return CompanionSessionMiniLivenessUpdate(
            reason: streamUpdate.syncReason,
            latestSeq: streamUpdate.latestSeq,
            serverTime: nonEmpty(streamUpdate.serverTime) ?? "",
            isLive: !streamUpdate.shouldStop && (
                streamUpdate.syncReason == CompanionSessionMiniSyncReason.delta ||
                    streamUpdate.syncReason == CompanionSessionMiniSyncReason.heartbeat
            ),
            endpointURL: endpointURL
        )
    }

    private static func endpointURL(from snapshot: ClientStateSnapshot?) -> URL? {
        guard let endpointURLString = nonEmpty(snapshot?.endpointUrl ?? "") else {
            return nil
        }

        return URL(string: endpointURLString)
    }
}

enum CompanionSessionRuntimeError: LocalizedError {
    case noRealtimeEndpoint
    case notConfigured

    var errorDescription: String? {
        switch self {
        case .noRealtimeEndpoint:
            "No realtime endpoint available"
        case .notConfigured:
            "Session runtime has no configured endpoints"
        }
    }
}

private extension CompanionSessionMiniPendingCommand {
    init(_ command: ClientPendingCommand) {
        self.init(
            kind: command.kind,
            clientMutationID: command.clientMutationId,
            threadID: command.threadId,
            assistantSurface: command.assistantSurface.nilIfEmpty,
            notificationID: command.notificationId.nilIfEmpty,
            prompt: command.prompt.nilIfEmpty,
            attemptCount: Int(command.attemptCount)
        )
    }
}

extension CompanionSessionMiniLocalStore {
    func currentStateMiniSnapshot() throws -> ClientLocalStateSnapshot {
        try sessionManager.localSnapshot()
    }
}

private extension String {
    var nilIfEmpty: String? {
        isEmpty ? nil : self
    }
}

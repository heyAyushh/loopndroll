import Foundation
import LooperClientCore
import LooperCompanionCore

enum CompanionSessionMiniSyncReason {
    static let delta = "delta"
    static let heartbeat = "heartbeat"
    static let textChunk = "text_chunk"
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
    /// Raw text chunk carried by this stream frame, independent of `update`:
    /// populated even when the full mobile snapshot can't be (or doesn't need
    /// to be) rebuilt, so the fast streaming-text path never waits on the
    /// snapshot projection pipeline.
    let textChunk: ClientTextChunk?
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

struct CompanionRecoveredSessionMiniSnapshot: Sendable {
    let snapshot: MobileSnapshot
    let latestSeq: Int64
}

typealias CompanionSessionMiniSyncUpdateHandler = @MainActor @Sendable (
    CompanionSessionMiniSyncUpdate
) -> Void

/// Fires for every raw text chunk on the stream, ahead of (and independent
/// of) `CompanionSessionMiniSyncUpdateHandler`: the fast path for feeding a
/// `StreamingReplyBuffer` without waiting on snapshot projection.
typealias CompanionSessionMiniTextChunkHandler = @MainActor @Sendable (
    ClientTextChunk
) -> Void

typealias CompanionSessionMiniLivenessUpdateHandler = @MainActor @Sendable (
    CompanionSessionMiniLivenessUpdate
) -> Void

typealias CompanionSessionMiniSyncDebugHandler = @MainActor @Sendable (String) -> Void

struct CompanionSessionRuntimeStartConfiguration: Sendable {
    typealias EndpointResolver = @Sendable () async throws -> [ClientEndpoint]

    let bearerToken: String?
    let endpointResolver: EndpointResolver
}

enum CompanionRealtimeEndpointResolver {
    static func endpoints(
        configuredBaseURLs: [URL],
        health: CompanionServerHealth? = nil
    ) -> [ClientEndpoint] {
        let recoveryBaseURLs = recoveryBaseURLs(
            configuredBaseURLs: configuredBaseURLs,
            health: health
        )
        let primaryRecoveryBaseURL = recoveryBaseURLs.first?.absoluteString ?? ""
        // Health-advertised URLs always include the server's loopback fallback, which
        // is only dialable from the simulator; run everything through the same
        // attemptability filter the HTTP layer uses.
        let h3Endpoints = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(uniqueURLs(
            [health?.grpcH3BaseURL].compactMap(\.self) + (health?.grpcH3BaseURLs ?? [])
        ))
        .map {
            ClientEndpoint.h3(
                url: $0.absoluteString,
                recoveryBaseURL: recoveryBaseURL(
                    for: $0,
                    candidates: recoveryBaseURLs,
                    fallback: primaryRecoveryBaseURL
                ),
                certificateSha256: health?.grpcH3CertificateSha256
            )
        }

        let h2EndpointURLs = CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(uniqueURLs(
            [health?.grpcBaseURL].compactMap(\.self) +
                (health?.grpcBaseURLs ?? []) +
                configuredBaseURLs
                    .map(CompanionBaseURLRouting.canonicalRealtimeGRPCBaseURL)
                    .map(\.absoluteString)
        ))
        let h2Endpoints = h2EndpointURLs.map {
            ClientEndpoint.h2(
                url: $0.absoluteString,
                recoveryBaseURL: CompanionBaseURLRouting
                    .canonicalHTTPAPIBaseURL(for: $0)
                    .absoluteString
            )
        }

        return h3Endpoints + h2Endpoints
    }

    private static func recoveryBaseURLs(
        configuredBaseURLs: [URL],
        health: CompanionServerHealth?
    ) -> [URL] {
        uniqueURLs(
            [health?.baseURL].compactMap(\.self) +
                (health?.baseURLs ?? []) +
                configuredBaseURLs.map(\.absoluteString)
        )
        .map(CompanionBaseURLRouting.canonicalHTTPAPIBaseURL)
    }

    private static func recoveryBaseURL(
        for endpointURL: URL,
        candidates: [URL],
        fallback: String
    ) -> String {
        let endpointHost = endpointURL.host?.lowercased()
        return candidates
            .first { $0.host?.lowercased() == endpointHost }?
            .absoluteString ?? fallback
    }

    private static func uniqueURLs(_ values: [String]) -> [URL] {
        var seen = Set<String>()
        return values.compactMap { value -> URL? in
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty,
                  let url = URL(string: trimmed),
                  seen.insert(url.absoluteString).inserted
            else {
                return nil
            }
            return url
        }
    }
}

private final class CompanionSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    fileprivate unowned let sessionManager: LooperClientCoreSessionManager

    fileprivate init(sessionManager: LooperClientCoreSessionManager) {
        self.sessionManager = sessionManager
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        let localSnapshot = try currentStateMiniSnapshot()
        return try mobileSnapshot(from: localSnapshot)
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
        pendingCommands: [ClientPendingCommand],
        serverTime: String?
    ) throws -> MobileSnapshot? {
        let projection = try reduceStateMinisMobileSnapshotWithPendingCommands(
            latestSeq: latestSeq,
            sessions: minis,
            pendingCommands: pendingCommands,
            serverTime: serverTime ?? ""
        )
        guard projection.hasSnapshot else {
            return nil
        }

        let snapshot = MobileSnapshot(clientCore: projection.snapshot)
        guard !Self.containsCorruptFallbackSession(snapshot) else {
            CompanionDiagnostics.record(
                "session-mini:cache-corrupt-skip sessions=\(snapshot.sessions.count)"
            )
            return nil
        }
        return snapshot
    }

    fileprivate func mobileSnapshot(from snapshot: ClientLocalStateSnapshot) throws -> MobileSnapshot? {
        try mobileSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            pendingCommands: snapshot.pendingCommands,
            serverTime: snapshot.serverTime
        )
    }

    private static func containsCorruptFallbackSession(_ snapshot: MobileSnapshot) -> Bool {
        snapshot.sessions.contains { session in
            session.ref == session.id &&
                session.title == session.id &&
                session.lastUpdatedAt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
                session.lastActivityAt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty &&
                !session.canSendPrompt
        }
    }

}
final class CompanionSessionRuntime: @unchecked Sendable {
    static let defaultFileName = CompanionSessionMiniLocalStore.defaultFileName

    private let localStore: CompanionSessionMiniLocalStore
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
        preferredRealtimeEndpoints: () async throws -> [ClientEndpoint]
    ) async throws -> ClientStateSnapshot? {
        let endpoints = try await preferredRealtimeEndpoints()
        guard !endpoints.isEmpty else {
            throw CompanionSessionRuntimeError.noRealtimeEndpoint
        }
        Self.recordEndpointCandidates(endpoints, reason: "start")
        let snapshot = try start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
        Self.recordRuntimeDiagnostics(snapshot, reason: "start")
        return snapshot
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

    func sessionDetail(sessionID: String) throws -> ClientSessionDetailProjection {
        try sessionManager.sessionDetail(sessionID: sessionID)
    }

    func recoverStateMiniSnapshot() async throws -> CompanionRecoveredSessionMiniSnapshot? {
        guard let startConfiguration = currentStartConfiguration() else {
            throw CompanionSessionRuntimeError.notConfigured
        }
        let endpoints = try await startConfiguration.endpointResolver()
        guard !endpoints.isEmpty else {
            throw CompanionSessionRuntimeError.noRealtimeEndpoint
        }
        let localSnapshot = try await sessionManager.recoverStateMiniSnapshot(
            endpoints: endpoints,
            bearerToken: startConfiguration.bearerToken ?? "",
            mobileSessionHeader: CompanionMobileSessionStore.loadValidHeaderValue() ?? ""
        )
        guard let snapshot = try localStore.mobileSnapshot(from: localSnapshot) else {
            return nil
        }
        return CompanionRecoveredSessionMiniSnapshot(
            snapshot: snapshot,
            latestSeq: localSnapshot.latestSeq
        )
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
        let stateSnapshot = try? sessionManager.stateSnapshot()
        Self.recordRuntimeDiagnostics(stateSnapshot, reason: streamUpdate.syncReason)
        let endpointURL = Self.endpointURL(from: stateSnapshot)
        return try mobileSnapshotStreamResult(
            from: streamUpdate,
            stateSnapshot: try? localStore.currentStateMiniSnapshot(),
            endpointURL: endpointURL
        )
    }

    func mobileSnapshotStreamResult(
        from streamUpdate: ClientMobileSnapshotStreamUpdate,
        stateSnapshot: ClientLocalStateSnapshot?,
        endpointURL: URL?
    ) throws -> CompanionClientCoreMobileSnapshotStreamResult {
        let livenessUpdate = Self.livenessUpdate(
            from: streamUpdate,
            endpointURL: endpointURL
        )
        let textChunk = streamUpdate.hasTextChunk ? streamUpdate.textChunk : nil
        guard streamUpdate.hasSnapshot else {
            if streamUpdate.hasTextChunk,
               streamUpdate.syncReason == CompanionSessionMiniSyncReason.textChunk,
               let stateSnapshot,
               let snapshot = try localStore.mobileSnapshot(from: stateSnapshot) {
                return CompanionClientCoreMobileSnapshotStreamResult(
                    update: CompanionSessionMiniSyncUpdate(
                        reason: streamUpdate.syncReason,
                        latestSeq: streamUpdate.latestSeq,
                        endpointURL: endpointURL,
                        snapshot: snapshot
                    ),
                    liveness: livenessUpdate,
                    textChunk: textChunk,
                    shouldStop: streamUpdate.shouldStop,
                    debugMessage: streamUpdate.debugMessage
                )
            }
            return CompanionClientCoreMobileSnapshotStreamResult(
                update: nil,
                liveness: livenessUpdate,
                textChunk: textChunk,
                shouldStop: streamUpdate.shouldStop,
                debugMessage: streamUpdate.debugMessage
            )
        }

        let snapshot = MobileSnapshot(clientCore: streamUpdate.snapshot)
        return CompanionClientCoreMobileSnapshotStreamResult(
            update: CompanionSessionMiniSyncUpdate(
                reason: streamUpdate.syncReason,
                latestSeq: streamUpdate.latestSeq,
                endpointURL: endpointURL,
                snapshot: snapshot
            ),
            liveness: livenessUpdate,
            textChunk: textChunk,
            shouldStop: streamUpdate.shouldStop,
            debugMessage: streamUpdate.debugMessage
        )
    }

    func runStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onLiveness: @escaping CompanionSessionMiniLivenessUpdateHandler,
        onTextChunk: @escaping CompanionSessionMiniTextChunkHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async {
        defer {
            stopStateMiniStream()
        }

        do {
            try await drainStateMiniSync(
                onUpdate: onUpdate,
                onLiveness: onLiveness,
                onTextChunk: onTextChunk,
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
        CompanionDiagnostics.record(
            "mode:grpc-accepted id=\(threadID) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "mode-accepted")
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
            "prompt:grpc-accepted id=\(threadID) kind=\(Self.dispatchKind(from: result.dispatchKind)) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "prompt-accepted")
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
        CompanionDiagnostics.record(
            "siri-current:grpc-accepted id=\(threadID) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "siri-current-accepted")
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
        CompanionDiagnostics.record(
            "siri-default:grpc-accepted id=\(threadID) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "siri-default-accepted")
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
        CompanionDiagnostics.record(
            "default-prompt:grpc-accepted clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "default-prompt-accepted")
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
        CompanionDiagnostics.record(
            "archive:grpc-accepted id=\(threadID) archived=\(archived) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "archive-accepted")
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
        CompanionDiagnostics.record(
            "delete:grpc-accepted id=\(threadID) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "delete-accepted")
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
        CompanionDiagnostics.record(
            "mute:grpc-accepted id=\(threadID) clientMutationID=\(result.clientMutationId)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "mute-accepted")
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
            "notification-reply:grpc-pending-accepted id=\(sessionID) notificationID=\(notificationID) kind=\(Self.dispatchKind(from: result.dispatchKind)) clientMutationID=\(result.clientMutationId) ackSeq=\(result.ackSeq)"
        )
        Self.recordRuntimeDiagnostics(try? sessionManager.stateSnapshot(), reason: "notification-reply-pending-accepted")
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: sessionID
        )
    }

    private func drainStateMiniSync(
        onUpdate: @escaping CompanionSessionMiniSyncUpdateHandler,
        onLiveness: @escaping CompanionSessionMiniLivenessUpdateHandler,
        onTextChunk: @escaping CompanionSessionMiniTextChunkHandler,
        onDebugMessage: @escaping CompanionSessionMiniSyncDebugHandler
    ) async throws {
        while !Task.isCancelled {
            let result = try await nextMobileSnapshotStreamResult()
            // Fires first and independent of the snapshot/update path below —
            // the streaming-text fast path must never wait on projection.
            if let textChunk = result.textChunk {
                await onTextChunk(textChunk)
            }
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
            "notification-reply:grpc-accepted id=\(sessionID) notificationID=\(fallbackNotificationID) kind=\(dispatchKind(from: result.dispatchKind)) clientMutationID=\(result.clientMutationId) ackSeq=\(result.ackSeq)"
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
                    streamUpdate.syncReason == CompanionSessionMiniSyncReason.heartbeat ||
                    streamUpdate.syncReason == CompanionSessionMiniSyncReason.textChunk
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

    /// Logs the candidates as Swift hands them to the core, BEFORE the core's
    /// `endpoints_with_last_good` reorder consults its persisted last-good
    /// endpoint — so `lastGood` here is always false and says nothing about
    /// persistence (that lives in the core's local store).
    private static func recordEndpointCandidates(_ endpoints: [ClientEndpoint], reason: String) {
        let summary = endpoints.enumerated().map { index, endpoint in
            "index=\(index) transport=\(transportName(endpoint.transport)) url=\(endpoint.url) recoveryBaseURL=\(endpoint.recoveryBaseUrl) h3CertSha256=\(nonEmpty(endpoint.h3CertificateSha256) ?? "none") h3SpkiSha256=\(nonEmpty(endpoint.h3CertificateSpkiSha256) ?? "none") lastGood=\(endpoint.lastGood)"
        }.joined(separator: " | ")
        CompanionDiagnostics.record("session-runtime:endpoint-candidates-pre-core reason=\(reason) \(summary)")
    }

    private static func recordRuntimeDiagnostics(_ snapshot: ClientStateSnapshot?, reason: String) {
        guard let snapshot else {
            CompanionDiagnostics.record("session-runtime:state-unavailable reason=\(reason)")
            return
        }
        let pending = snapshot.pendingMutations.map {
            "\($0.clientMutationId):\(commandKindName($0.commandKind)):\($0.threadId)"
        }.joined(separator: ",")
        let recentAcks = snapshot.recentCommandAcks.map {
            "\($0.clientMutationId):accepted=\($0.accepted):ackSeq=\($0.ackSeq):entity=\($0.entityId):revision=\($0.revision)"
        }.joined(separator: ",")
        CompanionDiagnostics.record(
            "session-runtime:state reason=\(reason) phase=\(snapshot.phase) selectedTransport=\(transportName(snapshot.endpointTransport)) endpointURL=\(snapshot.endpointUrl) fallbackReason=\(nonEmpty(snapshot.transportFallbackReason) ?? "none") latestSeq=\(snapshot.latestSeq) outboxDepth=\(snapshot.outboxDepth) pendingMutations=[\(pending)] recentCommandAcks=[\(recentAcks)] lastError=\(nonEmpty(snapshot.lastError) ?? "none")"
        )
    }

    private static func transportName(_ transport: ClientEndpointTransport) -> String {
        switch transport {
        case .h2:
            "h2"
        case .h3:
            "h3"
        }
    }

    private static func commandKindName(_ kind: ClientCommandKind) -> String {
        switch kind {
        case .setSessionMode:
            "SetSessionMode"
        case .sendSessionPrompt:
            "SendSessionPrompt"
        case .submitNotificationReply:
            "SubmitNotificationReply"
        case .setSiriCurrentSession:
            "SetSiriCurrentSession"
        case .setSiriDefaultSession:
            "SetSiriDefaultSession"
        case .saveDefaultPrompt:
            "SaveDefaultPrompt"
        case .setDefaultNotificationTargets:
            "SetDefaultNotificationTargets"
        case .setSessionArchived:
            "SetSessionArchived"
        case .deleteSession:
            "DeleteSession"
        case .muteSession:
            "MuteSession"
        }
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

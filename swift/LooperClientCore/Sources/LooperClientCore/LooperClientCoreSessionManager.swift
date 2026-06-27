import Foundation

public final class LooperClientCoreSessionManager: @unchecked Sendable {
    private let runtime: LooperClientCoreSessionRuntime

    public init(fileURL: URL) throws {
        self.runtime = try LooperClientCoreSessionRuntime(filePath: fileURL.path)
    }

    public init(filePath: String) throws {
        self.runtime = try LooperClientCoreSessionRuntime(filePath: filePath)
    }

    @discardableResult
    public func start(
        endpoints: [ClientEndpoint],
        bearerToken: String,
        mobileSessionHeader: String
    ) throws -> ClientStateSnapshot {
        try runtime.start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    @discardableResult
    public func stop() throws -> ClientStateSnapshot {
        try runtime.stop()
    }

    public func observe() async throws -> ClientStateMiniStreamUpdate {
        try await runtime.observe()
    }

    public func observeLocalStateChange() async throws -> ClientLocalStateStreamUpdate {
        try await runtime.observeLocalStateChange()
    }

    public func observeMobileSnapshotChange() async throws -> ClientMobileSnapshotStreamUpdate {
        try await runtime.observeMobileSnapshotChange()
    }

    public func observeMenuSnapshotChange() async throws -> ClientMenuSnapshotStreamUpdate {
        try await runtime.observeMenuSnapshotChange()
    }

    public func isRuntimeConfigured() throws -> Bool {
        let snapshot = try runtime.stateSnapshot()
        return !snapshot.endpointUrl.isEmpty && snapshot.phase != .disconnected
    }

    public func localSnapshot() throws -> ClientLocalStateSnapshot {
        try runtime.localSnapshot()
    }

    public func stateSnapshot() throws -> ClientStateSnapshot {
        try runtime.stateSnapshot()
    }

    @discardableResult
    public func setMode(
        threadID: String,
        preset: String
    ) async throws -> ClientSessionModeIntentResult {
        try await runtime.setMode(
            threadId: threadID,
            preset: preset
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String,
        promptIntent: String = "steer"
    ) async throws -> ClientSessionPromptIntentResult {
        try await runtime.sendPrompt(
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            promptIntent: promptIntent
        )
    }

    @discardableResult
    public func setAssistantSurface(
        _ assistantSurface: String
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.setAssistantSurface(assistantSurface: assistantSurface)
    }

    @discardableResult
    public func setSiriCurrentSession(
        threadID: String,
        assistantSurface: String
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.setSiriCurrentSession(
            threadId: threadID,
            assistantSurface: assistantSurface
        )
    }

    @discardableResult
    public func setSiriDefaultSession(
        threadID: String,
        assistantSurface: String
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.setSiriDefaultSession(
            threadId: threadID,
            assistantSurface: assistantSurface
        )
    }

    @discardableResult
    public func saveDefaultPrompt(
        _ prompt: String
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.saveDefaultPrompt(prompt: prompt)
    }

    @discardableResult
    public func setSessionArchived(
        threadID: String,
        archived: Bool
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.setSessionArchived(
            threadId: threadID,
            archived: archived
        )
    }

    @discardableResult
    public func deleteSession(
        threadID: String
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.deleteSession(threadId: threadID)
    }

    @discardableResult
    public func muteSession(
        threadID: String
    ) async throws -> ClientSessionCommandIntentResult {
        try await runtime.muteSession(threadId: threadID)
    }

    @discardableResult
    public func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult {
        try await runtime.submitNotificationReply(
            notificationId: notificationID,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationId: clientMutationID
        )
    }

    @discardableResult
    public func submitNotificationReplyWithGeneratedMutation(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String
    ) async throws -> ClientNotificationReplyIntentResult {
        try await runtime.submitNotificationReplyWithGeneratedMutation(
            notificationId: notificationID,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
    }

    @discardableResult
    public func drainNotificationReplyOutbox() async throws -> ClientNotificationReplyIntentResult {
        try await runtime.drainNotificationReplyOutbox()
    }

    @discardableResult
    public func persistNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String,
        clientMutationID: String
    ) throws -> ClientLocalStateSnapshot {
        try runtime.persistNotificationReply(
            notificationId: notificationID,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationId: clientMutationID
        )
    }

    @discardableResult
    public func persistNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String
    ) throws -> ClientNotificationReplyPersistResult {
        try runtime.persistNotificationReplyWithGeneratedMutation(
            notificationId: notificationID,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
    }

    public func outboxDepth() throws -> UInt32 {
        try runtime.outboxDepth()
    }
}

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
        try !runtime.stateSnapshot().endpointUrl.isEmpty
    }

    public func localSnapshot() throws -> ClientLocalStateSnapshot {
        try runtime.localSnapshot()
    }

    public func stateSnapshot() throws -> ClientStateSnapshot {
        try runtime.stateSnapshot()
    }

    @discardableResult
    public func replaceStateMinis(snapshot: ClientStateMiniSnapshot) throws -> ClientLocalStateSnapshot {
        try runtime.replaceStateMinis(snapshot: snapshot)
    }

    @discardableResult
    public func applyStateMiniDelta(_ delta: ClientStateMiniDelta) throws -> ClientLocalStateSnapshot {
        try runtime.applyStateMiniDelta(delta: delta)
    }

    @discardableResult
    public func setMode(
        threadID: String,
        preset: String,
        clientMutationID: String
    ) async throws -> ClientCommandAckEnvelope {
        try await runtime.setMode(
            threadId: threadID,
            preset: preset,
            clientMutationId: clientMutationID
        )
    }

    @discardableResult
    public func setMode(
        threadID: String,
        preset: String
    ) async throws -> ClientCommandAckEnvelope {
        try await runtime.setModeWithGeneratedMutation(
            threadId: threadID,
            preset: preset
        )
    }

    @discardableResult
    public func queueSetMode(
        threadID: String,
        preset: String,
        clientMutationID: String
    ) throws -> ClientLocalStateSnapshot {
        try runtime.queueSetMode(
            threadId: threadID,
            preset: preset,
            clientMutationId: clientMutationID
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String,
        clientMutationID: String
    ) async throws -> ClientCommandAckEnvelope {
        try await runtime.sendPrompt(
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationId: clientMutationID
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String
    ) async throws -> ClientCommandAckEnvelope {
        try await runtime.sendPromptWithGeneratedMutation(
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
    }

    @discardableResult
    public func submitNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String,
        clientMutationID: String
    ) async throws -> ClientCommandAckEnvelope {
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
    ) async throws -> ClientCommandAckEnvelope {
        try await runtime.submitNotificationReplyWithGeneratedMutation(
            notificationId: notificationID,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
    }

    @discardableResult
    public func drainNotificationReplyOutbox() async throws -> ClientCommandAckEnvelope {
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

    public func outboxDepth() throws -> UInt32 {
        try runtime.outboxDepth()
    }
}

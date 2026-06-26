import Foundation

public final class LooperClientCoreSessionManager: @unchecked Sendable {
    public let clientCore: LooperClientCore
    public let localStore: LooperClientCoreLocalStore

    public init(
        clientCore: LooperClientCore = LooperClientCore(),
        localStore: LooperClientCoreLocalStore
    ) {
        self.clientCore = clientCore
        self.localStore = localStore
    }

    @discardableResult
    public func configure(
        endpoints: [ClientEndpoint],
        bearerToken: String,
        mobileSessionHeader: String
    ) throws -> ClientStateSnapshot {
        try clientCore.configureSessionRuntime(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    @discardableResult
    public func start(
        endpoints: [ClientEndpoint],
        bearerToken: String,
        mobileSessionHeader: String
    ) throws -> ClientStateSnapshot {
        try clientCore.start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    @discardableResult
    public func startConfiguredStateMiniStream() throws -> ClientStateSnapshot {
        try clientCore.startConfiguredStateMiniStream()
    }

    public func observe() async throws -> ClientStateMiniStreamUpdate {
        try await clientCore.observe()
    }

    @discardableResult
    public func stop() throws -> ClientStateSnapshot {
        try clientCore.stop()
    }

    @discardableResult
    public func stopStateMiniStream() throws -> ClientStateSnapshot {
        try clientCore.stopStateMiniStream()
    }

    @discardableResult
    public func replaceStateMinis(snapshot: ClientStateMiniSnapshot) throws -> ClientStateSnapshot {
        try clientCore.replaceStateMinis(snapshot: snapshot)
    }

    @discardableResult
    public func setMode(
        threadID: String,
        preset: String,
        clientMutationID: String
    ) async throws -> ClientCommandAckEnvelope {
        try await clientCore.submitSetModeDurable(
            localStore: localStore,
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
        try await clientCore.submitSendPromptDurable(
            localStore: localStore,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationId: clientMutationID
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
        try await clientCore.submitNotificationReplyDurable(
            localStore: localStore,
            notificationId: notificationID,
            threadId: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationId: clientMutationID
        )
    }

    public func outboxDepth() throws -> UInt32 {
        try clientCore.snapshot().outboxDepth
    }
}

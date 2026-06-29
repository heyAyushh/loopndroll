import Foundation
import LooperClientCore

public struct MenuBarSessionMiniLocalSnapshot: Equatable, Sendable {
    public let latestSeq: Int64
    public let sessions: [MenuBarSessionMini]
    public let pendingCommands: [MenuBarSessionMiniPendingCommand]
    let clientCoreSnapshot: ClientMenuBarSessionMiniLocalSnapshot
}

public struct MenuBarSessionMiniPendingCommand: Equatable, Sendable {
    public let kind: ClientPendingCommandKind
    public let clientMutationID: String
    public let threadID: String
    public let notificationID: String?
    public let prompt: String?
    public let attemptCount: Int
}

public struct MenuBarClientCoreMenuSnapshotStreamResult: Sendable {
    public let snapshot: MenuBarSessionMiniLocalSnapshot?
    public let shouldStop: Bool
    public let debugMessage: String
}

public struct MenuBarSessionMini: Equatable, Sendable {
    private enum PendingText {
        static let singleNotificationReply = "Reply pending"
        static func notificationReplies(_ count: Int) -> String {
            "\(count) replies pending"
        }
    }

    public let sessionID: String
    public let assistantSurface: String
    public let seq: Int64
    public let revision: String
    public let ref: String
    public let title: String
    public let subtitle: String
    public let status: String
    public let effectiveMode: String?
    public let replyable: Bool
    public let promptUnavailableReason: String?
    public let blockedGoal: MenuBarSessionMiniBlockedGoal?
    public let queueCount: Int
    public let lifecycle: String?
    public let notificationStatus: MenuBarSessionMiniNotificationStatus?
    public let notificationTitle: String?
    public let isArchived: Bool
    public let assistantPreview: String?
    public let projectName: String?
    public let projectPath: String?
    public let lastActivityAtMs: Int64?
    public let updatedAtMs: Int64?

    fileprivate init(
        _ mini: ClientMenuBarSessionMini,
        pendingCommands: [MenuBarSessionMiniPendingCommand] = []
    ) {
        let pendingNotificationReplyCount = pendingCommands.pendingNotificationReplyCount(
            for: mini.sessionId
        )
        self.sessionID = mini.sessionId
        self.assistantSurface = mini.assistantSurface
        self.seq = mini.seq
        self.revision = mini.revision
        self.ref = mini.refId
        self.title = mini.title
        self.subtitle = Self.subtitle(
            mini.subtitle,
            pendingNotificationReplyCount: pendingNotificationReplyCount
        )
        self.status = mini.status
        self.effectiveMode = mini.hasEffectiveMode ? mini.effectiveMode : nil
        self.replyable = pendingNotificationReplyCount == 0 && mini.replyable
        self.promptUnavailableReason =
            mini.hasPromptUnavailableReason ? mini.promptUnavailableReason : nil
        self.blockedGoal = mini.hasBlockedGoal ? MenuBarSessionMiniBlockedGoal(mini.blockedGoal) : nil
        self.queueCount = Int(mini.queueCount)
        self.lifecycle = mini.hasLifecycle ? mini.lifecycle : nil
        self.notificationStatus = mini.hasNotificationStatus
            ? MenuBarSessionMiniNotificationStatus(mini.notificationStatus)
            : nil
        self.notificationTitle = Self.notificationTitle(
            mini.hasNotificationTitle ? mini.notificationTitle : nil,
            pendingNotificationReplyCount: pendingNotificationReplyCount
        )
        self.isArchived = mini.isArchived
        self.assistantPreview = mini.hasAssistantPreview ? mini.assistantPreview : nil
        self.projectName = mini.hasProjectName ? mini.projectName : nil
        self.projectPath = mini.hasProjectPath ? mini.projectPath : nil
        self.lastActivityAtMs = mini.hasLastActivityAtMs ? mini.lastActivityAtMs : nil
        self.updatedAtMs = mini.hasUpdatedAtMs ? mini.updatedAtMs : nil
    }

    private static func subtitle(
        _ subtitle: String,
        pendingNotificationReplyCount: Int
    ) -> String {
        let pendingTitle = pendingNotificationReplyTitle(count: pendingNotificationReplyCount)
        guard let pendingTitle else {
            return subtitle
        }
        guard !subtitle.contains(pendingTitle) else {
            return subtitle
        }
        guard !subtitle.isEmpty else {
            return pendingTitle
        }
        return "\(subtitle) - \(pendingTitle)"
    }

    private static func notificationTitle(
        _ notificationTitle: String?,
        pendingNotificationReplyCount: Int
    ) -> String? {
        pendingNotificationReplyTitle(count: pendingNotificationReplyCount) ?? notificationTitle
    }

    private static func pendingNotificationReplyTitle(count: Int) -> String? {
        guard count > 0 else {
            return nil
        }
        return count == 1
            ? PendingText.singleNotificationReply
            : PendingText.notificationReplies(count)
    }
}

public struct MenuBarSessionMiniBlockedGoal: Equatable, Sendable {
    public let id: String?
    public let title: String?
    public let status: String?
    public let lifecycle: String?
    public let reason: String?

    fileprivate init(_ goal: ClientMenuBarSessionMiniBlockedGoal) {
        self.id = goal.id.nilIfBlank
        self.title = goal.title.nilIfBlank
        self.status = goal.status.nilIfBlank
        self.lifecycle = goal.lifecycle.nilIfBlank
        self.reason = goal.reason.nilIfBlank
    }
}

public struct MenuBarSessionMiniNotificationStatus: Equatable, Sendable {
    public let enabled: Bool
    public let targetIds: [String]
    public let usesDefault: Bool

    fileprivate init(_ status: ClientMenuBarSessionMiniNotificationStatus) {
        self.enabled = status.enabled
        self.targetIds = status.targetIds
        self.usesDefault = status.usesDefault
    }
}

private final class MenuBarSessionMiniLocalStore: @unchecked Sendable {
    static let defaultFileName = "looper-realtime-state-minis.json"

    private enum Defaults {
        static let applicationSupportDirectoryName = "looper"
    }

    private let sessionManager: LooperClientCoreSessionManager

    init(fileURL: URL) throws {
        self.sessionManager = try LooperClientCoreSessionManager(fileURL: fileURL)
    }

    init(sessionManager: LooperClientCoreSessionManager) {
        self.sessionManager = sessionManager
    }

    func cachedSnapshot() throws -> MenuBarSessionMiniLocalSnapshot {
        try menuSnapshot(from: currentStateMiniSnapshot())
    }

    func pendingCommands() -> [MenuBarSessionMiniPendingCommand] {
        (try? sessionManager.localSnapshot().pendingCommands.map(MenuBarSessionMiniPendingCommand.init)) ?? []
    }

    static func defaultFileURL() throws -> URL {
        try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        .appendingPathComponent(Defaults.applicationSupportDirectoryName, isDirectory: true)
        .appendingPathComponent(defaultFileName)
    }

    private func menuSnapshot(from snapshot: ClientLocalStateSnapshot) throws
        -> MenuBarSessionMiniLocalSnapshot
    {
        let projection = try reduceStateMinisMenuSnapshot(snapshot: snapshot)
        return MenuBarSessionMiniLocalSnapshot(projection)
    }
}

public final class MenuBarSessionRuntime: @unchecked Sendable {
    public static let defaultFileName = MenuBarSessionMiniLocalStore.defaultFileName

    private let localStore: MenuBarSessionMiniLocalStore
    private let sessionManager: LooperClientCoreSessionManager

    public init(fileURL: URL) throws {
        let sessionManager = try LooperClientCoreSessionManager(fileURL: fileURL)
        self.sessionManager = sessionManager
        self.localStore = MenuBarSessionMiniLocalStore(sessionManager: sessionManager)
    }

    public static func liveDefault() -> MenuBarSessionRuntime? {
        do {
            return try MenuBarSessionRuntime(fileURL: MenuBarSessionMiniLocalStore.defaultFileURL())
        } catch {
            return nil
        }
    }

    public static func available(fileURL: URL) -> MenuBarSessionRuntime? {
        try? MenuBarSessionRuntime(fileURL: fileURL)
    }

    public func cachedSnapshot() throws -> MenuBarSessionMiniLocalSnapshot {
        try localStore.cachedSnapshot()
    }

    public func currentStateMiniSnapshot() throws -> ClientLocalStateSnapshot {
        try localStore.currentStateMiniSnapshot()
    }

    public func runtimeStateSnapshot() throws -> ClientStateSnapshot {
        try sessionManager.stateSnapshot()
    }

    public func pendingCommands() -> [MenuBarSessionMiniPendingCommand] {
        localStore.pendingCommands()
    }

    @discardableResult
    public func setDefaultNotificationTargets(
        _ targetIDs: [String]
    ) async throws -> ClientSessionCommandIntentResult {
        try await sessionManager.setDefaultNotificationTargets(targetIDs)
    }

    @discardableResult
    public func startIfNeeded(
        bearerToken: String = "",
        mobileSessionHeader: String = "",
        preferredRealtimeEndpointURLs: @MainActor () async throws -> [URL]
    ) async throws -> ClientStateSnapshot? {
        let endpoints = try await preferredRealtimeEndpointURLs().map {
            ClientEndpoint(url: $0.absoluteString, lastGood: false)
        }
        guard !endpoints.isEmpty else {
            throw MenuBarSessionRuntimeError.noRealtimeEndpoint
        }
        return try start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    @discardableResult
    private func start(
        endpoints: [ClientEndpoint],
        bearerToken: String,
        mobileSessionHeader: String
    ) throws -> ClientStateSnapshot {
        return try sessionManager.start(
            endpoints: endpoints,
            bearerToken: bearerToken,
            mobileSessionHeader: mobileSessionHeader
        )
    }

    public func stop() {
        _ = try? sessionManager.stop()
    }

    public func nextMenuSnapshotStreamResult() async throws
        -> MenuBarClientCoreMenuSnapshotStreamResult
    {
        let streamUpdate = try await sessionManager.observeMenuSnapshotChange()
        guard streamUpdate.hasSnapshot else {
            return MenuBarClientCoreMenuSnapshotStreamResult(
                snapshot: nil,
                shouldStop: streamUpdate.shouldStop,
                debugMessage: streamUpdate.debugMessage
            )
        }

        return MenuBarClientCoreMenuSnapshotStreamResult(
            snapshot: MenuBarSessionMiniLocalSnapshot(streamUpdate.snapshot),
            shouldStop: streamUpdate.shouldStop,
            debugMessage: streamUpdate.debugMessage
        )
    }

    public func runStateMiniSync(
        onSnapshot: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void,
        onDebugMessage: @escaping @MainActor (String) -> Void
    ) async {
        defer {
            stop()
        }

        do {
            try await drainStateMiniSync(
                onSnapshot: onSnapshot,
                onDebugMessage: onDebugMessage
            )
        } catch {
            await onDebugMessage("session mini stream failed: \(error.localizedDescription)")
        }
    }

    @discardableResult
    public func setSessionMode(
        threadID: String,
        preset: String
    ) async throws -> ClientSessionModeIntentResult {
        try await sessionManager.setMode(
            threadID: threadID,
            preset: preset
        )
    }

    @discardableResult
    public func sendPrompt(
        threadID: String,
        prompt: String,
        assistantSurface: String
    ) async throws -> ClientSessionPromptIntentResult {
        try await sessionManager.sendPrompt(
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            promptIntent: "steer"
        )
    }

    public func persistNotificationReply(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String
    ) throws -> ClientNotificationReplyPersistResult {
        try sessionManager.persistNotificationReply(
            notificationID: notificationID,
            threadID: threadID,
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
        clientMutationID: String?
    ) async throws -> ClientNotificationReplyIntentResult {
        if let clientMutationID {
            return try await sessionManager.submitNotificationReply(
                notificationID: notificationID,
                threadID: threadID,
                prompt: prompt,
                assistantSurface: assistantSurface,
                clientMutationID: clientMutationID
            )
        }
        return try await sessionManager.submitNotificationReplyWithGeneratedMutation(
            notificationID: notificationID,
            threadID: threadID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
    }

    private func drainStateMiniSync(
        onSnapshot: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void,
        onDebugMessage: @escaping @MainActor (String) -> Void
    ) async throws {
        while !Task.isCancelled {
            let result = try await nextMenuSnapshotStreamResult()
            if let snapshot = result.snapshot {
                await onSnapshot(snapshot)
            }
            if !result.debugMessage.isEmpty {
                await onDebugMessage(result.debugMessage)
            }
            if result.shouldStop {
                return
            }
        }
    }
}

public enum MenuBarSessionRuntimeError: LocalizedError {
    case noRealtimeEndpoint

    public var errorDescription: String? {
        switch self {
        case .noRealtimeEndpoint:
            "No realtime endpoint available"
        }
    }
}

private extension MenuBarSessionMiniLocalSnapshot {
    init(_ snapshot: ClientMenuBarSessionMiniLocalSnapshot) {
        let pendingCommands = snapshot.pendingCommands.map(MenuBarSessionMiniPendingCommand.init)
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions.map {
                MenuBarSessionMini($0, pendingCommands: pendingCommands)
            },
            pendingCommands: pendingCommands,
            clientCoreSnapshot: snapshot
        )
    }
}

private extension MenuBarSessionMiniPendingCommand {
    init(_ command: ClientMenuBarSessionMiniPendingCommand) {
        self.init(
            kind: command.kind,
            clientMutationID: command.clientMutationId,
            threadID: command.threadId,
            notificationID: command.notificationId.nilIfBlank,
            prompt: command.prompt.nilIfBlank,
            attemptCount: Int(command.attemptCount)
        )
    }

    init(_ command: ClientPendingCommand) {
        self.init(
            kind: command.kind,
            clientMutationID: command.clientMutationId,
            threadID: command.threadId,
            notificationID: command.notificationId.nilIfBlank,
            prompt: command.prompt.nilIfBlank,
            attemptCount: Int(command.attemptCount)
        )
    }
}

private extension [MenuBarSessionMiniPendingCommand] {
    func pendingNotificationReplyCount(for threadID: String) -> Int {
        filter {
            $0.kind == .submitNotificationReply
                && $0.threadID == threadID
        }.count
    }
}

private extension MenuBarSessionMiniLocalStore {
    func currentStateMiniSnapshot() throws -> ClientLocalStateSnapshot {
        try sessionManager.localSnapshot()
    }

}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

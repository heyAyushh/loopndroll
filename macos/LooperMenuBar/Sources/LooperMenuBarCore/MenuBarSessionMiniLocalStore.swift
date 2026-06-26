import Foundation
import LooperClientCore

public struct MenuBarSessionMiniRecord: Equatable, Sendable {
    public let sessionID: String
    public let assistantSurface: String
    public let seq: Int64
    public let revision: String
    public let payloadJSON: String

    public init(
        sessionID: String,
        assistantSurface: String,
        seq: Int64,
        revision: String,
        payloadJSON: String
    ) {
        self.sessionID = sessionID
        self.assistantSurface = assistantSurface
        self.seq = seq
        self.revision = revision
        self.payloadJSON = payloadJSON
    }
}

public struct MenuBarSessionMiniLocalSnapshot: Equatable, Sendable {
    public let latestSeq: Int64
    public let sessions: [MenuBarSessionMini]
    public let pendingCommands: [MenuBarSessionMiniPendingCommand]
}

public struct MenuBarSessionMiniPendingCommand: Equatable, Sendable {
    public let kind: ClientPendingCommandKind
    public let clientMutationID: String
    public let threadID: String
    public let notificationID: String?
    public let prompt: String?
    public let attemptCount: Int
}

public enum MenuBarClientCoreStateMiniStreamUpdateReason: Equatable, Sendable {
    case delta
    case heartbeat
    case reconnecting
    case recoveryRequired
    case stopped
}

public struct MenuBarClientCoreStateMiniStreamResult: Sendable {
    public let reason: MenuBarClientCoreStateMiniStreamUpdateReason
    public let snapshot: MenuBarSessionMiniLocalSnapshot?
    public let didChange: Bool
    public let errorDescription: String
}

public struct MenuBarSessionMini: Equatable, Sendable {
    public let sessionID: String
    public let assistantSurface: String
    public let seq: Int64
    public let revision: String
    public let ref: String
    public let title: String
    public let status: String
    public let effectiveMode: String?
    public let replyable: Bool
    public let promptUnavailableReason: String?
    public let blockedGoal: MenuBarSessionMiniBlockedGoal?
    public let queueCount: Int
    public let lifecycle: String?
    public let notificationStatus: MenuBarSessionMiniNotificationStatus?
    public let isArchived: Bool
    public let assistantPreview: String?
    public let projectName: String?
    public let projectPath: String?
    public let lastActivityAtMs: Int64?
    public let updatedAtMs: Int64?

    fileprivate init(_ mini: ClientMenuBarSessionMini) {
        self.sessionID = mini.sessionId
        self.assistantSurface = mini.assistantSurface
        self.seq = mini.seq
        self.revision = mini.revision
        self.ref = mini.refId
        self.title = mini.title
        self.status = mini.status
        self.effectiveMode = mini.hasEffectiveMode ? mini.effectiveMode : nil
        self.replyable = mini.replyable
        self.promptUnavailableReason =
            mini.hasPromptUnavailableReason ? mini.promptUnavailableReason : nil
        self.blockedGoal = mini.hasBlockedGoal ? MenuBarSessionMiniBlockedGoal(mini.blockedGoal) : nil
        self.queueCount = Int(mini.queueCount)
        self.lifecycle = mini.hasLifecycle ? mini.lifecycle : nil
        self.notificationStatus = mini.hasNotificationStatus
            ? MenuBarSessionMiniNotificationStatus(mini.notificationStatus)
            : nil
        self.isArchived = mini.isArchived
        self.assistantPreview = mini.hasAssistantPreview ? mini.assistantPreview : nil
        self.projectName = mini.hasProjectName ? mini.projectName : nil
        self.projectPath = mini.hasProjectPath ? mini.projectPath : nil
        self.lastActivityAtMs = mini.hasLastActivityAtMs ? mini.lastActivityAtMs : nil
        self.updatedAtMs = mini.hasUpdatedAtMs ? mini.updatedAtMs : nil
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

public final class MenuBarSessionMiniLocalStore: @unchecked Sendable {
    public static let defaultFileName = "looper-realtime-state-minis.json"

    private enum Defaults {
        static let applicationSupportDirectoryName = "looper"
    }

    private let store: LooperClientCoreLocalStore
    private let clientCore: LooperClientCore

    public init(fileURL: URL) throws {
        store = try LooperClientCoreLocalStore(filePath: fileURL.path)
        clientCore = LooperClientCore()
        _ = try? clientCore.replaceStateMinis(
            snapshot: ClientStateMiniSnapshot(store.snapshot())
        )
    }

    public static func liveDefault() -> MenuBarSessionMiniLocalStore? {
        do {
            return try MenuBarSessionMiniLocalStore(fileURL: defaultFileURL())
        } catch {
            return nil
        }
    }

    public static func available(fileURL: URL) -> MenuBarSessionMiniLocalStore? {
        try? MenuBarSessionMiniLocalStore(fileURL: fileURL)
    }

    public func cachedSnapshot() throws -> MenuBarSessionMiniLocalSnapshot? {
        let snapshot = currentStateMiniSnapshot()
        guard !snapshot.sessions.isEmpty else {
            return nil
        }
        return try menuSnapshot(from: snapshot)
    }

    @discardableResult
    public func replace(with snapshot: ClientStateMiniSnapshot) throws
        -> MenuBarSessionMiniLocalSnapshot?
    {
        let localSnapshot = try replaceStateMinis(with: snapshot)
        return try menuSnapshot(from: localSnapshot)
    }

    @discardableResult
    public func apply(_ delta: ClientStateMiniDelta) throws
        -> MenuBarSessionMiniLocalSnapshot?
    {
        let localSnapshot = try applyStateMiniDelta(delta)
        return try menuSnapshot(from: localSnapshot)
    }

    @discardableResult
    public func replace(
        latestSeq: Int64,
        records: [MenuBarSessionMiniRecord],
        serverTime: String? = nil
    ) throws -> MenuBarSessionMiniLocalSnapshot? {
        try replace(
            with: ClientStateMiniSnapshot(
                latestSeq: latestSeq,
                sessions: records.map(ClientStateMini.init),
                serverTime: serverTime ?? ""
            )
        )
    }

    public func enqueueModeCommand(
        threadID: String,
        preset: String?,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            command: ClientPendingCommand(
                kind: .setSessionMode,
                clientMutationId: clientMutationID,
                threadId: threadID,
                preset: preset ?? "",
                assistantSurface: "",
                prompt: "",
                notificationId: "",
                attemptCount: 0
            )
        )
    }

    public func enqueuePromptCommand(
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            command: ClientPendingCommand(
                kind: .sendSessionPrompt,
                clientMutationId: clientMutationID,
                threadId: threadID,
                preset: "",
                assistantSurface: assistantSurface ?? "",
                prompt: prompt,
                notificationId: "",
                attemptCount: 0
            )
        )
    }

    public func enqueueNotificationReplyCommand(
        notificationID: String,
        threadID: String,
        prompt: String,
        assistantSurface: String?,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            command: ClientPendingCommand(
                kind: .submitNotificationReply,
                clientMutationId: clientMutationID,
                threadId: threadID,
                preset: "",
                assistantSurface: assistantSurface ?? "",
                prompt: prompt,
                notificationId: notificationID,
                attemptCount: 0
            )
        )
    }

    public func markAttempted(clientMutationID: String) throws {
        _ = try store.markAttempted(clientMutationId: clientMutationID)
    }

    public func markDelivered(clientMutationID: String) throws {
        try store.markDelivered(clientMutationId: clientMutationID)
    }

    public func pendingCommands() -> [MenuBarSessionMiniPendingCommand] {
        (try? store.snapshot().pendingCommands.map(MenuBarSessionMiniPendingCommand.init)) ?? []
    }

    public func startClientCoreStateMiniStream(
        using transport: any LooperClientCoreStateMiniStreamTransport
    ) async throws {
        _ = try clientCore.replaceStateMinis(snapshot: ClientStateMiniSnapshot(store.snapshot()))
        try await transport.startClientCoreStateMiniStream(clientCore: clientCore)
    }

    public func nextClientCoreStateMiniStreamResult(
        using transport: any LooperClientCoreStateMiniStreamTransport
    ) async throws -> MenuBarClientCoreStateMiniStreamResult {
        let streamUpdate = try await transport.nextClientCoreStateMiniStreamUpdate(
            clientCore: clientCore
        )
        guard
            (streamUpdate.reason == .delta || streamUpdate.reason == .recoveryRequired),
            streamUpdate.didChange
        else {
            return MenuBarClientCoreStateMiniStreamResult(
                reason: MenuBarClientCoreStateMiniStreamUpdateReason(streamUpdate.reason),
                snapshot: nil,
                didChange: false,
                errorDescription: streamUpdate.errorDescription
            )
        }

        let localSnapshot = try persistValidated(streamUpdate.snapshot)
        return MenuBarClientCoreStateMiniStreamResult(
            reason: MenuBarClientCoreStateMiniStreamUpdateReason(streamUpdate.reason),
            snapshot: try menuSnapshot(from: localSnapshot),
            didChange: true,
            errorDescription: streamUpdate.errorDescription
        )
    }

    public func stopClientCoreStateMiniStream(
        using transport: any LooperClientCoreStateMiniStreamTransport
    ) {
        try? transport.stopClientCoreStateMiniStream(clientCore: clientCore)
    }

    public func runClientCoreStateMiniSync(
        using transport: any LooperClientCoreStateMiniStreamTransport,
        onSnapshot: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void,
        onDebugMessage: @escaping @MainActor (String) -> Void
    ) async {
        defer {
            stopClientCoreStateMiniStream(using: transport)
        }

        do {
            try await startClientCoreStateMiniStream(using: transport)
            try await drainClientCoreStateMiniSync(
                using: transport,
                onSnapshot: onSnapshot,
                onDebugMessage: onDebugMessage
            )
        } catch {
            await onDebugMessage("session mini stream failed: \(error.localizedDescription)")
        }
    }

    private static func defaultFileURL() throws -> URL {
        try FileManager.default.url(
            for: .applicationSupportDirectory,
            in: .userDomainMask,
            appropriateFor: nil,
            create: true
        )
        .appendingPathComponent(Defaults.applicationSupportDirectoryName, isDirectory: true)
        .appendingPathComponent(defaultFileName)
    }

    private func drainClientCoreStateMiniSync(
        using transport: any LooperClientCoreStateMiniStreamTransport,
        onSnapshot: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void,
        onDebugMessage: @escaping @MainActor (String) -> Void
    ) async throws {
        while !Task.isCancelled {
            let result = try await nextClientCoreStateMiniStreamResult(using: transport)
            switch result.reason {
            case .delta:
                if let snapshot = result.snapshot {
                    await onSnapshot(snapshot)
                }
            case .heartbeat, .reconnecting:
                continue
            case .recoveryRequired:
                if let snapshot = result.snapshot {
                    await onSnapshot(snapshot)
                } else if !result.errorDescription.isEmpty {
                    await onDebugMessage(
                        "session mini stream recovery waiting: \(result.errorDescription)"
                    )
                }
            case .stopped:
                return
            }
        }
    }

    private func localSnapshot(from snapshot: ClientStateSnapshot) -> ClientLocalStateSnapshot {
        let durableSnapshot = try? store.snapshot()
        return ClientLocalStateSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.stateMinis,
            pendingCommands: durableSnapshot?.pendingCommands ?? [],
            serverTime: snapshot.serverTime
        )
    }

    @discardableResult
    private func persistValidated(_ snapshot: ClientStateSnapshot) throws
        -> ClientLocalStateSnapshot
    {
        let sessions = snapshot.stateMinis
        _ = try menuSnapshot(
            from: ClientLocalStateSnapshot(
                latestSeq: snapshot.latestSeq,
                sessions: sessions,
                pendingCommands: [],
                serverTime: snapshot.serverTime
            )
        )
        return try store.replaceStateMinis(
            snapshot: ClientStateMiniSnapshot(
                latestSeq: snapshot.latestSeq,
                sessions: sessions,
                serverTime: snapshot.serverTime
            )
        )
    }

    private func menuSnapshot(from snapshot: ClientLocalStateSnapshot) throws
        -> MenuBarSessionMiniLocalSnapshot?
    {
        let projection = try reduceStateMinisMenuSnapshot(snapshot: snapshot)
        guard !projection.sessions.isEmpty else {
            return nil
        }
        return MenuBarSessionMiniLocalSnapshot(
            latestSeq: projection.latestSeq,
            sessions: projection.sessions.map(MenuBarSessionMini.init),
            pendingCommands: projection.pendingCommands.map(MenuBarSessionMiniPendingCommand.init)
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

private extension ClientStateMini {
    init(_ record: MenuBarSessionMiniRecord) {
        self.init(
            sessionId: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJson: record.payloadJSON
        )
    }
}

extension MenuBarSessionMiniLocalStore {
    public func currentStateMiniSnapshot() -> ClientLocalStateSnapshot {
        do {
            return try localSnapshot(from: clientCore.snapshot())
        } catch {
            return (try? store.snapshot())
                ?? ClientLocalStateSnapshot(
                    latestSeq: 0,
                    sessions: [],
                    pendingCommands: [],
                    serverTime: ""
                )
        }
    }

    @discardableResult
    public func replaceStateMinis(with snapshot: ClientStateMiniSnapshot) throws
        -> ClientLocalStateSnapshot
    {
        let coreSnapshot = try clientCore.replaceStateMinis(
            snapshot: snapshot
        )
        return try persistValidated(coreSnapshot)
    }

    @discardableResult
    public func applyStateMiniDelta(_ delta: ClientStateMiniDelta) throws
        -> ClientLocalStateSnapshot
    {
        let result = try clientCore.applyStateMiniDeltaWithResult(
            delta: delta
        )
        guard result.didChange else {
            return localSnapshot(from: result.snapshot)
        }
        return try persistValidated(result.snapshot)
    }
}

private extension ClientStateMiniSnapshot {
    init(_ snapshot: ClientLocalStateSnapshot) {
        self.init(
            latestSeq: snapshot.latestSeq,
            sessions: snapshot.sessions,
            serverTime: snapshot.serverTime
        )
    }
}

private extension MenuBarClientCoreStateMiniStreamUpdateReason {
    init(_ reason: ClientStateMiniStreamUpdateReason) {
        switch reason {
        case .delta:
            self = .delta
        case .heartbeat:
            self = .heartbeat
        case .reconnecting:
            self = .reconnecting
        case .recoveryRequired:
            self = .recoveryRequired
        case .stopped:
            self = .stopped
        }
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

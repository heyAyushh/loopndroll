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

    fileprivate init(record: ClientStateMini, payload: MenuBarSessionMiniPayload) {
        self.sessionID = record.sessionId
        self.assistantSurface = record.assistantSurface
        self.seq = record.seq
        self.revision = record.revision
        self.ref = payload.ref
        self.title = payload.displayTitle(fallbackID: record.sessionId)
        self.status = payload.status
        self.effectiveMode = payload.effectiveMode?.nilIfBlank
        self.replyable = payload.replyable
        self.promptUnavailableReason = payload.promptUnavailableReason?.nilIfBlank
        self.blockedGoal = payload.blockedGoal
        self.queueCount = payload.queueCount
        self.lifecycle = payload.lifecycle?.nilIfBlank
        self.notificationStatus = payload.notificationStatus
        self.isArchived = payload.isArchived
        self.assistantPreview = payload.assistantPreview?.nilIfBlank
        self.projectName = payload.metadata?.projectName?.nilIfBlank
        self.projectPath = payload.metadata?.projectPath?.nilIfBlank
        self.lastActivityAtMs = payload.lastActivityAtMs
        self.updatedAtMs = payload.updatedAtMs
    }

    fileprivate static func sortForMenu(lhs: MenuBarSessionMini, rhs: MenuBarSessionMini) -> Bool {
        let lhsActivity = lhs.lastActivityAtMs ?? lhs.updatedAtMs ?? lhs.seq
        let rhsActivity = rhs.lastActivityAtMs ?? rhs.updatedAtMs ?? rhs.seq
        if lhsActivity != rhsActivity {
            return lhsActivity > rhsActivity
        }
        if lhs.seq != rhs.seq {
            return lhs.seq > rhs.seq
        }
        return lhs.sessionID < rhs.sessionID
    }
}

public struct MenuBarSessionMiniBlockedGoal: Codable, Equatable, Sendable {
    public let id: String?
    public let title: String?
    public let status: String?
    public let lifecycle: String?
    public let reason: String?
}

public struct MenuBarSessionMiniNotificationStatus: Codable, Equatable, Sendable {
    public let enabled: Bool
    public let targetIds: [String]
    public let usesDefault: Bool
}

public enum MenuBarSessionMiniLocalStoreError: Error, Equatable, Sendable {
    case sessionIDMismatch(expected: String, actual: String)
}

public final class MenuBarSessionMiniLocalStore: @unchecked Sendable {
    public static let defaultFileName = "looper-realtime-state-minis.json"

    private enum Defaults {
        static let applicationSupportDirectoryName = "looper"
    }

    private let store: LooperClientCoreLocalStore
    private let clientCore: LooperClientCore
    private let decoder = JSONDecoder()

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
        guard streamUpdate.reason == .delta, streamUpdate.didChange else {
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

    public func recoverClientCoreStateMiniStream(
        using transport: any LooperClientCoreStateMiniStreamTransport
    ) async throws -> MenuBarSessionMiniLocalSnapshot? {
        let snapshot = try await transport.recoverClientCoreStateMiniSnapshot(
            clientCore: clientCore
        )
        let localSnapshot = try persistValidated(snapshot)
        return try menuSnapshot(from: localSnapshot)
    }

    public func stopClientCoreStateMiniStream(
        using transport: any LooperClientCoreStateMiniStreamTransport
    ) {
        try? transport.stopClientCoreStateMiniStream(clientCore: clientCore)
    }

    public func runClientCoreStateMiniSync(
        using transport: any LooperClientCoreStateMiniStreamTransport,
        retryDelay: Duration,
        onSnapshot: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void,
        onDebugMessage: @escaping @MainActor (String) -> Void
    ) async {
        defer {
            stopClientCoreStateMiniStream(using: transport)
        }

        while !Task.isCancelled {
            do {
                try await startClientCoreStateMiniStream(using: transport)
                try await drainClientCoreStateMiniSync(
                    using: transport,
                    onSnapshot: onSnapshot,
                    onDebugMessage: onDebugMessage
                )
            } catch {
                await recoverClientCoreStateMiniSync(
                    using: transport,
                    errorDescription: error.localizedDescription,
                    onSnapshot: onSnapshot,
                    onDebugMessage: onDebugMessage
                )
            }

            do {
                try await Task.sleep(for: retryDelay)
            } catch {
                return
            }
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
                await recoverClientCoreStateMiniSync(
                    using: transport,
                    errorDescription: result.errorDescription,
                    onSnapshot: onSnapshot,
                    onDebugMessage: onDebugMessage
                )
                return
            case .stopped:
                return
            }
        }
    }

    private func recoverClientCoreStateMiniSync(
        using transport: any LooperClientCoreStateMiniStreamTransport,
        errorDescription: String,
        onSnapshot: @escaping @MainActor (MenuBarSessionMiniLocalSnapshot) -> Void,
        onDebugMessage: @escaping @MainActor (String) -> Void
    ) async {
        if !errorDescription.isEmpty {
            await onDebugMessage("session mini stream recovery: \(errorDescription)")
        }

        do {
            if let snapshot = try await recoverClientCoreStateMiniStream(using: transport) {
                await onSnapshot(snapshot)
            }
        } catch {
            await onDebugMessage(
                "session mini stream recovery failed: \(error.localizedDescription)"
            )
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
        _ = try menuSnapshot(from: snapshot.latestSeq, sessions: sessions)
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
        guard !snapshot.sessions.isEmpty else {
            return nil
        }
        return try MenuBarSessionMiniLocalSnapshot(
            latestSeq: snapshot.latestSeq,
            sessions: menuSessions(from: snapshot.sessions),
            pendingCommands: snapshot.pendingCommands.map(MenuBarSessionMiniPendingCommand.init)
        )
    }
    private func menuSnapshot(
        from latestSeq: Int64,
        sessions: [ClientStateMini]
    ) throws -> MenuBarSessionMiniLocalSnapshot? {
        guard !sessions.isEmpty else {
            return nil
        }
        return try MenuBarSessionMiniLocalSnapshot(
            latestSeq: latestSeq,
            sessions: menuSessions(from: sessions),
            pendingCommands: []
        )
    }

    private func menuSessions(from sessions: [ClientStateMini]) throws
        -> [MenuBarSessionMini]
    {
        try sessions
            .map(decodeSessionMini)
            .sorted(by: MenuBarSessionMini.sortForMenu)
    }

    private func decodeSessionMini(from record: ClientStateMini) throws -> MenuBarSessionMini {
        let data = Data(record.payloadJson.utf8)
        let payload = try decoder.decode(MenuBarSessionMiniPayload.self, from: data)
        guard payload.sessionID == record.sessionId else {
            throw MenuBarSessionMiniLocalStoreError.sessionIDMismatch(
                expected: record.sessionId,
                actual: payload.sessionID
            )
        }
        return MenuBarSessionMini(record: record, payload: payload)
    }
}

private struct MenuBarSessionMiniPayload: Decodable {
    let sessionID: String
    let ref: String
    let title: String
    let status: String
    let effectiveMode: String?
    let replyable: Bool
    let promptUnavailableReason: String?
    let blockedGoal: MenuBarSessionMiniBlockedGoal?
    let queueCount: Int
    let lifecycle: String?
    let notificationStatus: MenuBarSessionMiniNotificationStatus?
    let isArchived: Bool
    let assistantPreview: String?
    let metadata: MenuBarSessionMiniMetadata?
    let lastActivityAtMs: Int64?
    let updatedAtMs: Int64?

    private enum CodingKeys: String, CodingKey {
        case id
        case sessionID = "sessionId"
        case ref
        case title
        case status
        case effectiveMode
        case canSendPrompt
        case replyable
        case promptDeliveryUnavailableReason
        case blockedGoal
        case queueCount
        case lifecycle
        case notificationStatus
        case isArchived
        case assistantPreview
        case metadata
        case lastActivityAtMs
        case updatedAtMs
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        sessionID = try container.decodeIfPresent(String.self, forKey: .sessionID)
            ?? container.decode(String.self, forKey: .id)
        ref = try container.decodeIfPresent(String.self, forKey: .ref) ?? sessionID
        title = try container.decodeIfPresent(String.self, forKey: .title) ?? sessionID
        status = try container.decodeIfPresent(String.self, forKey: .status) ?? ""
        effectiveMode = try container.decodeIfPresent(String.self, forKey: .effectiveMode)
        replyable = try container.decodeIfPresent(Bool.self, forKey: .replyable)
            ?? container.decodeIfPresent(Bool.self, forKey: .canSendPrompt)
            ?? false
        promptUnavailableReason = try container.decodeIfPresent(
            String.self,
            forKey: .promptDeliveryUnavailableReason
        )
        blockedGoal = try container.decodeIfPresent(MenuBarSessionMiniBlockedGoal.self, forKey: .blockedGoal)
        queueCount = try container.decodeIfPresent(Int.self, forKey: .queueCount) ?? 0
        lifecycle = try container.decodeIfPresent(String.self, forKey: .lifecycle)
        notificationStatus = try container.decodeIfPresent(
            MenuBarSessionMiniNotificationStatus.self,
            forKey: .notificationStatus
        )
        let decodedArchived = try container.decodeIfPresent(Bool.self, forKey: .isArchived)
        isArchived = decodedArchived ?? (status == "archived")
        assistantPreview = try container.decodeIfPresent(String.self, forKey: .assistantPreview)
        metadata = try container.decodeIfPresent(MenuBarSessionMiniMetadata.self, forKey: .metadata)
        lastActivityAtMs = try container.decodeIfPresent(Int64.self, forKey: .lastActivityAtMs)
        updatedAtMs = try container.decodeIfPresent(Int64.self, forKey: .updatedAtMs)
    }

    func displayTitle(fallbackID: String) -> String {
        let candidates = [title, ref, fallbackID]
        return candidates
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first { !$0.isEmpty } ?? fallbackID
    }
}

private struct MenuBarSessionMiniMetadata: Decodable {
    let projectName: String?
    let projectPath: String?
}

private extension MenuBarSessionMiniPendingCommand {
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

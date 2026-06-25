import Foundation
import LooperRealtime

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
    public let kind: LooperRealtimePendingCommand.Kind
    public let clientMutationID: String
    public let threadID: String
    public let notificationID: String?
    public let prompt: String?
    public let attemptCount: Int
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

    fileprivate init(record: LooperRealtimeStateMini, payload: MenuBarSessionMiniPayload) {
        self.sessionID = record.sessionID
        self.assistantSurface = record.assistantSurface
        self.seq = record.seq
        self.revision = record.revision
        self.ref = payload.ref
        self.title = payload.displayTitle(fallbackID: record.sessionID)
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
    public static let defaultFileName = LooperRealtimeLocalStore.defaultFileName

    private enum Defaults {
        static let applicationSupportDirectoryName = "looper"
    }

    private let store: LooperRealtimeLocalStore
    private let decoder = JSONDecoder()

    public var realtimeLocalStore: LooperRealtimeLocalStore {
        store
    }

    public init(fileURL: URL) throws {
        store = try LooperRealtimeLocalStore(recovering: fileURL)
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
        let snapshot = store.snapshot()
        guard !snapshot.sessions.isEmpty else {
            return nil
        }
        return try menuSnapshot(from: snapshot)
    }

    @discardableResult
    public func replace(with snapshot: LooperRealtimeStateMiniSnapshot) throws
        -> MenuBarSessionMiniLocalSnapshot?
    {
        _ = try menuSnapshot(from: snapshot.latestSeq, sessions: snapshot.sessions)
        let localSnapshot = try store.replace(with: snapshot)
        return try menuSnapshot(from: localSnapshot)
    }

    @discardableResult
    public func apply(_ delta: LooperRealtimeStateMiniDelta) throws
        -> MenuBarSessionMiniLocalSnapshot?
    {
        let currentSnapshot = store.snapshot()
        if delta.seq <= currentSnapshot.latestSeq {
            return try menuSnapshot(from: currentSnapshot)
        }

        if let session = delta.session {
            _ = try decodeSessionMini(from: session)
        }

        let localSnapshot = try store.apply(delta)
        return try menuSnapshot(from: localSnapshot)
    }

    @discardableResult
    public func replace(
        latestSeq: Int64,
        records: [MenuBarSessionMiniRecord],
        serverTime: String? = nil
    ) throws -> MenuBarSessionMiniLocalSnapshot? {
        try replace(
            with: LooperRealtimeStateMiniSnapshot(
                latestSeq: latestSeq,
                sessions: records.map(LooperRealtimeStateMini.init),
                serverTime: serverTime
            )
        )
    }

    public func enqueueModeCommand(
        threadID: String,
        preset: String?,
        clientMutationID: String
    ) throws {
        _ = try store.enqueue(
            LooperRealtimePendingCommand(
                kind: .setSessionMode,
                clientMutationID: clientMutationID,
                threadID: threadID,
                preset: preset
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
            LooperRealtimePendingCommand(
                kind: .sendSessionPrompt,
                clientMutationID: clientMutationID,
                threadID: threadID,
                assistantSurface: assistantSurface,
                prompt: prompt
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
            LooperRealtimePendingCommand(
                kind: .submitNotificationReply,
                clientMutationID: clientMutationID,
                threadID: threadID,
                assistantSurface: assistantSurface,
                prompt: prompt,
                notificationID: notificationID
            )
        )
    }

    public func markAttempted(clientMutationID: String) throws {
        _ = try store.markAttempted(clientMutationID: clientMutationID)
    }

    public func markDelivered(clientMutationID: String) throws {
        try store.markDelivered(clientMutationID: clientMutationID)
    }

    public func pendingCommands() -> [MenuBarSessionMiniPendingCommand] {
        store.snapshot().pendingCommands.map(MenuBarSessionMiniPendingCommand.init)
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

    private func menuSnapshot(from snapshot: LooperRealtimeLocalStateSnapshot) throws
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
        sessions: [LooperRealtimeStateMini]
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

    private func menuSessions(from sessions: [LooperRealtimeStateMini]) throws
        -> [MenuBarSessionMini]
    {
        try sessions
            .map(decodeSessionMini)
            .sorted(by: MenuBarSessionMini.sortForMenu)
    }

    private func decodeSessionMini(from record: LooperRealtimeStateMini) throws -> MenuBarSessionMini {
        let data = Data(record.payloadJSON.utf8)
        let payload = try decoder.decode(MenuBarSessionMiniPayload.self, from: data)
        guard payload.sessionID == record.sessionID else {
            throw MenuBarSessionMiniLocalStoreError.sessionIDMismatch(
                expected: record.sessionID,
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
    init(_ command: LooperRealtimePendingCommand) {
        self.init(
            kind: command.kind,
            clientMutationID: command.clientMutationID,
            threadID: command.threadID,
            notificationID: command.notificationID,
            prompt: command.prompt,
            attemptCount: command.attemptCount
        )
    }
}

private extension LooperRealtimeStateMini {
    init(_ record: MenuBarSessionMiniRecord) {
        self.init(
            sessionID: record.sessionID,
            assistantSurface: record.assistantSurface,
            seq: record.seq,
            revision: record.revision,
            payloadJSON: record.payloadJSON
        )
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

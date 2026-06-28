import Foundation

private enum CompanionSessionMiniControllerRetry {
    static let delay: Duration = .milliseconds(750)
    static let restartReason = "runtime-restart"
    static let restartLatestSeq: Int64 = 0
    static let restartServerTime = ""
}

enum SessionSyncAssistantSurfaceCommandStatus: Equatable {
    case applied
    case stale
    case failed(String)
}

struct SessionSyncAssistantSurfaceCommandResult: Equatable {
    let requestedSurface: CompanionAssistantSurface
    let appliedSurface: CompanionAssistantSurface?
    let generation: Int
    let status: SessionSyncAssistantSurfaceCommandStatus
    let pendingCommandCount: Int

    var didApplySelection: Bool {
        switch status {
        case .applied:
            return true
        case .stale,
             .failed:
            return false
        }
    }
}

@MainActor
protocol SessionSyncCommandDispatching: AnyObject {
    func dispatchAssistantSurface(_ surface: CompanionAssistantSurface) async throws
    func pendingCommands() -> [CompanionSessionMiniPendingCommand]
}

extension CompanionSessionRuntime: SessionSyncCommandDispatching {
    func dispatchAssistantSurface(_ surface: CompanionAssistantSurface) async throws {
        try await setAssistantSurface(surface)
    }
}

@MainActor
final class SessionSyncEngine {
    private struct AssistantSurfaceSelectionRequest: Equatable {
        let surface: CompanionAssistantSurface
        let generation: Int
    }

    private let commandDispatcher: (any SessionSyncCommandDispatching)?
    private var assistantSurfaceGeneration = 0
    private var assistantSurfaceSelectionTask: Task<SessionSyncAssistantSurfaceCommandResult, Never>?
    private var queuedAssistantSurfaceSelection: AssistantSurfaceSelectionRequest?
    private var activeAssistantSurfaceSelection: AssistantSurfaceSelectionRequest?

    init(commandDispatcher: (any SessionSyncCommandDispatching)?) {
        self.commandDispatcher = commandDispatcher
    }

    var pendingCommandCount: Int {
        let runtimePendingCommands = commandDispatcher?.pendingCommands() ?? []
        let localAssistantSurfacePendingCount = hasLocalAssistantSurfaceSelectionPending &&
            !runtimePendingCommands.contains { command in
                command.kind == .setAssistantSurface
            }
            ? 1
            : 0
        return runtimePendingCommands.count + localAssistantSurfacePendingCount
    }

    private var hasLocalAssistantSurfaceSelectionPending: Bool {
        queuedAssistantSurfaceSelection != nil ||
            activeAssistantSurfaceSelection != nil ||
            assistantSurfaceSelectionTask != nil
    }

    @discardableResult
    func selectAssistantSurface(
        _ surface: CompanionAssistantSurface
    ) -> Task<SessionSyncAssistantSurfaceCommandResult, Never>? {
        guard let commandDispatcher else {
            CompanionDiagnostics.record("assistant-surface:missing-dispatcher surface=\(surface.rawValue)")
            return nil
        }

        if queuedAssistantSurfaceSelection?.surface == surface ||
            activeAssistantSurfaceSelection?.surface == surface
        {
            CompanionDiagnostics.assistantSurface.debug(
                "Selection ignored surface=\(surface.rawValue, privacy: .public) reason=already-pending"
            )
            CompanionDiagnostics.record(
                "assistant-surface:ignored surface=\(surface.rawValue) reason=already-pending pendingCommands=\(pendingCommandCount)"
            )
            return assistantSurfaceSelectionTask
        }

        if let supersededSurface = queuedAssistantSurfaceSelection?.surface {
            CompanionDiagnostics.assistantSurface.debug(
                "Selection coalesced surface=\(supersededSurface.rawValue, privacy: .public) nextSurface=\(surface.rawValue, privacy: .public) reason=queued-replaced"
            )
            CompanionDiagnostics.record(
                "assistant-surface:coalesced surface=\(supersededSurface.rawValue) nextSurface=\(surface.rawValue) reason=queued-replaced"
            )
        }

        assistantSurfaceGeneration += 1
        let generation = assistantSurfaceGeneration
        queuedAssistantSurfaceSelection = AssistantSurfaceSelectionRequest(
            surface: surface,
            generation: generation
        )
        CompanionDiagnostics.assistantSurface.info(
            "Selection requested surface=\(surface.rawValue, privacy: .public) generation=\(generation, privacy: .public) pendingCommands=\(self.pendingCommandCount, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "assistant-surface:requested surface=\(surface.rawValue) generation=\(generation) pendingCommands=\(self.pendingCommandCount)"
        )

        if let assistantSurfaceSelectionTask {
            return assistantSurfaceSelectionTask
        }

        let task = Task { @MainActor [weak self, commandDispatcher] in
            await self?.drainAssistantSurfaceSelections(commandDispatcher: commandDispatcher)
                ?? SessionSyncAssistantSurfaceCommandResult(
                    requestedSurface: surface,
                    appliedSurface: nil,
                    generation: generation,
                    status: .failed("sync engine released"),
                    pendingCommandCount: 0
                )
        }
        assistantSurfaceSelectionTask = task
        return task
    }

    private func drainAssistantSurfaceSelections(
        commandDispatcher: any SessionSyncCommandDispatching
    ) async -> SessionSyncAssistantSurfaceCommandResult {
        var latestResult: SessionSyncAssistantSurfaceCommandResult?
        defer {
            queuedAssistantSurfaceSelection = nil
            activeAssistantSurfaceSelection = nil
            assistantSurfaceSelectionTask = nil
        }

        while let selection = queuedAssistantSurfaceSelection {
            queuedAssistantSurfaceSelection = nil
            activeAssistantSurfaceSelection = selection
            defer {
                activeAssistantSurfaceSelection = nil
            }

            let surface = selection.surface
            let generation = selection.generation
            do {
                CompanionDiagnostics.assistantSurface.info(
                    "Selection dispatch surface=\(surface.rawValue, privacy: .public) generation=\(generation, privacy: .public)"
                )
                CompanionDiagnostics.record(
                    "assistant-surface:dispatch surface=\(surface.rawValue) generation=\(generation)"
                )
                try await commandDispatcher.dispatchAssistantSurface(surface)
                guard assistantSurfaceGeneration == generation else {
                    CompanionDiagnostics.assistantSurface.debug(
                        "Selection stale surface=\(surface.rawValue, privacy: .public) generation=\(generation, privacy: .public) currentGeneration=\(self.assistantSurfaceGeneration, privacy: .public)"
                    )
                    CompanionDiagnostics.record(
                        "assistant-surface:stale surface=\(surface.rawValue) generation=\(generation) currentGeneration=\(self.assistantSurfaceGeneration)"
                    )
                    latestResult = SessionSyncAssistantSurfaceCommandResult(
                        requestedSurface: surface,
                        appliedSurface: nil,
                        generation: generation,
                        status: .stale,
                        pendingCommandCount: pendingCommandCount
                    )
                    continue
                }

                CompanionDiagnostics.assistantSurface.info(
                    "Selection applied surface=\(surface.rawValue, privacy: .public) generation=\(generation, privacy: .public)"
                )
                CompanionDiagnostics.record(
                    "assistant-surface:applied surface=\(surface.rawValue) generation=\(generation) pendingCommands=\(self.pendingCommandCount)"
                )
                let appliedResult = SessionSyncAssistantSurfaceCommandResult(
                    requestedSurface: surface,
                    appliedSurface: surface,
                    generation: generation,
                    status: .applied,
                    pendingCommandCount: pendingCommandCount
                )
                latestResult = appliedResult
                return appliedResult
            } catch {
                guard assistantSurfaceGeneration == generation else {
                    latestResult = SessionSyncAssistantSurfaceCommandResult(
                        requestedSurface: surface,
                        appliedSurface: nil,
                        generation: generation,
                        status: .stale,
                        pendingCommandCount: pendingCommandCount
                    )
                    continue
                }

                CompanionDiagnostics.assistantSurface.error(
                    "Selection failed surface=\(surface.rawValue, privacy: .public) generation=\(generation, privacy: .public) error=\(error.localizedDescription, privacy: .public)"
                )
                CompanionDiagnostics.record(
                    "assistant-surface:failed surface=\(surface.rawValue) generation=\(generation) error=\(error.localizedDescription)"
                )
                let failedResult = SessionSyncAssistantSurfaceCommandResult(
                    requestedSurface: surface,
                    appliedSurface: nil,
                    generation: generation,
                    status: .failed(error.localizedDescription),
                    pendingCommandCount: pendingCommandCount
                )
                latestResult = failedResult
                return failedResult
            }
        }

        return latestResult ?? SessionSyncAssistantSurfaceCommandResult(
            requestedSurface: .defaultSurface,
            appliedSurface: nil,
            generation: assistantSurfaceGeneration,
            status: .failed("no assistant surface selection dispatched"),
            pendingCommandCount: pendingCommandCount
        )
    }
}

@MainActor
final class CompanionSessionMiniController {
    typealias SyncUpdateHandler = @MainActor @Sendable (
        CompanionSessionMiniSyncUpdate,
        Int
    ) -> Void
    typealias LivenessUpdateHandler = @MainActor @Sendable (
        CompanionSessionMiniLivenessUpdate,
        Int
    ) -> Void
    typealias SnapshotApplyHandler = @MainActor (MobileSnapshot, String, Int64) -> Void
    typealias NotificationReplySubmitter = @MainActor @Sendable () async -> Bool

    let sessionRuntime: CompanionSessionRuntime?

    private var syncTask: Task<Void, Never>?
    private var notificationReplyOutboxDrainTask: Task<Bool, Never>?

    var isSyncing: Bool {
        syncTask != nil
    }

    init(sessionRuntime: CompanionSessionRuntime?) {
        self.sessionRuntime = sessionRuntime
    }

    func startSyncIfNeeded(
        connectionRevision: Int,
        onUpdate: @escaping SyncUpdateHandler,
        onLiveness: @escaping LivenessUpdateHandler
    ) {
        guard syncTask == nil, let sessionRuntime else {
            return
        }

        syncTask = Task { @MainActor [weak self] in
            defer {
                self?.syncTask = nil
            }
            while !Task.isCancelled {
                await sessionRuntime.prepareSessionRuntime()
                await sessionRuntime.runStateMiniSync(
                    onUpdate: { update in
                        onUpdate(update, connectionRevision)
                    },
                    onLiveness: { liveness in
                        onLiveness(liveness, connectionRevision)
                    },
                    onDebugMessage: { message in
                        CompanionDiagnostics.record(message)
                    }
                )

                guard !Task.isCancelled else {
                    return
                }

                onLiveness(Self.restartLivenessUpdate(), connectionRevision)
                CompanionDiagnostics.record("session-mini:sync-restarting")
                try? await Task.sleep(for: CompanionSessionMiniControllerRetry.delay)
            }
        }
    }

    static func restartLivenessUpdate() -> CompanionSessionMiniLivenessUpdate {
        CompanionSessionMiniLivenessUpdate(
            reason: CompanionSessionMiniControllerRetry.restartReason,
            latestSeq: CompanionSessionMiniControllerRetry.restartLatestSeq,
            serverTime: CompanionSessionMiniControllerRetry.restartServerTime,
            isLive: false,
            endpointURL: nil
        )
    }

    func stopSync() {
        let task = syncTask
        syncTask = nil
        task?.cancel()
        sessionRuntime?.stopStateMiniStream()
    }

    func stopSyncAndWait() async {
        let task = syncTask
        syncTask = nil
        task?.cancel()
        sessionRuntime?.stopStateMiniStream()
        await task?.value
    }

    @discardableResult
    func restoreCachedSnapshotIfAvailable(
        reason: String,
        applySnapshot: SnapshotApplyHandler
    ) -> Bool {
        guard let sessionRuntime else {
            return false
        }

        do {
            let localSnapshot = try sessionRuntime.currentStateMiniSnapshot()
            guard let cachedSnapshot = try sessionRuntime.cachedSnapshot() else {
                return false
            }

            applySnapshot(cachedSnapshot, "session-mini-\(reason)", localSnapshot.latestSeq)
            CompanionDiagnostics.record(
                "session-mini:cache-restore reason=\(reason) sessions=\(cachedSnapshot.sessions.count)"
            )
            return true
        } catch {
            CompanionDiagnostics.record(
                "session-mini:cache-restore-failed reason=\(reason) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    func cachedSnapshot() throws -> MobileSnapshot? {
        try sessionRuntime?.cachedSnapshot()
    }

    @discardableResult
    func startNotificationReplyOutboxDrainIfNeeded(
        submit: @escaping NotificationReplySubmitter
    ) -> Task<Bool, Never>? {
        guard notificationReplyOutboxDrainTask == nil else {
            return notificationReplyOutboxDrainTask
        }

        let drainTask = Task { @MainActor [weak self] in
            guard let self else {
                return false
            }
            defer {
                self.notificationReplyOutboxDrainTask = nil
            }
            if !Task.isCancelled {
                return await submit()
            }
            return false
        }
        notificationReplyOutboxDrainTask = drainTask
        return drainTask
    }

    func stopNotificationReplyOutboxDrain() {
        notificationReplyOutboxDrainTask?.cancel()
        notificationReplyOutboxDrainTask = nil
    }
}

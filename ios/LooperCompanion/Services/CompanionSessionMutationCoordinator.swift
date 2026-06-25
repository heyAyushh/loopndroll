import Foundation
import LooperClientCore

private enum LocalFirstMutationError: LocalizedError {
    case modeBarrierRejected

    var errorDescription: String? {
        switch self {
        case .modeBarrierRejected:
            return "Mode change was not accepted. Prompt stayed queued."
        }
    }
}

private extension ClientModeMutation {
    var mode: SessionMode? {
        let trimmedPreset = preset.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPreset.isEmpty else {
            return nil
        }

        return SessionMode(rawValue: trimmedPreset)
    }
}

struct ModeRollbackState: Sendable {
    let snapshot: MobileSnapshot?
    let detail: SessionDetail?
}

private struct ModeMutationEnvelope: Sendable {
    let mutation: ClientModeMutation
    let connectionRevision: Int
    let rollbackState: ModeRollbackState?
    let service: any CompanionService

    var sessionID: String {
        mutation.sessionId
    }

    var clientMutationID: String {
        mutation.clientMutationId
    }

    var mode: SessionMode? {
        mutation.mode
    }
}

private struct PromptMutationEnvelope: Sendable {
    let sessionID: String
    let prompt: String
    let assistantSurface: CompanionAssistantSurface
    let clientMutationID: String
    let connectionRevision: Int
    let service: any CompanionService
    let pendingModeMutation: ClientModeMutation?
    let modeBarrierTask: Task<Bool, Never>?
}

private actor LocalFirstMutationBarrier {
    private var result: Bool?
    private var continuations: [CheckedContinuation<Bool, Never>] = []

    func wait() async -> Bool {
        if let result {
            return result
        }

        return await withCheckedContinuation { continuation in
            continuations.append(continuation)
        }
    }

    func resolve(_ accepted: Bool) {
        guard result == nil else {
            return
        }

        result = accepted
        let continuations = continuations
        self.continuations.removeAll()
        for continuation in continuations {
            continuation.resume(returning: accepted)
        }
    }
}

@MainActor
protocol CompanionSessionMutationCoordinatorDelegate: AnyObject {
    var sessionMutationService: any CompanionService { get }
    var sessionMutationConnectionRevision: Int { get }

    func sessionMutationMakeClientMutationID() -> String
    func sessionMutationRollbackState(for sessionID: String) -> ModeRollbackState
    func sessionMutationAssistantSurface(for sessionID: String) -> CompanionAssistantSurface
    func sessionMutationCanSendPrompt(to sessionID: String) -> Bool
    func sessionMutationRejectPrompt(_ message: String)
    func sessionMutationApplyOptimisticMode(_ preset: SessionMode?, to sessionID: String)
    func sessionMutationApplyModeResult(
        _ result: CompanionSessionModeResult,
        sessionID: String
    ) async
    func sessionMutationHandleModeFailure(
        _ error: Error,
        sessionID: String,
        rollbackState: ModeRollbackState?
    ) -> Bool
    func sessionMutationSetPromptMutating(_ isMutating: Bool, sessionID: String)
    func sessionMutationApplyPromptResult(
        _ result: CompanionPromptSendResult,
        sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) async
    func sessionMutationHandlePromptFailure(_ error: Error, sessionID: String) -> Bool
}

@MainActor
final class CompanionSessionMutationCoordinator {
    private let commandStore: (any CompanionSessionCommandLocalStore)?
    private let modeMutationQueue = ClientModeMutationQueue()
    private weak var delegate: CompanionSessionMutationCoordinatorDelegate?

    private var modeMutationDrainTasksBySessionID: [String: Task<Bool, Never>] = [:]
    private var modeMutationDrainIDBySessionID: [String: String] = [:]
    private var modeRollbackStateBySessionID: [String: ModeRollbackState] = [:]
    private var modeMutationBarriersByID: [String: LocalFirstMutationBarrier] = [:]

    #if DEBUG
    private var modeDrainBeforeFinishHook: (() async -> Void)?
    #endif

    init(
        commandStore: (any CompanionSessionCommandLocalStore)?,
        delegate: CompanionSessionMutationCoordinatorDelegate
    ) {
        self.commandStore = commandStore
        self.delegate = delegate
    }

    func applyMode(_ preset: SessionMode?, to sessionID: String) async {
        let drainTask = beginApplyMode(preset, to: sessionID)
        _ = await drainTask.value
    }

    @discardableResult
    func beginApplyMode(_ preset: SessionMode?, to sessionID: String) -> Task<Bool, Never> {
        guard let delegate else {
            return Task.detached { false }
        }

        if modeRollbackStateBySessionID[sessionID] == nil {
            modeRollbackStateBySessionID[sessionID] = delegate.sessionMutationRollbackState(
                for: sessionID
            )
        }

        let clientMutationID = delegate.sessionMutationMakeClientMutationID()
        let barrier = LocalFirstMutationBarrier()
        let enqueueResult: ClientModeMutationEnqueueResult
        do {
            enqueueResult = try modeMutationQueue.enqueueModeMutation(
                sessionId: sessionID,
                preset: preset?.rawValue ?? "",
                clientMutationId: clientMutationID
            )
        } catch {
            CompanionDiagnostics.record(
                "mode:queue-enqueue-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
            Task {
                await barrier.resolve(false)
            }
            return modeMutationBarrierTask(barrier)
        }

        modeMutationBarriersByID[clientMutationID] = barrier
        delegate.sessionMutationApplyOptimisticMode(preset, to: sessionID)
        commandStore?.enqueueModeCommand(
            sessionID: sessionID,
            preset: preset,
            clientMutationID: clientMutationID
        )

        guard enqueueResult.shouldStartDrain else {
            return modeMutationBarrierTask(barrier)
        }

        startModeMutationDrainIfPossible(enqueueResult.mutation)
        return modeMutationBarrierTask(barrier)
    }

    private func startModeMutationDrainIfPossible(_ mutation: ClientModeMutation) {
        guard let delegate else {
            Task {
                await resolveModeMutationBarrier(
                    clientMutationID: mutation.clientMutationId,
                    accepted: false
                )
            }
            return
        }

        let drainID = delegate.sessionMutationMakeClientMutationID()
        do {
            try modeMutationQueue.startModeDrain(
                sessionId: mutation.sessionId,
                drainId: drainID
            )
        } catch {
            CompanionDiagnostics.record(
                "mode:drain-start-failed sessionID=\(mutation.sessionId) error=\(error.localizedDescription)"
            )
            Task {
                await resolveModeMutationBarrier(
                    clientMutationID: mutation.clientMutationId,
                    accepted: false
                )
            }
            return
        }

        guard let envelope = makeModeMutationEnvelope(mutation) else {
            Task {
                await resolveModeMutationBarrier(
                    clientMutationID: mutation.clientMutationId,
                    accepted: false
                )
            }
            return
        }

        let drainTask = makeModeMutationDrainTask(first: envelope, drainID: drainID)
        modeMutationDrainTasksBySessionID[mutation.sessionId] = drainTask
        modeMutationDrainIDBySessionID[mutation.sessionId] = drainID
    }

    @discardableResult
    func sendSessionPrompt(_ prompt: String, to sessionID: String) async -> Bool {
        let promptTask = beginSendSessionPrompt(prompt, to: sessionID)
        return await promptTask.value
    }

    @discardableResult
    func beginSendSessionPrompt(_ prompt: String, to sessionID: String) -> Task<Bool, Never> {
        guard let delegate else {
            return Task.detached { false }
        }

        let targetSurface = delegate.sessionMutationAssistantSurface(for: sessionID)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPrompt.isEmpty else {
            delegate.sessionMutationRejectPrompt("Prompt is required.")
            return Task.detached { false }
        }

        guard delegate.sessionMutationCanSendPrompt(to: sessionID) else {
            return Task.detached { false }
        }

        let clientMutationID = delegate.sessionMutationMakeClientMutationID()
        let service = delegate.sessionMutationService
        let pendingModeMutation = service.supportsModePromptBatch
            ? latestModeMutation(for: sessionID)
            : nil
        let modeBarrierTask = pendingModeMutation.flatMap { mutation in
            modeMutationBarriersByID[mutation.clientMutationId].map(modeMutationBarrierTask)
        }
        commandStore?.enqueuePromptCommand(
            sessionID: sessionID,
            prompt: trimmedPrompt,
            assistantSurface: targetSurface,
            clientMutationID: clientMutationID
        )

        delegate.sessionMutationSetPromptMutating(true, sessionID: sessionID)
        let envelope = PromptMutationEnvelope(
            sessionID: sessionID,
            prompt: trimmedPrompt,
            assistantSurface: targetSurface,
            clientMutationID: clientMutationID,
            connectionRevision: delegate.sessionMutationConnectionRevision,
            service: service,
            pendingModeMutation: pendingModeMutation,
            modeBarrierTask: modeBarrierTask
        )

        return Task.detached(priority: .userInitiated) { [weak self] in
            await self?.sendPromptMutation(envelope) ?? false
        }
    }

    func resolveModeMutationBarriers(_ accepted: Bool) async {
        let barriers = Array(modeMutationBarriersByID.values)
        modeMutationBarriersByID = [:]
        for barrier in barriers {
            await barrier.resolve(accepted)
        }
    }

    func cancelAllModeMutations(resolveAs accepted: Bool) async {
        modeMutationDrainTasksBySessionID.values.forEach { task in
            task.cancel()
        }
        await resolveModeMutationBarriers(accepted)
        modeMutationDrainTasksBySessionID = [:]
        modeMutationDrainIDBySessionID = [:]
        modeRollbackStateBySessionID = [:]
        do {
            try modeMutationQueue.clear()
        } catch {
            CompanionDiagnostics.record(
                "mode:queue-clear-failed error=\(error.localizedDescription)"
            )
        }
    }

    #if DEBUG
    func setModeDrainBeforeFinishHookForSelfTest(_ hook: (() async -> Void)?) {
        modeDrainBeforeFinishHook = hook
    }
    #endif

    private func modeMutationBarrierTask(
        _ barrier: LocalFirstMutationBarrier
    ) -> Task<Bool, Never> {
        Task.detached(priority: .userInitiated) {
            await barrier.wait()
        }
    }

    private func resolveModeMutationBarrier(
        clientMutationID: String,
        accepted: Bool
    ) async {
        let barrier = modeMutationBarriersByID.removeValue(forKey: clientMutationID)
        await barrier?.resolve(accepted)
    }

    private func makeModeMutationDrainTask(
        first envelope: ModeMutationEnvelope,
        drainID: String
    ) -> Task<Bool, Never> {
        Task.detached(priority: .userInitiated) { [weak self] in
            var nextEnvelope: ModeMutationEnvelope? = envelope
            var didAcceptLatestMutation = true
            while !Task.isCancelled, let currentEnvelope = nextEnvelope {
                let didAcceptMutation = await self?.sendModeMutation(currentEnvelope) ?? false
                await self?.resolveModeMutationBarrier(
                    clientMutationID: currentEnvelope.clientMutationID,
                    accepted: didAcceptMutation
                )
                if !didAcceptMutation {
                    didAcceptLatestMutation = false
                    break
                }
                nextEnvelope = await self?.nextModeMutationEnvelope(for: currentEnvelope.sessionID)
            }
            if Task.isCancelled, let unresolvedEnvelope = nextEnvelope {
                await self?.resolveModeMutationBarrier(
                    clientMutationID: unresolvedEnvelope.clientMutationID,
                    accepted: false
                )
            }
            #if DEBUG
            await self?.runModeDrainBeforeFinishHookIfNeeded()
            #endif
            await self?.finishModeMutationDrain(for: envelope.sessionID, drainID: drainID)
            return didAcceptLatestMutation && !Task.isCancelled
        }
    }

    private func nextModeMutationEnvelope(for sessionID: String) async -> ModeMutationEnvelope? {
        let nextMutation: ClientModeMutationOption
        do {
            nextMutation = try modeMutationQueue.takeNextModeMutation(sessionId: sessionID)
        } catch {
            CompanionDiagnostics.record(
                "mode:next-mutation-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
            return nil
        }

        guard nextMutation.hasMutation else {
            return nil
        }

        guard let envelope = makeModeMutationEnvelope(nextMutation.mutation) else {
            await resolveModeMutationBarrier(
                clientMutationID: nextMutation.mutation.clientMutationId,
                accepted: false
            )
            return nil
        }
        return envelope
    }

    private func finishModeMutationDrain(for sessionID: String, drainID: String) async {
        guard modeMutationDrainIDBySessionID[sessionID] == drainID else {
            CompanionDiagnostics.record("mode:stale-drain-finish-skip sessionID=\(sessionID)")
            return
        }

        let finish: ClientModeMutationDrainFinish
        do {
            finish = try modeMutationQueue.finishModeDrain(sessionId: sessionID, drainId: drainID)
        } catch {
            CompanionDiagnostics.record(
                "mode:drain-finish-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
            return
        }

        guard !finish.isStale else {
            CompanionDiagnostics.record("mode:stale-drain-finish-skip sessionID=\(sessionID)")
            return
        }

        modeMutationDrainTasksBySessionID[sessionID] = nil
        modeMutationDrainIDBySessionID[sessionID] = nil
        if finish.shouldClearRollback {
            modeRollbackStateBySessionID[sessionID] = nil
        }
        if finish.hasNextMutation {
            startModeMutationDrainIfPossible(finish.nextMutation)
        }
    }

    #if DEBUG
    private func runModeDrainBeforeFinishHookIfNeeded() async {
        guard let hook = modeDrainBeforeFinishHook else {
            return
        }

        modeDrainBeforeFinishHook = nil
        await hook()
    }
    #endif

    private func makeModeMutationEnvelope(_ mutation: ClientModeMutation) -> ModeMutationEnvelope? {
        guard let delegate else {
            return nil
        }

        return ModeMutationEnvelope(
            mutation: mutation,
            connectionRevision: delegate.sessionMutationConnectionRevision,
            rollbackState: modeRollbackStateBySessionID[mutation.sessionId],
            service: delegate.sessionMutationService
        )
    }

    private func latestModeMutation(for sessionID: String) -> ClientModeMutation? {
        do {
            let latest = try modeMutationQueue.latestModeMutation(sessionId: sessionID)
            return latest.hasMutation ? latest.mutation : nil
        } catch {
            CompanionDiagnostics.record(
                "mode:latest-mutation-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
            return nil
        }
    }

    private func isLatestModeMutation(_ envelope: ModeMutationEnvelope) -> Bool {
        do {
            return try modeMutationQueue.isLatestModeMutation(
                sessionId: envelope.sessionID,
                clientMutationId: envelope.clientMutationID
            )
        } catch {
            CompanionDiagnostics.record(
                "mode:latest-check-failed sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func sendModeMutation(_ envelope: ModeMutationEnvelope) async -> Bool {
        guard !Task.isCancelled else {
            return false
        }

        do {
            let result = try await envelope.service.setSessionMode(
                id: envelope.sessionID,
                preset: envelope.mode,
                clientMutationID: envelope.clientMutationID
            )
            guard !Task.isCancelled else {
                return false
            }
            return await handleModeMutationSuccess(result, envelope: envelope)
        } catch {
            guard !Task.isCancelled else {
                return false
            }
            return handleModeMutationFailure(error, envelope: envelope)
        }
    }

    private func handleModeMutationSuccess(
        _ result: CompanionSessionModeResult,
        envelope: ModeMutationEnvelope
    ) async -> Bool {
        guard let delegate else {
            return false
        }

        commandStore?.markCommandDelivered(result.clientMutationID ?? envelope.clientMutationID)
        guard envelope.connectionRevision == delegate.sessionMutationConnectionRevision else {
            CompanionDiagnostics.record("mode:mutation-stale-skip sessionID=\(envelope.sessionID)")
            return false
        }

        guard isLatestModeMutation(envelope) else {
            CompanionDiagnostics.record("mode:mutation-superseded-skip sessionID=\(envelope.sessionID)")
            return true
        }

        await delegate.sessionMutationApplyModeResult(result, sessionID: envelope.sessionID)
        return true
    }

    private func handleModeMutationFailure(
        _ error: Error,
        envelope: ModeMutationEnvelope
    ) -> Bool {
        guard let delegate else {
            return false
        }

        guard envelope.connectionRevision == delegate.sessionMutationConnectionRevision else {
            CompanionDiagnostics.record(
                "mode:mutation-stale-error-skip sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return false
        }

        guard isLatestModeMutation(envelope) else {
            CompanionDiagnostics.record(
                "mode:mutation-superseded-error-skip sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return true
        }

        return delegate.sessionMutationHandleModeFailure(
            error,
            sessionID: envelope.sessionID,
            rollbackState: envelope.rollbackState
        )
    }

    private func sendPromptMutation(_ envelope: PromptMutationEnvelope) async -> Bool {
        if let pendingModeMutation = envelope.pendingModeMutation {
            do {
                let result = try await envelope.service.sendSessionPromptAfterMode(
                    id: envelope.sessionID,
                    modePreset: pendingModeMutation.mode,
                    modeClientMutationID: pendingModeMutation.clientMutationId,
                    prompt: envelope.prompt,
                    assistantSurface: envelope.assistantSurface,
                    promptClientMutationID: envelope.clientMutationID
                )
                return await handleModePromptBatchSuccess(
                    result,
                    pendingModeMutation: pendingModeMutation,
                    envelope: envelope
                )
            } catch {
                CompanionDiagnostics.record(
                    "prompt:mode-batch-failed sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
                )
                return handlePromptMutationFailure(error, envelope: envelope)
            }
        }

        if let modeBarrierTask = envelope.modeBarrierTask {
            let didAcceptMode = await modeBarrierTask.value
            guard didAcceptMode else {
                return handlePromptMutationFailure(
                    LocalFirstMutationError.modeBarrierRejected,
                    envelope: envelope
                )
            }
        }

        do {
            let result = try await envelope.service.sendSessionPrompt(
                id: envelope.sessionID,
                prompt: envelope.prompt,
                assistantSurface: envelope.assistantSurface,
                clientMutationID: envelope.clientMutationID
            )
            return await handlePromptMutationSuccess(result, envelope: envelope)
        } catch {
            return handlePromptMutationFailure(error, envelope: envelope)
        }
    }

    private func handleModePromptBatchSuccess(
        _ result: CompanionModePromptBatchResult,
        pendingModeMutation: ClientModeMutation,
        envelope: PromptMutationEnvelope
    ) async -> Bool {
        commandStore?.markCommandDelivered(
            result.mode.clientMutationID ?? pendingModeMutation.clientMutationId
        )
        await finishModeMutationDeliveredByBatch(
            pendingModeMutation,
            sessionID: envelope.sessionID
        )
        return await handlePromptMutationSuccess(result.prompt, envelope: envelope)
    }

    private func finishModeMutationDeliveredByBatch(
        _ mutation: ClientModeMutation,
        sessionID: String
    ) async {
        await resolveModeMutationBarrier(
            clientMutationID: mutation.clientMutationId,
            accepted: true
        )

        let finish: ClientModeMutationBatchFinish
        do {
            finish = try modeMutationQueue.finishBatchedModeMutation(
                sessionId: sessionID,
                clientMutationId: mutation.clientMutationId
            )
        } catch {
            CompanionDiagnostics.record(
                "mode:batch-finish-failed sessionID=\(sessionID) error=\(error.localizedDescription)"
            )
            return
        }

        if finish.shouldCancelActiveDrain {
            modeMutationDrainTasksBySessionID[sessionID]?.cancel()
            modeMutationDrainTasksBySessionID[sessionID] = nil
            modeMutationDrainIDBySessionID[sessionID] = nil
        }

        if finish.shouldClearRollback {
            modeRollbackStateBySessionID[sessionID] = nil
        }
        if finish.hasNextMutation {
            startModeMutationDrainIfPossible(finish.nextMutation)
        }
    }

    private func handlePromptMutationSuccess(
        _ result: CompanionPromptSendResult,
        envelope: PromptMutationEnvelope
    ) async -> Bool {
        defer {
            delegate?.sessionMutationSetPromptMutating(false, sessionID: envelope.sessionID)
        }

        guard let delegate else {
            return false
        }

        commandStore?.markCommandDelivered(result.clientMutationID ?? envelope.clientMutationID)
        guard envelope.connectionRevision == delegate.sessionMutationConnectionRevision else {
            CompanionDiagnostics.record("prompt:mutation-stale-skip sessionID=\(envelope.sessionID)")
            return false
        }

        await delegate.sessionMutationApplyPromptResult(
            result,
            sessionID: envelope.sessionID,
            assistantSurface: envelope.assistantSurface
        )
        return true
    }

    private func handlePromptMutationFailure(
        _ error: Error,
        envelope: PromptMutationEnvelope
    ) -> Bool {
        defer {
            delegate?.sessionMutationSetPromptMutating(false, sessionID: envelope.sessionID)
        }

        guard let delegate else {
            return false
        }

        guard envelope.connectionRevision == delegate.sessionMutationConnectionRevision else {
            CompanionDiagnostics.record(
                "prompt:mutation-stale-error-skip sessionID=\(envelope.sessionID) error=\(error.localizedDescription)"
            )
            return false
        }

        return delegate.sessionMutationHandlePromptFailure(error, sessionID: envelope.sessionID)
    }
}

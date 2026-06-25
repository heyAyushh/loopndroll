import Foundation

private enum LocalFirstMutationError: LocalizedError {
    case modeBarrierRejected

    var errorDescription: String? {
        switch self {
        case .modeBarrierRejected:
            return "Mode change was not accepted. Prompt stayed queued."
        }
    }
}

enum PendingSessionModeSelection: Sendable {
    case globalDefault
    case preset(SessionMode)

    var mode: SessionMode? {
        switch self {
        case .globalDefault:
            return nil
        case let .preset(mode):
            return mode
        }
    }

    init(_ mode: SessionMode?) {
        if let mode {
            self = .preset(mode)
        } else {
            self = .globalDefault
        }
    }
}

struct ModeRollbackState: Sendable {
    let snapshot: MobileSnapshot?
    let detail: SessionDetail?
}

private struct PendingSessionModeMutation: Sendable {
    let selection: PendingSessionModeSelection
    let clientMutationID: String
    let barrier: LocalFirstMutationBarrier

    var mode: SessionMode? {
        selection.mode
    }

    init(
        mode: SessionMode?,
        clientMutationID: String
    ) {
        selection = PendingSessionModeSelection(mode)
        self.clientMutationID = clientMutationID
        barrier = LocalFirstMutationBarrier()
    }
}

private struct ModeMutationEnvelope: Sendable {
    let sessionID: String
    let selection: PendingSessionModeSelection
    let clientMutationID: String
    let barrier: LocalFirstMutationBarrier
    let connectionRevision: Int
    let rollbackState: ModeRollbackState?
    let service: any CompanionService

    var mode: SessionMode? {
        selection.mode
    }
}

private struct PromptMutationEnvelope: Sendable {
    let sessionID: String
    let prompt: String
    let assistantSurface: CompanionAssistantSurface
    let clientMutationID: String
    let connectionRevision: Int
    let service: any CompanionService
    let pendingModeMutation: PendingSessionModeMutation?
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
    func sessionMutationEnqueueModeCommand(
        sessionID: String,
        preset: SessionMode?,
        clientMutationID: String
    )
    func sessionMutationEnqueuePromptCommand(
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface,
        clientMutationID: String
    )
    func sessionMutationMarkCommandDelivered(_ clientMutationID: String?)
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
    private weak var delegate: CompanionSessionMutationCoordinatorDelegate?

    private var modeMutationDrainTasksBySessionID: [String: Task<Bool, Never>] = [:]
    private var modeMutationDrainIDBySessionID: [String: String] = [:]
    private var pendingModeMutationsBySessionID: [String: [PendingSessionModeMutation]] = [:]
    private var modeRollbackStateBySessionID: [String: ModeRollbackState] = [:]
    private var latestModeMutationBySessionID: [String: PendingSessionModeMutation] = [:]
    private var latestModeMutationIDBySessionID: [String: String] = [:]
    private var latestModeMutationBarrierBySessionID: [String: LocalFirstMutationBarrier] = [:]

    #if DEBUG
    private var modeDrainBeforeFinishHook: (() async -> Void)?
    #endif

    init(delegate: CompanionSessionMutationCoordinatorDelegate) {
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
        let pendingMutation = PendingSessionModeMutation(
            mode: preset,
            clientMutationID: clientMutationID
        )
        delegate.sessionMutationApplyOptimisticMode(preset, to: sessionID)
        delegate.sessionMutationEnqueueModeCommand(
            sessionID: sessionID,
            preset: preset,
            clientMutationID: clientMutationID
        )
        latestModeMutationBySessionID[sessionID] = pendingMutation
        latestModeMutationIDBySessionID[sessionID] = clientMutationID
        latestModeMutationBarrierBySessionID[sessionID] = pendingMutation.barrier

        if modeMutationDrainTasksBySessionID[sessionID] != nil {
            pendingModeMutationsBySessionID[sessionID, default: []].append(pendingMutation)
            return modeMutationBarrierTask(pendingMutation.barrier)
        }

        guard let envelope = makeModeMutationEnvelope(pendingMutation, sessionID: sessionID) else {
            Task {
                await pendingMutation.barrier.resolve(false)
            }
            return modeMutationBarrierTask(pendingMutation.barrier)
        }
        let drainID = delegate.sessionMutationMakeClientMutationID()
        let drainTask = makeModeMutationDrainTask(first: envelope, drainID: drainID)
        modeMutationDrainTasksBySessionID[sessionID] = drainTask
        modeMutationDrainIDBySessionID[sessionID] = drainID
        return modeMutationBarrierTask(pendingMutation.barrier)
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
            ? latestModeMutationBySessionID[sessionID]
            : nil
        let modeBarrierTask = pendingModeMutation.map {
            modeMutationBarrierTask($0.barrier)
        }
        delegate.sessionMutationEnqueuePromptCommand(
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
        let barriers = Array(latestModeMutationBarrierBySessionID.values)
            + pendingModeMutationsBySessionID.values.flatMap { mutations in
                mutations.map(\.barrier)
            }
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
        pendingModeMutationsBySessionID = [:]
        modeRollbackStateBySessionID = [:]
        latestModeMutationBySessionID = [:]
        latestModeMutationIDBySessionID = [:]
        latestModeMutationBarrierBySessionID = [:]
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

    private func makeModeMutationDrainTask(
        first envelope: ModeMutationEnvelope,
        drainID: String
    ) -> Task<Bool, Never> {
        Task.detached(priority: .userInitiated) { [weak self] in
            var nextEnvelope: ModeMutationEnvelope? = envelope
            var didAcceptLatestMutation = true
            while !Task.isCancelled, let currentEnvelope = nextEnvelope {
                let didAcceptMutation = await self?.sendModeMutation(currentEnvelope) ?? false
                await currentEnvelope.barrier.resolve(didAcceptMutation)
                if !didAcceptMutation {
                    didAcceptLatestMutation = false
                    break
                }
                nextEnvelope = await self?.nextModeMutationEnvelope(for: currentEnvelope.sessionID)
            }
            if Task.isCancelled, let unresolvedEnvelope = nextEnvelope {
                await unresolvedEnvelope.barrier.resolve(false)
            }
            #if DEBUG
            await self?.runModeDrainBeforeFinishHookIfNeeded()
            #endif
            await self?.finishModeMutationDrain(for: envelope.sessionID, drainID: drainID)
            return didAcceptLatestMutation && !Task.isCancelled
        }
    }

    private func nextModeMutationEnvelope(for sessionID: String) async -> ModeMutationEnvelope? {
        guard var pendingMutations = pendingModeMutationsBySessionID[sessionID],
              !pendingMutations.isEmpty
        else {
            return nil
        }

        let mutation = pendingMutations.removeFirst()
        pendingModeMutationsBySessionID[sessionID] = pendingMutations.isEmpty ? nil : pendingMutations
        guard let envelope = makeModeMutationEnvelope(mutation, sessionID: sessionID) else {
            await mutation.barrier.resolve(false)
            return nil
        }
        return envelope
    }

    private func finishModeMutationDrain(for sessionID: String, drainID: String) async {
        guard modeMutationDrainIDBySessionID[sessionID] == drainID else {
            CompanionDiagnostics.record("mode:stale-drain-finish-skip sessionID=\(sessionID)")
            return
        }

        modeMutationDrainTasksBySessionID[sessionID] = nil
        modeMutationDrainIDBySessionID[sessionID] = nil
        guard let nextEnvelope = await nextModeMutationEnvelope(for: sessionID) else {
            modeRollbackStateBySessionID[sessionID] = nil
            latestModeMutationBySessionID[sessionID] = nil
            latestModeMutationIDBySessionID[sessionID] = nil
            latestModeMutationBarrierBySessionID[sessionID] = nil
            return
        }

        guard let delegate else {
            await nextEnvelope.barrier.resolve(false)
            return
        }

        let nextDrainID = delegate.sessionMutationMakeClientMutationID()
        let drainTask = makeModeMutationDrainTask(first: nextEnvelope, drainID: nextDrainID)
        modeMutationDrainTasksBySessionID[sessionID] = drainTask
        modeMutationDrainIDBySessionID[sessionID] = nextDrainID
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

    private func makeModeMutationEnvelope(
        _ mutation: PendingSessionModeMutation,
        sessionID: String
    ) -> ModeMutationEnvelope? {
        guard let delegate else {
            return nil
        }

        return ModeMutationEnvelope(
            sessionID: sessionID,
            selection: mutation.selection,
            clientMutationID: mutation.clientMutationID,
            barrier: mutation.barrier,
            connectionRevision: delegate.sessionMutationConnectionRevision,
            rollbackState: modeRollbackStateBySessionID[sessionID],
            service: delegate.sessionMutationService
        )
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
            return await handleModeMutationFailure(error, envelope: envelope)
        }
    }

    private func handleModeMutationSuccess(
        _ result: CompanionSessionModeResult,
        envelope: ModeMutationEnvelope
    ) async -> Bool {
        guard let delegate else {
            return false
        }

        delegate.sessionMutationMarkCommandDelivered(result.clientMutationID ?? envelope.clientMutationID)
        guard envelope.connectionRevision == delegate.sessionMutationConnectionRevision else {
            CompanionDiagnostics.record("mode:mutation-stale-skip sessionID=\(envelope.sessionID)")
            return false
        }

        guard latestModeMutationIDBySessionID[envelope.sessionID] == envelope.clientMutationID else {
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

        guard latestModeMutationIDBySessionID[envelope.sessionID] == envelope.clientMutationID else {
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
                    modeClientMutationID: pendingModeMutation.clientMutationID,
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
                return await handlePromptMutationFailure(error, envelope: envelope)
            }
        }

        if let modeBarrierTask = envelope.modeBarrierTask {
            let didAcceptMode = await modeBarrierTask.value
            guard didAcceptMode else {
                return await handlePromptMutationFailure(
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
            return await handlePromptMutationFailure(error, envelope: envelope)
        }
    }

    private func handleModePromptBatchSuccess(
        _ result: CompanionModePromptBatchResult,
        pendingModeMutation: PendingSessionModeMutation,
        envelope: PromptMutationEnvelope
    ) async -> Bool {
        delegate?.sessionMutationMarkCommandDelivered(
            result.mode.clientMutationID ?? pendingModeMutation.clientMutationID
        )
        await finishModeMutationDeliveredByBatch(
            pendingModeMutation,
            sessionID: envelope.sessionID
        )
        return await handlePromptMutationSuccess(result.prompt, envelope: envelope)
    }

    private func finishModeMutationDeliveredByBatch(
        _ mutation: PendingSessionModeMutation,
        sessionID: String
    ) async {
        await mutation.barrier.resolve(true)

        if var pendingMutations = pendingModeMutationsBySessionID[sessionID] {
            pendingMutations.removeAll { pendingMutation in
                pendingMutation.clientMutationID == mutation.clientMutationID
            }
            pendingModeMutationsBySessionID[sessionID] = pendingMutations.isEmpty
                ? nil
                : pendingMutations
        }

        let batchedMutationWasLatest =
            latestModeMutationIDBySessionID[sessionID] == mutation.clientMutationID
        if batchedMutationWasLatest {
            latestModeMutationBySessionID[sessionID] = nil
            latestModeMutationIDBySessionID[sessionID] = nil
            latestModeMutationBarrierBySessionID[sessionID] = nil
        }

        modeMutationDrainTasksBySessionID[sessionID]?.cancel()
        modeMutationDrainTasksBySessionID[sessionID] = nil
        modeMutationDrainIDBySessionID[sessionID] = nil

        guard let nextEnvelope = await nextModeMutationEnvelope(for: sessionID) else {
            if batchedMutationWasLatest {
                modeRollbackStateBySessionID[sessionID] = nil
            }
            return
        }

        guard let delegate else {
            await nextEnvelope.barrier.resolve(false)
            return
        }

        let nextDrainID = delegate.sessionMutationMakeClientMutationID()
        modeMutationDrainTasksBySessionID[sessionID] = makeModeMutationDrainTask(
            first: nextEnvelope,
            drainID: nextDrainID
        )
        modeMutationDrainIDBySessionID[sessionID] = nextDrainID
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

        delegate.sessionMutationMarkCommandDelivered(result.clientMutationID ?? envelope.clientMutationID)
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

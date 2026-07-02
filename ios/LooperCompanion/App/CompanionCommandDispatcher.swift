import Foundation
import LooperClientCore
import LooperCompanionCore

@MainActor
protocol CompanionCommandDispatcherDelegate: AnyObject {
    var commandDispatcherSessionMiniController: CompanionSessionMiniController { get }
    var commandDispatcherSnapshotState: CompanionSnapshotStateStore { get }
    var commandDispatcherSnapshot: MobileSnapshot? { get }
    var commandDispatcherServerHealth: CompanionServerHealth? { get }
    var commandDispatcherReachedBaseURL: URL? { get }
    var commandDispatcherConnectionState: ConnectivityState { get set }
    var commandDispatcherErrorMessage: String? { get set }

    func commandDispatcherSetLastUpdatedAt(_ date: Date)
    func commandDispatcherClearConnectionRouteStateIfNeeded(for state: ConnectivityState)
    func commandDispatcherSessionAuthoritativeConnectionState(
        _ projectedState: ConnectivityState
    ) -> ConnectivityState
    func commandDispatcherSessionAuthoritativeErrorMessage(
        shouldSuppressProjectionError: Bool,
        error: Error
    ) -> String?
    func commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: String) -> Bool
    func commandDispatcherAssistantSurface(for sessionID: String) -> CompanionAssistantSurface
    func commandDispatcherDonateSetDefaultSiriSession(_ session: SessionSummary) async
    func commandDispatcherStartNotificationReplyOutboxDrainIfNeeded()
}

@MainActor
final class CompanionCommandDispatcher {
    private weak var delegate: CompanionCommandDispatcherDelegate?

    init(delegate: CompanionCommandDispatcherDelegate) {
        self.delegate = delegate
    }

    func applyMode(_ preset: SessionMode?, to sessionID: String) async {
        _ = await applyModeIntent(preset, to: sessionID)
    }

    @discardableResult
    func beginApplyMode(_ preset: SessionMode?, to sessionID: String) -> Task<Bool, Never> {
        Task { @MainActor [weak self] in
            await self?.applyModeIntent(preset, to: sessionID) ?? false
        }
    }

    func setSessionArchived(_ archived: Bool, sessionID: String) async {
        guard let targetRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: true
            )
            Haptics.error()
            return
        }

        do {
            try await targetRuntime.setSessionArchived(
                threadID: sessionID,
                archived: archived
            )
            _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "archive")
            delegate?.commandDispatcherErrorMessage = nil
            delegate?.commandDispatcherSetLastUpdatedAt(Date())
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        CompanionDiagnostics.record("archive:client-core-owned sessionID=\(sessionID) archived=\(archived)")
    }

    func deleteSession(_ sessionID: String) async {
        guard let targetRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: true
            )
            Haptics.error()
            return
        }

        do {
            try await targetRuntime.deleteSession(threadID: sessionID)
            _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "delete")
            delegate?.commandDispatcherErrorMessage = nil
            delegate?.commandDispatcherSetLastUpdatedAt(Date())
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }

        CompanionDiagnostics.record("delete:client-core-owned sessionID=\(sessionID)")
    }

    @discardableResult
    func sendSessionPrompt(
        _ prompt: String,
        intent: CompanionPromptIntent = .steer,
        to sessionID: String
    ) async -> Bool {
        await sendPromptIntent(prompt, intent: intent, to: sessionID)
    }

    @discardableResult
    func beginSendSessionPrompt(
        _ prompt: String,
        intent: CompanionPromptIntent = .steer,
        to sessionID: String
    ) -> Task<Bool, Never> {
        Task { @MainActor [weak self] in
            await self?.sendPromptIntent(prompt, intent: intent, to: sessionID) ?? false
        }
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        prompt: String,
        to sessionID: String
    ) async -> Bool {
        let trimmedNotificationID = notificationID.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedNotificationID.isEmpty else {
            rejectNotificationReply("Notification reply is missing its delivery ID.")
            return false
        }
        guard !trimmedPrompt.isEmpty else {
            rejectNotificationReply("Prompt is required.")
            return false
        }

        return await submitNotificationReplyCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt
        )
    }

    @discardableResult
    func submitPendingNotificationReply() async -> Bool {
        guard let sessionRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            CompanionDiagnostics.record("notification-reply:pending-drain-missing-session-runtime")
            return false
        }

        do {
            let response = try await sessionRuntime.submitPendingNotificationReply()
            _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "notification-reply-pending")
            recordNotificationReplyAccepted(
                response,
                sessionID: Self.nonEmptyText(response.entityId) ?? "unknown",
                notificationID: Self.nonEmptyText(response.notificationId) ?? "unknown"
            )
            return true
        } catch ClientCoreError.NoPendingNotificationReply {
            CompanionDiagnostics.record("notification-reply:pending-drain-empty")
            return false
        } catch {
            CompanionDiagnostics.record(
                "notification-reply:pending-drain-failed error=\(error.localizedDescription)"
            )
            return false
        }
    }

    func muteSession(_ sessionID: String) async {
        guard let targetRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: true
            )
            Haptics.error()
            return
        }

        do {
            try await targetRuntime.muteSession(threadID: sessionID)
            _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "mute")
            delegate?.commandDispatcherErrorMessage = nil
            delegate?.commandDispatcherSetLastUpdatedAt(Date())
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return
        }
    }

    @discardableResult
    func setSiriDefaultSession(_ session: SessionSummary) async -> Bool {
        await setSiriDefaultSession(session.id)
    }

    @discardableResult
    func setSiriDefaultSession(
        _ sessionID: String,
        assistantSurface requestedSurface: CompanionAssistantSurface? = nil
    ) async -> Bool {
        guard let delegate else {
            return false
        }

        let targetSurface = requestedSurface ?? delegate.commandDispatcherAssistantSurface(for: sessionID)
        guard let targetRuntime = delegate.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: true
            )
            Haptics.error()
            return false
        }

        do {
            try await targetRuntime.setSiriDefaultSession(
                threadID: sessionID,
                assistantSurface: targetSurface
            )
            delegate.commandDispatcherSnapshotState.applyAcceptedSiriDefaultSession(
                sessionID: sessionID,
                assistantSurface: targetSurface
            )
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            Haptics.error()
            return false
        }

        delegate.commandDispatcherErrorMessage = nil
        delegate.commandDispatcherSetLastUpdatedAt(Date())
        CompanionDiagnostics.record("siri-default:client-core-owned sessionID=\(sessionID)")
        Haptics.success()
        if let session = delegate.commandDispatcherSnapshotState.session(
            withID: sessionID,
            assistantSurface: targetSurface
        ) {
            await delegate.commandDispatcherDonateSetDefaultSiriSession(session)
        }
        return true
    }

    @discardableResult
    func markCurrentSiriSession(_ session: SessionSummary) async -> Bool {
        await markCurrentSiriSession(session.id)
    }

    @discardableResult
    func markCurrentSiriSession(
        _ sessionID: String,
        assistantSurface requestedSurface: CompanionAssistantSurface? = nil
    ) async -> Bool {
        guard let delegate else {
            return false
        }

        let targetSurface = requestedSurface ?? delegate.commandDispatcherAssistantSurface(for: sessionID)
        guard let targetRuntime = delegate.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: true
            )
            return false
        }

        do {
            try await targetRuntime.setSiriCurrentSession(
                threadID: sessionID,
                assistantSurface: targetSurface
            )
            delegate.commandDispatcherSnapshotState.applyAcceptedSiriCurrentSession(
                sessionID: sessionID,
                assistantSurface: targetSurface
            )
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            return false
        }

        delegate.commandDispatcherErrorMessage = nil
        delegate.commandDispatcherSetLastUpdatedAt(Date())
        CompanionDiagnostics.record("siri-current:client-core-owned sessionID=\(sessionID)")
        return true
    }

    @discardableResult
    func saveDefaultPrompt(_ defaultPrompt: String) async -> Bool {
        guard let delegate else {
            return false
        }
        guard let targetRuntime = delegate.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: true
            )
            return false
        }

        do {
            try await targetRuntime.saveDefaultPrompt(defaultPrompt)
            delegate.commandDispatcherSnapshotState.applyAcceptedDefaultPrompt(defaultPrompt)
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: true)
            return false
        }

        delegate.commandDispatcherErrorMessage = nil
        delegate.commandDispatcherSetLastUpdatedAt(Date())
        CompanionDiagnostics.record("default-prompt:client-core-owned")
        return true
    }

    func applyConnectionFailure(
        _ error: Error,
        suppressErrorWhenSnapshotUsable: Bool
    ) {
        guard let delegate else {
            return
        }

        let mappedErrorState = Self.connectionState(for: error)
        if suppressErrorWhenSnapshotUsable {
            let projection = reduceSnapshotLoadFailureOrCrash(
                mappedErrorState: mappedErrorState,
                currentState: delegate.commandDispatcherConnectionState,
                hasUsableSnapshot: delegate.commandDispatcherSnapshot != nil,
                hasServerHealth: delegate.commandDispatcherServerHealth != nil,
                hasReachedBaseURL: delegate.commandDispatcherReachedBaseURL != nil
            )
            if projection.preservedConnectedState {
                CompanionDiagnostics.record(
                    "connection:local-state-preserved error=\(error.localizedDescription)"
                )
            }
            let nextConnectionState = delegate.commandDispatcherSessionAuthoritativeConnectionState(
                connectionState(rawValue: projection.connectionState)
            )
            delegate.commandDispatcherConnectionState = nextConnectionState
            delegate.commandDispatcherClearConnectionRouteStateIfNeeded(for: nextConnectionState)
            delegate.commandDispatcherErrorMessage = delegate.commandDispatcherSessionAuthoritativeErrorMessage(
                shouldSuppressProjectionError: projection.shouldSuppressError,
                error: error
            )
            return
        }

        let projection = reduceConnectionFailureOrCrash(
            mappedErrorState: mappedErrorState,
            hasUsableSnapshot: delegate.commandDispatcherSnapshot != nil,
            suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
        )
        let nextConnectionState = delegate.commandDispatcherSessionAuthoritativeConnectionState(
            connectionState(rawValue: projection.connectionState)
        )
        delegate.commandDispatcherConnectionState = nextConnectionState
        delegate.commandDispatcherClearConnectionRouteStateIfNeeded(for: nextConnectionState)
        delegate.commandDispatcherErrorMessage = delegate.commandDispatcherSessionAuthoritativeErrorMessage(
            shouldSuppressProjectionError: projection.shouldSuppressError,
            error: error
        )
    }

    static func connectionState(for error: Error) -> ConnectivityState {
        if error is CompanionConfigurationError {
            return .unpaired
        }

        if let httpError = error as? HTTPCompanionServiceError {
            switch httpError {
            case .unauthorized:
                return .unauthorized
            case .passkeySessionRequired:
                return .locked
            case .invalidResponse, .localStoreUnavailable, .serverError:
                return .offline
            }
        }

        return .offline
    }

    private func applyModeIntent(_ preset: SessionMode?, to sessionID: String) async -> Bool {
        guard let targetRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: false
            )
            Haptics.error()
            return false
        }

        do {
            let result = try await targetRuntime.setMode(
                threadID: sessionID,
                preset: preset
            )
            if !applyAcceptedModeProjection(preset, sessionID: sessionID) {
                _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "mode")
            }
            recordModeAccepted(result, sessionID: sessionID)
            return true
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }
    }

    @discardableResult
    private func applyAcceptedModeProjection(_ preset: SessionMode?, sessionID: String) -> Bool {
        guard let delegate,
              let sourceSnapshot = delegate.commandDispatcherSnapshotState.sourceSnapshot
        else {
            return false
        }
        do {
            let projection = try reduceMobileSnapshotOptimisticMode(
                snapshot: sourceSnapshot.clientCoreSnapshot,
                sessionId: sessionID,
                preset: preset?.rawValue ?? "",
                selectedAssistantSurface: delegate.commandDispatcherSnapshotState.selectedAssistantSurface.rawValue
            )
            guard projection.didUpdate else {
                return false
            }
            let visibleSnapshot = MobileSnapshot(clientCore: projection.visibleSnapshot)
            delegate.commandDispatcherSnapshotState.applyOptimisticVisibleSnapshot(
                visibleSnapshot,
                selectedSurface: delegate.commandDispatcherSnapshotState.selectedAssistantSurface
            )
            delegate.commandDispatcherSetLastUpdatedAt(Date())
            return true
        } catch {
            CompanionDiagnostics.record(
                "mode:optimistic-projection-failed id=\(sessionID) error=\(error.localizedDescription)"
            )
            return false
        }
    }

    private func recordModeAccepted(
        _ result: ClientSessionModeIntentResult,
        sessionID: String
    ) {
        delegate?.commandDispatcherErrorMessage = nil
        delegate?.commandDispatcherSetLastUpdatedAt(Date())
        let acceptedMode = result.preset.trimmingCharacters(in: .whitespacesAndNewlines)
        CompanionDiagnostics.record(
            "mode:accepted sessionID=\(sessionID) mode=\(acceptedMode.isEmpty ? "unset" : acceptedMode)"
        )
    }

    private func sendPromptIntent(
        _ prompt: String,
        intent: CompanionPromptIntent,
        to sessionID: String
    ) async -> Bool {
        let targetSurface = delegate?.commandDispatcherAssistantSurface(for: sessionID)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedPrompt.isEmpty else {
            delegate?.commandDispatcherErrorMessage = "Prompt is required."
            Haptics.warning()
            return false
        }

        guard let targetRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            applyConnectionFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                suppressErrorWhenSnapshotUsable: false
            )
            Haptics.error()
            return false
        }

        do {
            let result = try await targetRuntime.sendPrompt(
                threadID: sessionID,
                prompt: trimmedPrompt,
                assistantSurface: targetSurface,
                promptIntent: intent
            )
            _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "prompt")
            recordPromptAccepted(
                result,
                sessionID: sessionID
            )
            return true
        } catch {
            applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
            Haptics.error()
            return false
        }
    }

    @discardableResult
    private func submitNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String
    ) async -> Bool {
        guard let sessionRuntime = delegate?.commandDispatcherSessionMiniController.sessionRuntime else {
            applyNotificationReplyFailure(
                HTTPCompanionServiceError.localStoreUnavailable,
                sessionID: sessionID,
                notificationID: notificationID
            )
            delegate?.commandDispatcherStartNotificationReplyOutboxDrainIfNeeded()
            return false
        }

        do {
            let response = try await sessionRuntime.submitNotificationReply(
                notificationID: notificationID,
                threadID: sessionID,
                prompt: prompt,
                assistantSurface: nil
            )
            _ = delegate?.commandDispatcherApplyAcceptedClientCoreLocalSnapshot(reason: "notification-reply")
            recordNotificationReplyAccepted(
                response,
                sessionID: sessionID,
                notificationID: notificationID
            )
            return true
        } catch {
            applyNotificationReplyFailure(
                error,
                sessionID: sessionID,
                notificationID: notificationID
            )
            delegate?.commandDispatcherStartNotificationReplyOutboxDrainIfNeeded()
            return false
        }
    }

    private func rejectNotificationReply(_ message: String) {
        delegate?.commandDispatcherErrorMessage = message
        Haptics.warning()
    }

    private func recordNotificationReplyAccepted(
        _ response: ClientNotificationReplyIntentResult,
        sessionID: String,
        notificationID: String
    ) {
        delegate?.commandDispatcherErrorMessage = nil
        delegate?.commandDispatcherSetLastUpdatedAt(Date())
        CompanionDiagnostics.record(
            "notification-reply:accepted sessionID=\(sessionID) notificationID=\(notificationID) kind=\(response.dispatchKind)"
        )
    }

    private func applyNotificationReplyFailure(
        _ error: Error,
        sessionID: String,
        notificationID: String
    ) {
        applyConnectionFailure(error, suppressErrorWhenSnapshotUsable: false)
        Haptics.error()
        CompanionDiagnostics.record(
            "notification-reply:send-failed sessionID=\(sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
        )
    }

    private func recordPromptAccepted(
        _ result: ClientSessionPromptIntentResult,
        sessionID: String
    ) {
        delegate?.commandDispatcherErrorMessage = nil
        delegate?.commandDispatcherSetLastUpdatedAt(Date())
        CompanionDiagnostics.record(
            "prompt:accepted sessionID=\(sessionID) kind=\(Self.nonEmptyText(result.dispatchKind) ?? "unknown")"
        )
    }

    private func reduceSnapshotLoadFailureOrCrash(
        mappedErrorState: ConnectivityState,
        currentState: ConnectivityState,
        hasUsableSnapshot: Bool,
        hasServerHealth: Bool,
        hasReachedBaseURL: Bool
    ) -> ClientSnapshotLoadFailureProjection {
        do {
            return try reduceSnapshotLoadFailure(
                mappedErrorState: mappedErrorState.rawValue,
                currentConnectionState: currentState.rawValue,
                hasUsableSnapshot: hasUsableSnapshot,
                hasServerHealth: hasServerHealth,
                hasReachedBaseUrl: hasReachedBaseURL
            )
        } catch {
            CompanionDiagnostics.record(
                "connection:snapshot-load-projection-failed error=\(error.localizedDescription)"
            )
            return ClientSnapshotLoadFailureProjection(
                connectionState: mappedErrorState.rawValue,
                preservedConnectedState: false,
                shouldClearRouteState: mappedErrorState != .connected,
                shouldSuppressError: hasUsableSnapshot
            )
        }
    }

    private func reduceConnectionFailureOrCrash(
        mappedErrorState: ConnectivityState,
        hasUsableSnapshot: Bool,
        suppressErrorWhenSnapshotUsable: Bool
    ) -> ClientConnectionFailureProjection {
        do {
            return try reduceConnectionFailure(
                mappedErrorState: mappedErrorState.rawValue,
                hasUsableSnapshot: hasUsableSnapshot,
                suppressErrorWhenSnapshotUsable: suppressErrorWhenSnapshotUsable
            )
        } catch {
            CompanionDiagnostics.record(
                "connection:failure-projection-failed error=\(error.localizedDescription)"
            )
            return ClientConnectionFailureProjection(
                connectionState: mappedErrorState.rawValue,
                shouldClearRouteState: mappedErrorState != .connected,
                shouldSuppressError: hasUsableSnapshot && suppressErrorWhenSnapshotUsable
            )
        }
    }

    private func connectionState(rawValue: String) -> ConnectivityState {
        guard let state = ConnectivityState(rawValue: rawValue) else {
            CompanionDiagnostics.record("connection:projection-unknown-state state=\(rawValue)")
            return .offline
        }
        return state
    }

    private static func nonEmptyText(_ value: String?) -> String? {
        guard let value = value?.trimmingCharacters(in: .whitespacesAndNewlines),
              !value.isEmpty
        else {
            return nil
        }

        return value
    }
}

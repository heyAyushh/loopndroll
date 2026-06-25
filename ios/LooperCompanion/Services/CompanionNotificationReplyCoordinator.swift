import Foundation
import LooperRealtime

@MainActor
protocol CompanionNotificationReplyCoordinatorDelegate: AnyObject {
    var notificationReplyService: any CompanionService { get }
    var notificationReplySelectedAssistantSurface: CompanionAssistantSurface { get }

    func notificationReplyMakeClientMutationID() -> String
    func notificationReplyAssistantSurface(for sessionID: String) -> CompanionAssistantSurface?
    func notificationReplyReject(_ message: String)
    func notificationReplyEnqueueCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        clientMutationID: String
    )
    func notificationReplyMarkCommandAttempted(_ clientMutationID: String)
    func notificationReplyMarkCommandDelivered(_ clientMutationID: String?)
    func notificationReplyApplyAccepted(
        _ response: LooperRealtimeNotificationReplyResponse,
        sessionID: String,
        notificationID: String,
        targetSurface: CompanionAssistantSurface
    ) async
    func notificationReplyApplyFailure(
        _ error: Error,
        sessionID: String,
        notificationID: String
    )
}

@MainActor
final class CompanionNotificationReplyCoordinator {
    private let sessionMiniController: CompanionSessionMiniController
    private weak var delegate: CompanionNotificationReplyCoordinatorDelegate?

    init(
        sessionMiniController: CompanionSessionMiniController,
        delegate: CompanionNotificationReplyCoordinatorDelegate
    ) {
        self.sessionMiniController = sessionMiniController
        self.delegate = delegate
    }

    @discardableResult
    func submitNotificationReply(
        notificationID: String,
        prompt: String,
        to sessionID: String,
        clientMutationID providedClientMutationID: String? = nil
    ) async -> Bool {
        guard let delegate else {
            return false
        }

        let trimmedNotificationID = notificationID.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedPrompt = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedNotificationID.isEmpty else {
            delegate.notificationReplyReject("Notification reply is missing its delivery ID.")
            return false
        }
        guard !trimmedPrompt.isEmpty else {
            delegate.notificationReplyReject("Prompt is required.")
            return false
        }

        let targetSurface = delegate.notificationReplyAssistantSurface(for: sessionID)
            ?? delegate.notificationReplySelectedAssistantSurface
        let providedMutationID = providedClientMutationID?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let clientMutationID = providedMutationID.isEmpty
            ? SessionQuickActionRequest.notificationReplyClientMutationID(
                notificationID: trimmedNotificationID
            )
            : providedMutationID
        delegate.notificationReplyEnqueueCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt,
            clientMutationID: clientMutationID
        )

        return await sendNotificationReplyCommand(
            notificationID: trimmedNotificationID,
            sessionID: sessionID,
            prompt: trimmedPrompt,
            targetSurface: targetSurface,
            clientMutationID: clientMutationID
        )
    }

    func drainPendingNotificationReplies() async {
        let drainTask = startOutboxDrainIfNeeded()
        await drainTask?.value
    }

    @discardableResult
    func startOutboxDrainIfNeeded() -> Task<Void, Never>? {
        guard let delegate else {
            return nil
        }

        return sessionMiniController.startNotificationReplyOutboxDrainIfNeeded(
            drainID: delegate.notificationReplyMakeClientMutationID()
        ) { [weak self] command in
            await self?.submitPendingNotificationReplyCommand(command) ?? false
        }
    }

    func stopOutboxDrain() {
        sessionMiniController.stopNotificationReplyOutboxDrain()
    }

    private func nextPendingCommand() -> CompanionSessionMiniPendingCommand? {
        sessionMiniController.pendingNotificationReplyCommand()
    }

    @discardableResult
    private func sendNotificationReplyCommand(
        notificationID: String,
        sessionID: String,
        prompt: String,
        targetSurface: CompanionAssistantSurface,
        clientMutationID: String
    ) async -> Bool {
        guard let delegate else {
            return false
        }

        do {
            let response = try await delegate.notificationReplyService.submitNotificationReply(
                notificationID: notificationID,
                sessionID: sessionID,
                prompt: prompt,
                assistantSurface: nil,
                clientMutationID: clientMutationID
            )
            delegate.notificationReplyMarkCommandDelivered(response.clientMutationID)
            if nextPendingCommand() == nil {
                sessionMiniController.resetNotificationReplyOutboxRetry()
            }
            await delegate.notificationReplyApplyAccepted(
                response,
                sessionID: sessionID,
                notificationID: notificationID,
                targetSurface: targetSurface
            )
            return true
        } catch {
            delegate.notificationReplyApplyFailure(
                error,
                sessionID: sessionID,
                notificationID: notificationID
            )
            scheduleOutboxRetryIfNeeded()
            return false
        }
    }

    @discardableResult
    private func submitPendingNotificationReplyCommand(
        _ command: CompanionSessionMiniPendingCommand
    ) async -> Bool {
        guard let notificationID = Self.nonEmptyText(command.notificationID),
              let prompt = Self.nonEmptyText(command.prompt),
              let sessionID = Self.nonEmptyText(command.threadID)
        else {
            CompanionDiagnostics.record(
                "notification-reply:drop-malformed-outbox-command id=\(command.clientMutationID)"
            )
            delegate?.notificationReplyMarkCommandDelivered(command.clientMutationID)
            return true
        }

        delegate?.notificationReplyMarkCommandAttempted(command.clientMutationID)
        let targetSurface = delegate?.notificationReplyAssistantSurface(for: sessionID)
            ?? delegate?.notificationReplySelectedAssistantSurface
            ?? .defaultSurface
        return await sendNotificationReplyCommand(
            notificationID: notificationID,
            sessionID: sessionID,
            prompt: prompt,
            targetSurface: targetSurface,
            clientMutationID: command.clientMutationID
        )
    }

    private func scheduleOutboxRetryIfNeeded() {
        guard let delegate else {
            return
        }

        sessionMiniController.scheduleNotificationReplyOutboxRetryIfNeeded(
            drainID: delegate.notificationReplyMakeClientMutationID()
        ) { [weak self] command in
            await self?.submitPendingNotificationReplyCommand(command) ?? false
        }
    }

    private static func nonEmptyText(_ value: String?) -> String? {
        guard let value else {
            return nil
        }
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

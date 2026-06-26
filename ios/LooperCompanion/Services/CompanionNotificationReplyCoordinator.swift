import Foundation
import LooperClientCore

@MainActor
protocol CompanionNotificationReplyCoordinatorDelegate: AnyObject {
    var notificationReplyService: any CompanionService { get }
    var notificationReplySelectedAssistantSurface: CompanionAssistantSurface { get }

    func notificationReplyMakeClientMutationID() -> String
    func notificationReplyAssistantSurface(for sessionID: String) -> CompanionAssistantSurface?
    func notificationReplyReject(_ message: String)
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
        ) { [weak self] in
            await self?.submitPendingNotificationReply() ?? false
        }
    }

    func stopOutboxDrain() {
        sessionMiniController.stopNotificationReplyOutboxDrain()
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
            startOutboxDrainIfNeeded()
            return false
        }
    }

    @discardableResult
    private func submitPendingNotificationReply() async -> Bool {
        do {
            guard let delegate else {
                return false
            }
            let response = try await delegate.notificationReplyService.submitPendingNotificationReply()
            guard let acceptedSessionID = Self.nonEmptyText(response.entityID),
                  let acceptedNotificationID = Self.nonEmptyText(response.notificationID)
            else {
                CompanionDiagnostics.record(
                    "notification-reply:pending-drain-missing-ack-target"
                )
                return false
            }
            let acceptedSurface = delegate.notificationReplyAssistantSurface(for: acceptedSessionID)
                ?? delegate.notificationReplySelectedAssistantSurface
            await delegate.notificationReplyApplyAccepted(
                response,
                sessionID: acceptedSessionID,
                notificationID: acceptedNotificationID,
                targetSurface: acceptedSurface
            )
            return true
        } catch {
            CompanionDiagnostics.record(
                "notification-reply:pending-drain-failed error=\(error.localizedDescription)"
            )
            return false
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

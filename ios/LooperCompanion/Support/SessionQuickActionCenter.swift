import Foundation

struct SessionQuickActionRequest: Equatable, Sendable {
    let action: QuickActionOption
    let sessionID: String
    let prompt: String?
    let notificationID: String?
    let clientMutationID: String?

    init(
        action: QuickActionOption,
        sessionID: String,
        prompt: String?,
        notificationID: String? = nil,
        clientMutationID: String? = nil
    ) {
        self.action = action
        self.sessionID = sessionID
        self.prompt = prompt
        self.notificationID = notificationID
        self.clientMutationID = clientMutationID
    }

    static func notificationReplyClientMutationID(notificationID: String) -> String {
        "notification-reply:\(notificationID)"
    }
}

@MainActor
final class SessionQuickActionCenter {
    typealias Handler = @MainActor @Sendable (SessionQuickActionRequest) async -> Void

    static let shared = SessionQuickActionCenter()

    private enum Limits {
        static let pendingRequestCount = 16
    }

    private var localStore: CompanionSessionMiniLocalStore?
    private var handler: Handler?
    private var pendingRequests: [SessionQuickActionRequest] = []

    init(localStore: CompanionSessionMiniLocalStore? = CompanionSessionMiniLocalStore.liveDefault()) {
        self.localStore = localStore
    }

    func configureLocalStore(_ localStore: CompanionSessionMiniLocalStore?) {
        self.localStore = localStore
    }

    func registerHandler(_ handler: @escaping Handler) {
        self.handler = handler
        drainPendingRequests(with: handler)
    }

    func unregisterHandler() {
        handler = nil
    }

    func submit(_ request: SessionQuickActionRequest) async {
        let request = persistDurableNotificationReplyIfNeeded(request)
        guard let handler else {
            rememberPendingRequest(request)
            return
        }

        if request.action == .reply {
            Task { @MainActor in
                await handler(request)
            }
            return
        }

        await handler(request)
    }

    private func drainPendingRequests(with handler: @escaping Handler) {
        let requests = pendingRequests
        pendingRequests.removeAll(keepingCapacity: true)
        guard !requests.isEmpty else {
            return
        }

        Task { @MainActor in
            for request in requests {
                await handler(request)
            }
        }
    }

    private func rememberPendingRequest(_ request: SessionQuickActionRequest) {
        pendingRequests.append(request)
        while pendingRequests.count > Limits.pendingRequestCount {
            pendingRequests.removeFirst()
        }
    }

    private func persistDurableNotificationReplyIfNeeded(
        _ request: SessionQuickActionRequest
    ) -> SessionQuickActionRequest {
        guard
            request.action == .reply,
            let notificationID = request.notificationID?.nilIfBlank,
            let prompt = request.prompt?.nilIfBlank
        else {
            return request
        }

        let clientMutationID = request.clientMutationID?.nilIfBlank
            ?? SessionQuickActionRequest.notificationReplyClientMutationID(
                notificationID: notificationID
            )
        do {
            try localStore?.enqueueNotificationReplyCommand(
                notificationID: notificationID,
                threadID: request.sessionID,
                prompt: prompt,
                assistantSurface: nil,
                clientMutationID: clientMutationID
            )
        } catch {
            CompanionDiagnostics.record(
                "notification-reply:quick-action-outbox-failed sessionID=\(request.sessionID) notificationID=\(notificationID) error=\(error.localizedDescription)"
            )
        }

        return SessionQuickActionRequest(
            action: request.action,
            sessionID: request.sessionID,
            prompt: request.prompt,
            notificationID: notificationID,
            clientMutationID: clientMutationID
        )
    }
}

private extension String {
    var nilIfBlank: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

import Foundation
import LooperClientCore

struct CompanionPromptSendResult: Sendable {
    let promptID: String?
    let dispatchKind: String?

    static func accepted(
        promptID: String?,
        dispatchKind: String?
    ) -> Self {
        Self(
            promptID: promptID,
            dispatchKind: dispatchKind
        )
    }
}

struct CompanionSessionModeResult: Sendable {
    let acceptedMode: SessionMode?

    static func accepted(
        mode: SessionMode?
    ) -> Self {
        Self(
            acceptedMode: mode
        )
    }
}

protocol CompanionSessionCommanding: Sendable {
    func prepareSessionRuntime() async
    func setSessionMode(
        id: String,
        preset: SessionMode?
    ) async throws -> CompanionSessionModeResult
    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult
    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult
    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult
    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult
}

struct CompanionSessionRuntimeEndpointResolution: Sendable {
    let bearerToken: String?
    let realtimeEndpointURLs: [URL]
}

struct CompanionSessionCommandClient: CompanionSessionCommanding {
    typealias EndpointResolver = @Sendable () async throws -> CompanionSessionRuntimeEndpointResolution

    private let sessionRuntime: CompanionSessionRuntime?
    private let endpointResolver: EndpointResolver

    init(
        sessionRuntime: CompanionSessionRuntime?,
        endpointResolver: @escaping EndpointResolver
    ) {
        self.sessionRuntime = sessionRuntime
        self.endpointResolver = endpointResolver
    }

    func prepareSessionRuntime() async {
        do {
            try await startSessionRuntime()
            CompanionDiagnostics.record("session-runtime:warm-success")
        } catch {
            CompanionDiagnostics.record("session-runtime:warm-failed error=\(error.localizedDescription)")
        }
    }

    func setSessionMode(
        id: String,
        preset: SessionMode?
    ) async throws -> CompanionSessionModeResult {
        let result = try await requiredSessionRuntime().setMode(
            threadID: id,
            preset: preset
        )
        return try Self.sessionModeResult(from: result, fallbackMode: preset, sessionID: id)
    }

    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult {
        let result = try await requiredSessionRuntime().sendPrompt(
            threadID: id,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
        return try Self.promptSendResult(from: result, sessionID: id)
    }

    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult {
        let result = try await requiredSessionRuntime().submitNotificationReply(
            notificationID: notificationID,
            threadID: sessionID,
            prompt: prompt,
            assistantSurface: assistantSurface
        )
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: sessionID
        )
    }

    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult {
        let result = try await requiredSessionRuntime().submitNotificationReply(
            notificationID: notificationID,
            threadID: sessionID,
            prompt: prompt,
            assistantSurface: assistantSurface,
            clientMutationID: clientMutationID
        )
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: sessionID
        )
    }

    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult {
        await prepareSessionRuntime()
        let result = try await requiredSessionRuntime().drainNotificationReplyOutbox()
        let notificationID = result.notificationId
        let sessionID = result.entityId
        guard result.accepted else {
            CompanionDiagnostics.record(
                "notification-reply:grpc-pending-invalid id=\(sessionID) notificationID=\(notificationID)"
            )
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record(
            "notification-reply:grpc-pending-accepted id=\(sessionID) notificationID=\(notificationID) kind=\(Self.dispatchKind(from: result.dispatchKind))"
        )
        return try Self.notificationReplyResponse(
            from: result,
            fallbackNotificationID: notificationID,
            sessionID: sessionID
        )
    }

    private func requiredSessionRuntime() throws -> CompanionSessionRuntime {
        guard let sessionRuntime else {
            throw HTTPCompanionServiceError.localStoreUnavailable
        }
        return sessionRuntime
    }

    private func startSessionRuntime() async throws {
        let endpointResolution = try await endpointResolver()
        _ = try await requiredSessionRuntime().startIfNeeded(
            bearerToken: endpointResolution.bearerToken,
            mobileSessionHeader: CompanionMobileSessionStore.loadValidHeaderValue() ?? ""
        ) {
            endpointResolution.realtimeEndpointURLs
        }
    }

    private static func sessionModeResult(
        from result: ClientSessionModeIntentResult,
        fallbackMode: SessionMode?,
        sessionID: String
    ) throws -> CompanionSessionModeResult {
        guard result.accepted else {
            CompanionDiagnostics.record("mode:grpc-invalid id=\(sessionID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record("mode:grpc-accepted id=\(sessionID)")
        return .accepted(
            mode: sessionMode(from: result.preset) ?? fallbackMode
        )
    }

    private static func promptSendResult(
        from result: ClientSessionPromptIntentResult,
        sessionID: String
    ) throws -> CompanionPromptSendResult {
        guard result.accepted else {
            CompanionDiagnostics.record("prompt:grpc-invalid id=\(sessionID)")
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record(
            "prompt:grpc-accepted id=\(sessionID) kind=\(dispatchKind(from: result.dispatchKind))"
        )
        return .accepted(
            promptID: nonEmpty(result.promptId),
            dispatchKind: dispatchKind(from: result.dispatchKind)
        )
    }

    private static func notificationReplyResponse(
        from result: ClientNotificationReplyIntentResult,
        fallbackNotificationID: String,
        sessionID: String
    ) throws -> ClientNotificationReplyIntentResult {
        guard result.accepted else {
            CompanionDiagnostics.record(
                "notification-reply:grpc-invalid id=\(sessionID) notificationID=\(fallbackNotificationID)"
            )
            throw HTTPCompanionServiceError.invalidResponse
        }
        CompanionDiagnostics.record(
            "notification-reply:grpc-accepted id=\(sessionID) notificationID=\(fallbackNotificationID) kind=\(dispatchKind(from: result.dispatchKind))"
        )
        return result
    }

    private static func sessionMode(from preset: String) -> SessionMode? {
        nonEmpty(preset).flatMap(SessionMode.init(rawValue:))
    }

    private static func dispatchKind(from value: String) -> String {
        nonEmpty(value) ?? "accepted"
    }

    private static func nonEmpty(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

struct UnconfiguredCompanionSessionCommandClient: CompanionSessionCommanding {
    let error: Error

    func prepareSessionRuntime() async {}

    func setSessionMode(
        id _: String,
        preset _: SessionMode?
    ) async throws -> CompanionSessionModeResult {
        throw error
    }

    func sendSessionPrompt(
        id _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult {
        throw error
    }

    func submitNotificationReply(
        notificationID _: String,
        sessionID _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult {
        throw error
    }

    func submitNotificationReply(
        notificationID _: String,
        sessionID _: String,
        prompt _: String,
        assistantSurface _: CompanionAssistantSurface?,
        clientMutationID _: String
    ) async throws -> ClientNotificationReplyIntentResult {
        throw error
    }

    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult {
        throw error
    }
}

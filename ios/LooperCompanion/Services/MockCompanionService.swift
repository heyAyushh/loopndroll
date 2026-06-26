import Foundation
import LooperClientCore
import LooperCompanionCore

private let mockCompanionBaseURL = "preview://looper"
private let millisecondsPerSecond: TimeInterval = 1_000

actor MockCompanionStore {
    private var allSessions: [SessionSummary]
    private var allDetails: [String: SessionDetail]
    var snapshot: MobileSnapshot
    private var details: [String: SessionDetail]
    var remotePushRegistration = RemotePushRegistrationResponse(
        state: .enabled,
        environment: .development,
        registeredAt: Date().ISO8601Format(),
        message: "Remote push is ready on this Mac."
    )

    init() {
        let initialSnapshot = PreviewFixtures.snapshot
        allSessions = initialSnapshot.sessions
        allDetails = PreviewFixtures.sessionDetails
        snapshot = initialSnapshot
        snapshot.sessions = Self.filteredSessions(
            allSessions,
            surface: initialSnapshot.globalSettings.assistantSurface
        )
        snapshot.surfaceSessions = Self.surfaceSessions(allSessions)
        details = Self.filteredDetails(allDetails, visibleSessions: snapshot.sessions)
    }

    func sessionDetail(id: String, surface: CompanionAssistantSurface?) -> SessionDetail {
        let visibleDetails: [String: SessionDetail]
        if let surface {
            visibleDetails = Self.filteredDetails(
                allDetails,
                visibleSessions: Self.filteredSessions(allSessions, surface: surface)
            )
        } else {
            visibleDetails = details
        }

        return visibleDetails[id] ?? visibleDetails.values.first ?? SessionDetail(
            id: id,
            ref: "C0",
            title: "Unknown Session",
            status: .stopped,
            effectiveMode: nil,
            lastUpdatedAt: Date().ISO8601Format(),
            assistantPreview: nil,
            latestAssistantMessage: nil,
            isArchived: false,
            assistantClient: .unknown,
            notificationIds: [],
            completionCheckID: nil,
            completionCheckWaitForReply: false,
            availableNotifications: snapshot.notifications,
            availableCompletionChecks: snapshot.completionChecks
        )
    }

    func setMode(id: String, preset: SessionMode?) -> MobileSnapshot {
        let timestamp = Date().ISO8601Format()
        allSessions = allSessions.map {
            guard $0.id == id else { return $0 }
            return SessionSummary(
                id: $0.id,
                ref: $0.ref,
                title: $0.title,
                status: $0.status,
                effectiveMode: preset,
                lastUpdatedAt: timestamp,
                lastActivityAt: timestamp,
                lastMessageAt: $0.lastMessageAt,
                assistantPreview: $0.assistantPreview,
                isArchived: $0.isArchived,
                assistantClient: $0.assistantClient,
                metadata: $0.metadata
            )
        }

        if var detail = allDetails[id] {
            detail.effectiveMode = preset
            detail.lastUpdatedAt = timestamp
            detail.lastActivityAt = detail.lastUpdatedAt
            allDetails[id] = detail
        }

        return publishSnapshot()
    }

    func setArchived(id: String, archived: Bool) -> MobileSnapshot {
        let timestamp = Date().ISO8601Format()
        allSessions = allSessions.map {
            guard $0.id == id else { return $0 }
            return SessionSummary(
                id: $0.id,
                ref: $0.ref,
                title: $0.title,
                status: archived ? .archived : .active,
                effectiveMode: $0.effectiveMode,
                lastUpdatedAt: timestamp,
                lastActivityAt: timestamp,
                lastMessageAt: $0.lastMessageAt,
                assistantPreview: $0.assistantPreview,
                isArchived: archived,
                assistantClient: $0.assistantClient,
                metadata: $0.metadata
            )
        }

        if var detail = allDetails[id] {
            detail.isArchived = archived
            detail.status = archived ? .archived : .active
            detail.lastUpdatedAt = timestamp
            detail.lastActivityAt = detail.lastUpdatedAt
            allDetails[id] = detail
        }

        return publishSnapshot()
    }

    func delete(id: String) -> MobileSnapshot {
        allSessions.removeAll { $0.id == id }
        allDetails.removeValue(forKey: id)
        return publishSnapshot()
    }

    func sendPrompt(id: String, prompt: String) -> MobileSnapshot {
        let timestamp = Date().ISO8601Format()
        let preview = "Queued prompt: \(prompt)"

        allSessions = allSessions.map {
            guard $0.id == id else { return $0 }
            return SessionSummary(
                id: $0.id,
                ref: $0.ref,
                title: $0.title,
                status: $0.status,
                effectiveMode: $0.effectiveMode,
                lastUpdatedAt: timestamp,
                lastActivityAt: timestamp,
                lastMessageAt: timestamp,
                assistantPreview: preview,
                isArchived: $0.isArchived,
                assistantClient: $0.assistantClient,
                metadata: $0.metadata
            )
        }

        if var detail = allDetails[id] {
            detail.assistantPreview = preview
            detail.latestAssistantMessage = preview
            detail.lastUpdatedAt = timestamp
            detail.lastActivityAt = timestamp
            detail.lastMessageAt = timestamp
            allDetails[id] = detail
        }

        return publishSnapshot()
    }

    func mute(id: String) -> MobileSnapshot {
        if var detail = allDetails[id] {
            detail.notificationIds = []
            allDetails[id] = detail
        }

        return publishSnapshot()
    }

    func savePrompt(_ prompt: String) -> MobileSnapshot {
        snapshot.globalSettings.defaultPrompt = prompt
        return publishSnapshot()
    }

    func saveAssistantSurface(_ surface: CompanionAssistantSurface) -> MobileSnapshot {
        snapshot.globalSettings.assistantSurface = surface
        return publishSnapshot()
    }

    func saveSiriDefaultSession(
        id: String?,
        assistantSurface: CompanionAssistantSurface?
    ) -> MobileSnapshot {
        snapshot.globalSettings.siriDefaultSessionId = id
        snapshot.globalSettings.siriDefaultAssistantSurface = id == nil ? nil : assistantSurface
        return publishSnapshot()
    }

    func saveSiriCurrentSession(
        id: String?,
        assistantSurface: CompanionAssistantSurface?
    ) -> MobileSnapshot {
        snapshot.globalSettings.siriCurrentSessionId = id
        snapshot.globalSettings.siriCurrentAssistantSurface = id == nil ? nil : assistantSurface
        snapshot.globalSettings.siriCurrentUpdatedAtMs = id == nil
            ? nil
            : Int64(Date().timeIntervalSince1970 * millisecondsPerSecond)
        return publishSnapshot()
    }

    @discardableResult
    private func publishSnapshot() -> MobileSnapshot {
        let surface = snapshot.globalSettings.assistantSurface
        snapshot.sessions = Self.filteredSessions(allSessions, surface: surface)
        snapshot.surfaceSessions = Self.surfaceSessions(allSessions)
        details = Self.filteredDetails(allDetails, visibleSessions: snapshot.sessions)
        return snapshot
    }

    private static func filteredSessions(
        _ sessions: [SessionSummary],
        surface: CompanionAssistantSurface
    ) -> [SessionSummary] {
        sessions.filter { session in
            CompanionSurfaceFiltering.matches(
                assistantClient: session.assistantClient.rawValue,
                surface: surface.rawValue
            )
        }
    }

    private static func surfaceSessions(
        _ sessions: [SessionSummary]
    ) -> [String: [SessionSummary]] {
        Dictionary(uniqueKeysWithValues: CompanionAssistantSurface.allCases.map { surface in
            (surface.rawValue, filteredSessions(sessions, surface: surface))
        })
    }

    private static func filteredDetails(
        _ details: [String: SessionDetail],
        visibleSessions: [SessionSummary]
    ) -> [String: SessionDetail] {
        let visibleSessionIDs = Set(visibleSessions.map(\.id))
        return details.filter { visibleSessionIDs.contains($0.key) }
    }

    func registerPushDevice(
        _ request: RemotePushRegistrationRequest
    ) -> RemotePushRegistrationResponse {
        remotePushRegistration = RemotePushRegistrationResponse(
            state: .enabled,
            environment: request.environment,
            registeredAt: Date().ISO8601Format(),
            message: "Remote push is ready on this Mac."
        )
        return remotePushRegistration
    }

    func sendTestPush() -> RemotePushTestResponse {
        RemotePushTestResponse(delivered: true, message: "Test push sent.")
    }
}

struct MockCompanionService: CompanionService, CompanionSessionCommanding {
    private enum MockCommandIDs {
        static let mode = "mock-mode"
        static let prompt = "mock-prompt"
        static let notificationReply = "mock"
        static let pendingNotificationReply = ""
    }

    private let store = MockCompanionStore()

    func loadServerHealth() async throws -> CompanionServerHealth {
        CompanionServerHealth(
            ok: true,
            baseURL: mockCompanionBaseURL,
            baseURLs: [mockCompanionBaseURL],
            grpcBaseURL: "http://127.0.0.1:8766",
            grpcBaseURLs: ["http://127.0.0.1:8766"],
            serverTime: Date().ISO8601Format()
        )
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        await store.snapshot
    }

    func loadSessionDetail(
        id: String,
        surface: CompanionAssistantSurface? = nil
    ) async throws -> SessionDetail {
        await store.sessionDetail(id: id, surface: surface)
    }

    func setSessionMode(
        id: String,
        preset: SessionMode?
    ) async throws -> CompanionSessionModeResult {
        _ = await store.setMode(id: id, preset: preset)
        return .accepted(
            mode: preset
        )
    }

    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot {
        await store.setArchived(id: id, archived: archived)
    }

    func deleteSession(id: String) async throws -> MobileSnapshot {
        await store.delete(id: id)
    }

    func sendSessionPrompt(
        id: String,
        prompt: String,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> CompanionPromptSendResult {
        _ = await store.sendPrompt(id: id, prompt: prompt)
        return .accepted(
            promptID: nil,
            dispatchKind: "mock"
        )
    }

    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface _: CompanionAssistantSurface?
    ) async throws -> ClientNotificationReplyIntentResult {
        _ = await store.sendPrompt(id: sessionID, prompt: prompt)
        return ClientNotificationReplyIntentResult(
            accepted: true,
            dispatchKind: "mock",
            promptId: "",
            serverTime: Date().ISO8601Format(),
            clientMutationId: MockCommandIDs.notificationReply,
            ackSeq: 0,
            entityId: sessionID,
            revision: "",
            idempotentReplay: false,
            notificationId: notificationID
        )
    }

    func submitNotificationReply(
        notificationID: String,
        sessionID: String,
        prompt: String,
        assistantSurface _: CompanionAssistantSurface?,
        clientMutationID: String
    ) async throws -> ClientNotificationReplyIntentResult {
        _ = await store.sendPrompt(id: sessionID, prompt: prompt)
        return ClientNotificationReplyIntentResult(
            accepted: true,
            dispatchKind: "mock",
            promptId: "",
            serverTime: Date().ISO8601Format(),
            clientMutationId: clientMutationID,
            ackSeq: 0,
            entityId: sessionID,
            revision: "",
            idempotentReplay: false,
            notificationId: notificationID
        )
    }

    func submitPendingNotificationReply() async throws -> ClientNotificationReplyIntentResult {
        ClientNotificationReplyIntentResult(
            accepted: true,
            dispatchKind: "mock",
            promptId: "",
            serverTime: Date().ISO8601Format(),
            clientMutationId: MockCommandIDs.pendingNotificationReply,
            ackSeq: 0,
            entityId: "",
            revision: "",
            idempotentReplay: false,
            notificationId: ""
        )
    }

    func muteSession(id: String) async throws -> MobileSnapshot {
        await store.mute(id: id)
    }

    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot {
        await store.savePrompt(prompt)
    }

    func saveAssistantSurface(_ surface: CompanionAssistantSurface) async throws -> MobileSnapshot {
        await store.saveAssistantSurface(surface)
    }

    func saveSiriDefaultSession(
        id: String?,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot {
        await store.saveSiriDefaultSession(id: id, assistantSurface: assistantSurface)
    }

    func saveSiriCurrentSession(
        id: String?,
        assistantSurface: CompanionAssistantSurface?
    ) async throws -> MobileSnapshot {
        await store.saveSiriCurrentSession(id: id, assistantSurface: assistantSurface)
    }

    func registerPushDevice(
        _ request: RemotePushRegistrationRequest
    ) async throws -> RemotePushRegistrationResponse {
        await store.registerPushDevice(request)
    }

    func sendTestPush(installationID _: String) async throws -> RemotePushTestResponse {
        await store.sendTestPush()
    }
}

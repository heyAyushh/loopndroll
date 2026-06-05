import Foundation

private let mockCompanionBaseURL = "preview://looper"

actor MockCompanionStore {
    var snapshot = PreviewFixtures.snapshot
    var details = PreviewFixtures.sessionDetails
    var remotePushRegistration = RemotePushRegistrationResponse(
        state: .enabled,
        environment: .development,
        registeredAt: Date().ISO8601Format(),
        message: "Remote push is ready on this Mac."
    )

    func sessionDetail(id: String) -> SessionDetail {
        details[id] ?? details.values.first ?? SessionDetail(
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
        snapshot.sessions = snapshot.sessions.map {
            guard $0.id == id else { return $0 }
            return SessionSummary(
                id: $0.id,
                ref: $0.ref,
                title: $0.title,
                status: $0.status,
                effectiveMode: preset,
                lastUpdatedAt: Date().ISO8601Format(),
                assistantPreview: $0.assistantPreview,
                isArchived: $0.isArchived,
                assistantClient: $0.assistantClient,
                metadata: $0.metadata
            )
        }

        if var detail = details[id] {
            detail.effectiveMode = preset
            detail.lastUpdatedAt = Date().ISO8601Format()
            details[id] = detail
        }

        return snapshot
    }

    func setArchived(id: String, archived: Bool) -> MobileSnapshot {
        snapshot.sessions = snapshot.sessions.map {
            guard $0.id == id else { return $0 }
            return SessionSummary(
                id: $0.id,
                ref: $0.ref,
                title: $0.title,
                status: archived ? .archived : .active,
                effectiveMode: $0.effectiveMode,
                lastUpdatedAt: Date().ISO8601Format(),
                assistantPreview: $0.assistantPreview,
                isArchived: archived,
                assistantClient: $0.assistantClient,
                metadata: $0.metadata
            )
        }

        if var detail = details[id] {
            detail.isArchived = archived
            detail.status = archived ? .archived : .active
            detail.lastUpdatedAt = Date().ISO8601Format()
            details[id] = detail
        }

        return snapshot
    }

    func delete(id: String) -> MobileSnapshot {
        snapshot.sessions.removeAll { $0.id == id }
        details.removeValue(forKey: id)
        return snapshot
    }

    func sendPrompt(id: String, prompt: String) -> MobileSnapshot {
        let timestamp = Date().ISO8601Format()
        let preview = "Queued prompt: \(prompt)"

        snapshot.sessions = snapshot.sessions.map {
            guard $0.id == id else { return $0 }
            return SessionSummary(
                id: $0.id,
                ref: $0.ref,
                title: $0.title,
                status: $0.status,
                effectiveMode: $0.effectiveMode,
                lastUpdatedAt: timestamp,
                assistantPreview: preview,
                isArchived: $0.isArchived,
                assistantClient: $0.assistantClient,
                metadata: $0.metadata
            )
        }

        if var detail = details[id] {
            detail.assistantPreview = preview
            detail.latestAssistantMessage = preview
            detail.lastUpdatedAt = timestamp
            details[id] = detail
        }

        return snapshot
    }

    func mute(id: String) -> MobileSnapshot {
        if var detail = details[id] {
            detail.notificationIds = []
            details[id] = detail
        }

        return snapshot
    }

    func savePrompt(_ prompt: String) -> MobileSnapshot {
        snapshot.globalSettings.defaultPrompt = prompt
        return snapshot
    }

    func saveAssistantSurface(_ surface: CompanionAssistantSurface) -> GlobalSettings {
        snapshot.globalSettings.assistantSurface = surface
        return snapshot.globalSettings
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

struct MockCompanionService: CompanionService {
    private let store = MockCompanionStore()

    func loadServerHealth() async throws -> CompanionServerHealth {
        CompanionServerHealth(
            ok: true,
            baseURL: mockCompanionBaseURL,
            baseURLs: [mockCompanionBaseURL],
            serverTime: Date().ISO8601Format()
        )
    }

    func loadSnapshot() async throws -> MobileSnapshot {
        await store.snapshot
    }

    func loadSessionDetail(id: String) async throws -> SessionDetail {
        await store.sessionDetail(id: id)
    }

    func setSessionMode(id: String, preset: SessionMode?) async throws -> MobileSnapshot {
        await store.setMode(id: id, preset: preset)
    }

    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot {
        await store.setArchived(id: id, archived: archived)
    }

    func deleteSession(id: String) async throws -> MobileSnapshot {
        await store.delete(id: id)
    }

    func sendSessionPrompt(id: String, prompt: String) async throws -> MobileSnapshot {
        await store.sendPrompt(id: id, prompt: prompt)
    }

    func muteSession(id: String) async throws -> MobileSnapshot {
        await store.mute(id: id)
    }

    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot {
        await store.savePrompt(prompt)
    }

    func saveAssistantSurface(_ surface: CompanionAssistantSurface) async throws -> GlobalSettings {
        await store.saveAssistantSurface(surface)
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

import Foundation

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
                isArchived: $0.isArchived
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
                isArchived: archived
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

    func savePrompt(_ prompt: String) -> MobileSnapshot {
        snapshot.globalSettings.defaultPrompt = prompt
        return snapshot
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

    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot {
        await store.savePrompt(prompt)
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

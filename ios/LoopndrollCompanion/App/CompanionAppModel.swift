import Foundation
import Observation

@MainActor
@Observable
final class CompanionAppModel {
    var snapshot: MobileSnapshot?
    var detailBySessionID: [String: SessionDetail] = [:]
    var connectionState: ConnectivityState = .connecting
    var errorMessage: String?
    var isLoading = false
    var lastUpdatedAt: Date?

    @ObservationIgnored private let service: any CompanionService

    init(environment: CompanionEnvironment) {
        service = environment.service
    }

    var activeSessions: [SessionSummary] {
        snapshot?.sessions.filter { !$0.isArchived } ?? []
    }

    var archivedSessions: [SessionSummary] {
        snapshot?.sessions.filter(\.isArchived) ?? []
    }

    func detail(for sessionID: String) -> SessionDetail? {
        detailBySessionID[sessionID]
    }

    func loadSnapshot() async {
        isLoading = true
        errorMessage = nil

        do {
            snapshot = try await service.loadSnapshot()
            connectionState = .connected
            lastUpdatedAt = Date()
        } catch {
            connectionState = .offline
            errorMessage = error.localizedDescription
            Haptics.error()
        }

        isLoading = false
    }

    func refresh() async {
        await loadSnapshot()
    }

    func loadSessionDetail(id: String) async {
        if detailBySessionID[id] != nil {
            return
        }

        do {
            detailBySessionID[id] = try await service.loadSessionDetail(id: id)
        } catch {
            errorMessage = error.localizedDescription
            Haptics.error()
        }
    }

    func applyMode(_ preset: SessionMode?, to sessionID: String) async {
        await mutateSnapshot {
            try await service.setSessionMode(id: sessionID, preset: preset)
        }
    }

    func setSessionArchived(_ archived: Bool, sessionID: String) async {
        await mutateSnapshot {
            try await service.setSessionArchived(id: sessionID, archived: archived)
        }
    }

    func deleteSession(_ sessionID: String) async {
        await mutateSnapshot {
            try await service.deleteSession(id: sessionID)
        }
    }

    func saveDefaultPrompt(_ defaultPrompt: String) async {
        await mutateSnapshot {
            try await service.saveDefaultPrompt(defaultPrompt)
        }
    }

    private func mutateSnapshot(_ operation: () async throws -> MobileSnapshot) async {
        errorMessage = nil

        do {
            snapshot = try await operation()
            lastUpdatedAt = Date()
            connectionState = .connected
            Haptics.impact()
        } catch {
            errorMessage = error.localizedDescription
            Haptics.error()
        }
    }
}

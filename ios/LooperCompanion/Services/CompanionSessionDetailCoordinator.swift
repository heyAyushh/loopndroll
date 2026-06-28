import Foundation

enum CompanionSessionDetailLoadOutcome {
    case loaded
    case skipped
    case stale
    case failed(Error)
}

@MainActor
final class CompanionSessionDetailCoordinator {
    private var loadingSessionDetailIDs: Set<String> = []

    func loadIfNeeded(
        id: String,
        service: any CompanionService,
        snapshotState: CompanionSnapshotStateStore,
        selectedAssistantSurface: CompanionAssistantSurface,
        connectionRevision: Int,
        isCurrentConnectionRevision: (Int) -> Bool
    ) async -> CompanionSessionDetailLoadOutcome {
        if snapshotState.hasDetail(for: id) {
            return .skipped
        }

        return await refresh(
            id: id,
            assistantSurface: nil,
            service: service,
            snapshotState: snapshotState,
            selectedAssistantSurface: selectedAssistantSurface,
            connectionRevision: connectionRevision,
            isCurrentConnectionRevision: isCurrentConnectionRevision
        )
    }

    func refresh(
        id: String,
        assistantSurface: CompanionAssistantSurface? = nil,
        service: any CompanionService,
        snapshotState: CompanionSnapshotStateStore,
        selectedAssistantSurface: CompanionAssistantSurface,
        connectionRevision: Int,
        isCurrentConnectionRevision: (Int) -> Bool
    ) async -> CompanionSessionDetailLoadOutcome {
        guard !loadingSessionDetailIDs.contains(id) else {
            return .skipped
        }

        loadingSessionDetailIDs.insert(id)
        defer {
            loadingSessionDetailIDs.remove(id)
        }

        let didApplyLocalDetail = applyLocalDetailIfAvailable(
            id: id,
            snapshotState: snapshotState
        )
        if didApplyLocalDetail {
            CompanionDiagnostics.record("session-detail:local-wins id=\(id)")
            return .loaded
        }

        var lastError: Error?
        for surface in detailQuerySurfaces(
            for: id,
            preferredSurface: assistantSurface,
            snapshotState: snapshotState,
            selectedAssistantSurface: selectedAssistantSurface
        ) {
            do {
                let detail = try await service.loadSessionDetail(
                    id: id,
                    surface: Optional(surface)
                )
                guard isCurrentConnectionRevision(connectionRevision) else {
                    CompanionDiagnostics.record("session-detail:stale-skip id=\(id)")
                    return didApplyLocalDetail ? .loaded : .stale
                }

                snapshotState.setDetail(detail, for: id)
                return .loaded
            } catch {
                guard isCurrentConnectionRevision(connectionRevision) else {
                    CompanionDiagnostics.record("session-detail:stale-error-skip id=\(id)")
                    return didApplyLocalDetail ? .loaded : .stale
                }

                lastError = error
                CompanionDiagnostics.record(
                    "session-detail:load-failed id=\(id) surface=\(surface.rawValue) error=\(error.localizedDescription)"
                )
            }
        }

        if let lastError {
            if didApplyLocalDetail {
                return .loaded
            }
            return .failed(lastError)
        }
        return didApplyLocalDetail ? .loaded : .skipped
    }

    private func applyLocalDetailIfAvailable(
        id: String,
        snapshotState: CompanionSnapshotStateStore
    ) -> Bool {
        guard let snapshot = snapshotState.snapshot,
              let session = snapshotState.session(withID: id) ?? snapshot.session(withID: id)
        else {
            return false
        }

        snapshotState.setDetail(
            SessionDetail(summary: session, snapshot: snapshot),
            for: id
        )
        return true
    }

    private func detailQuerySurfaces(
        for sessionID: String,
        preferredSurface: CompanionAssistantSurface?,
        snapshotState: CompanionSnapshotStateStore,
        selectedAssistantSurface: CompanionAssistantSurface
    ) -> [CompanionAssistantSurface] {
        var surfaces: [CompanionAssistantSurface] = []
        if let preferredSurface {
            surfaces.append(preferredSurface)
        } else if let detectedSurface = snapshotState.assistantSurface(containingSessionID: sessionID) {
            surfaces.append(detectedSurface)
        }

        if !surfaces.contains(selectedAssistantSurface) {
            surfaces.append(selectedAssistantSurface)
        }

        surfaces.append(contentsOf: CompanionAssistantSurface.allCases.filter { surface in
            !surfaces.contains(surface)
        })

        return surfaces
    }
}

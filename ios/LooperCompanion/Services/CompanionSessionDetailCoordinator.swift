@MainActor
final class CompanionSessionDetailCoordinator {
    func refresh(
        id: String,
        assistantSurface: CompanionAssistantSurface? = nil,
        snapshotState: CompanionSnapshotStateStore
    ) -> Bool {
        snapshotState.refreshDetail(
            for: id,
            assistantSurface: assistantSurface
        )
    }
}

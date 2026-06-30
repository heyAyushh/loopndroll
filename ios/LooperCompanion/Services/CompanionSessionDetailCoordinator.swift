@MainActor
final class CompanionSessionDetailCoordinator {
    func refresh(
        id: String,
        snapshotState: CompanionSnapshotStateStore
    ) -> Bool {
        snapshotState.refreshDetail(for: id)
    }
}

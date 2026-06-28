@MainActor
final class CompanionSessionDetailCoordinator {
    func refresh(
        id: String,
        snapshotState: CompanionSnapshotStateStore
    ) -> Bool {
        snapshotState.detail(for: id) != nil
    }
}

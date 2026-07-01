import Foundation

@MainActor
protocol CompanionSnapshotLoadCoordinatorDelegate: AnyObject {
    var snapshotLoadConnectionRevision: Int { get }

    func snapshotLoadSetLoading(_ isLoading: Bool)
    func snapshotLoadClearError()
    func snapshotLoadPerform(loadRevision: Int) async
}

@MainActor
final class CompanionSnapshotLoadCoordinator {
    private weak var delegate: CompanionSnapshotLoadCoordinatorDelegate?
    private var snapshotLoadTask: Task<Void, Never>?
    private var nextSnapshotLoadID = 0
    private var activeSnapshotLoadID = 0
    private var isDrainingSnapshotLoads = false
    private var hasPendingSnapshotLoad = false

    init(delegate: CompanionSnapshotLoadCoordinatorDelegate) {
        self.delegate = delegate
    }

    func cancelSnapshotLoad() {
        snapshotLoadTask?.cancel()
        snapshotLoadTask = nil
        delegate?.snapshotLoadSetLoading(false)
        isDrainingSnapshotLoads = false
        hasPendingSnapshotLoad = false
    }

    func loadSnapshot(allowsConcurrentConnectionReload: Bool) async {
        if isDrainingSnapshotLoads, !allowsConcurrentConnectionReload {
            hasPendingSnapshotLoad = true
            CompanionDiagnostics.lifecycle.info("Snapshot load coalesced behind active load")
            CompanionDiagnostics.record("snapshot:load-coalesced")
            await snapshotLoadTask?.value
            return
        }

        if isDrainingSnapshotLoads, allowsConcurrentConnectionReload {
            snapshotLoadTask?.cancel()
            hasPendingSnapshotLoad = false
        }

        isDrainingSnapshotLoads = true
        defer {
            isDrainingSnapshotLoads = false
        }

        repeat {
            hasPendingSnapshotLoad = false
            await loadSnapshotOnce()
        } while shouldDrainPendingSnapshotLoad()
    }

    private func loadSnapshotOnce() async {
        snapshotLoadTask?.cancel()
        let loadRevision = delegate?.snapshotLoadConnectionRevision ?? 0
        let loadID = nextSnapshotLoadIdentifier()
        delegate?.snapshotLoadSetLoading(true)
        delegate?.snapshotLoadClearError()

        let task = Task { @MainActor [weak self] in
            guard let self, let delegate = self.delegate else {
                return
            }

            await delegate.snapshotLoadPerform(loadRevision: loadRevision)
            self.finishSnapshotLoad(id: loadID, loadRevision: loadRevision)
        }
        snapshotLoadTask = task

        await withTaskCancellationHandler {
            await task.value
        } onCancel: {
            task.cancel()
        }
    }

    private func nextSnapshotLoadIdentifier() -> Int {
        nextSnapshotLoadID += 1
        activeSnapshotLoadID = nextSnapshotLoadID
        return nextSnapshotLoadID
    }

    private func finishSnapshotLoad(id: Int, loadRevision: Int) {
        guard id == activeSnapshotLoadID else {
            return
        }

        snapshotLoadTask = nil
        if loadRevision == delegate?.snapshotLoadConnectionRevision {
            delegate?.snapshotLoadSetLoading(false)
        }
    }

    private func shouldDrainPendingSnapshotLoad() -> Bool {
        guard hasPendingSnapshotLoad else {
            return false
        }

        guard !Task.isCancelled else {
            hasPendingSnapshotLoad = false
            return false
        }

        CompanionDiagnostics.record("snapshot:load-drain-pending")
        return true
    }
}

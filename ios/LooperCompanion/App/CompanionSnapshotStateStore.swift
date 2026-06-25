import Foundation
import LooperCompanionCore
import Observation

@MainActor
@Observable
final class CompanionSnapshotStateStore {
    var snapshot: MobileSnapshot?
    var detailBySessionID: [String: SessionDetail] = [:]
    var selectedAssistantSurface = CompanionAssistantSurface.defaultSurface
    private(set) var sessionSections = SessionSections.empty
    private(set) var sessionIndex = SessionIndex.empty

    @ObservationIgnored private var hasUserSelectedAssistantSurface = false
    @ObservationIgnored private var pendingAssistantSurfaceSave: CompanionAssistantSurface?
    @ObservationIgnored private var isSavingAssistantSurface = false

    var hasSnapshot: Bool {
        snapshot != nil
    }

    var allSessions: [SessionSummary] {
        sessionIndex.allSessions
    }

    var sessionIndexIdentity: String {
        sessionIndex.identity
    }

    var hasPendingAssistantSurfaceSave: Bool {
        pendingAssistantSurfaceSave != nil
    }

    func reset() {
        snapshot = nil
        detailBySessionID = [:]
        selectedAssistantSurface = .defaultSurface
        sessionSections = .empty
        sessionIndex = .empty
        hasUserSelectedAssistantSurface = false
        pendingAssistantSurfaceSave = nil
        isSavingAssistantSurface = false
    }

    func clearDetails() {
        detailBySessionID = [:]
    }

    func shouldSkipCachedRestore(onlyWhenSnapshotMissing: Bool) -> Bool {
        onlyWhenSnapshotMissing && hasSnapshot
    }

    @discardableResult
    func applySnapshot(
        _ nextSnapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface? = nil
    ) -> MobileSnapshot {
        let surface = preferredSurface ?? preferredAssistantSurface(for: nextSnapshot)
        let visibleSnapshot = nextSnapshot.visibleSnapshot(for: surface)
        selectedAssistantSurface = surface
        snapshot = visibleSnapshot
        sessionSections = SessionSections(sessions: visibleSnapshot.sessions)
        sessionIndex = SessionIndex(snapshot: visibleSnapshot)
        syncDetailCache(with: visibleSnapshot)
        return visibleSnapshot
    }

    @discardableResult
    func applyVisibleAssistantSurface(_ surface: CompanionAssistantSurface) -> MobileSnapshot? {
        guard let snapshot else {
            sessionSections = .empty
            sessionIndex = .empty
            selectedAssistantSurface = surface
            return nil
        }

        return applySnapshot(snapshot, preferredSurface: surface)
    }

    @discardableResult
    func selectAssistantSurface(_ surface: CompanionAssistantSurface) -> Bool {
        guard selectedAssistantSurface != surface else {
            return false
        }

        hasUserSelectedAssistantSurface = true
        pendingAssistantSurfaceSave = surface
        applyVisibleAssistantSurface(surface)
        return true
    }

    func beginAssistantSurfaceSaveIfNeeded() -> Bool {
        guard !isSavingAssistantSurface else {
            return false
        }

        isSavingAssistantSurface = true
        return true
    }

    func dequeuePendingAssistantSurfaceSave() -> CompanionAssistantSurface? {
        let nextAssistantSurface = pendingAssistantSurfaceSave
        pendingAssistantSurfaceSave = nil
        return nextAssistantSurface
    }

    func finishAssistantSurfaceSave() {
        isSavingAssistantSurface = false
    }

    func session(withID sessionID: String) -> SessionSummary? {
        sessionIndex.session(withID: sessionID)
    }

    func containsSession(_ sessionID: String) -> Bool {
        session(withID: sessionID) != nil
    }

    func sessions(for surface: CompanionAssistantSurface) -> [SessionSummary] {
        snapshot?.sessions(for: surface) ?? []
    }

    func assistantSurface(containingSessionID sessionID: String) -> CompanionAssistantSurface? {
        sessionIndex.assistantSurface(containingSessionID: sessionID)
    }

    func assistantSurface(for sessionID: String) -> CompanionAssistantSurface {
        assistantSurface(containingSessionID: sessionID) ?? selectedAssistantSurface
    }

    func activateRequestedAssistantSurfaceIfAvailable(
        _ requestedSurface: CompanionAssistantSurface?,
        sessionID: String
    ) -> CompanionAssistantSurface? {
        guard let requestedSurface,
              sessions(for: requestedSurface).contains(where: { session in
                  session.id == sessionID && !session.isArchived
              })
        else {
            return nil
        }

        applyVisibleAssistantSurface(requestedSurface)
        return requestedSurface
    }

    func selectAssistantSurfaceContainingSessionIfAvailable(
        _ sessionID: String
    ) -> CompanionAssistantSurface? {
        guard let surface = assistantSurface(containingSessionID: sessionID) else {
            return nil
        }

        applyVisibleAssistantSurface(surface)
        return surface
    }

    func detail(for sessionID: String) -> SessionDetail? {
        detailBySessionID[sessionID]
    }

    func hasDetail(for sessionID: String) -> Bool {
        detailBySessionID[sessionID] != nil
    }

    func setDetail(_ detail: SessionDetail, for sessionID: String) {
        detailBySessionID[sessionID] = detail
    }

    func removeDetail(for sessionID: String) {
        detailBySessionID[sessionID] = nil
    }

    func rollbackState(for sessionID: String) -> ModeRollbackState {
        ModeRollbackState(
            snapshot: snapshot,
            detail: detailBySessionID[sessionID]
        )
    }

    @discardableResult
    func applyOptimisticMode(_ preset: SessionMode?, to sessionID: String) -> Bool {
        var didUpdate = false

        if var detail = detailBySessionID[sessionID] {
            detail.effectiveMode = preset
            detailBySessionID[sessionID] = detail
            didUpdate = true
        }

        if var nextSnapshot = snapshot {
            updateMode(preset, for: sessionID, in: &nextSnapshot.sessions, didUpdate: &didUpdate)
            for surface in Array(nextSnapshot.surfaceSessions.keys) {
                updateMode(
                    preset,
                    for: sessionID,
                    in: &nextSnapshot.surfaceSessions[surface, default: []],
                    didUpdate: &didUpdate
                )
            }

            applySnapshot(nextSnapshot, preferredSurface: selectedAssistantSurface)
        }

        return didUpdate
    }

    func restoreOptimisticModeSnapshot(
        _ previousSnapshot: MobileSnapshot?,
        previousDetail: SessionDetail?,
        sessionID: String
    ) {
        if let previousSnapshot {
            applySnapshot(previousSnapshot, preferredSurface: selectedAssistantSurface)
        }

        detailBySessionID[sessionID] = previousDetail
    }

    private func preferredAssistantSurface(
        for nextSnapshot: MobileSnapshot
    ) -> CompanionAssistantSurface {
        if hasUserSelectedAssistantSurface {
            return selectedAssistantSurface
        }

        return nextSnapshot.globalSettings.assistantSurface
    }

    private func syncDetailCache(with nextSnapshot: MobileSnapshot) {
        let sessionIDs = Set(nextSnapshot.sessions.map(\.id))
        detailBySessionID = detailBySessionID.filter { sessionIDs.contains($0.key) }

        for session in nextSnapshot.sessions {
            guard var detail = detailBySessionID[session.id] else {
                continue
            }

            detail.status = session.status
            detail.effectiveMode = session.effectiveMode
            detail.lastUpdatedAt = session.lastUpdatedAt
            detail.lastActivityAt = session.lastActivityAt
            detail.lastMessageAt = session.lastMessageAt
            detail.assistantPreview = session.assistantPreview
            detail.isArchived = session.isArchived
            detail.metadata = session.metadata
            detailBySessionID[session.id] = detail
        }
    }

    private func updateMode(
        _ preset: SessionMode?,
        for sessionID: String,
        in sessions: inout [SessionSummary],
        didUpdate: inout Bool
    ) {
        guard let sessionIndex = sessions.firstIndex(where: { $0.id == sessionID }) else {
            return
        }

        sessions[sessionIndex].effectiveMode = preset
        didUpdate = true
    }
}

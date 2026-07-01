import Foundation
import LooperClientCore
import LooperCompanionCore
import Observation

private enum CompanionSnapshotSettingsTime {
    static let millisecondsPerSecond: TimeInterval = 1_000
}

private enum CompanionSnapshotLocalProjectionDefaults {
    static let hostID = "local"
    static let hostName = "Looper"
    static let scope = "global"
}

@MainActor
@Observable
final class CompanionSnapshotStateStore {
    private var visibleProjectionState = CompanionVisibleProjectionState.empty()

    var snapshot: MobileSnapshot? {
        visibleProjectionState.snapshot
    }

    var sourceSnapshot: MobileSnapshot? {
        sourceSnapshotForProjection()
    }

    var selectedAssistantSurface: CompanionAssistantSurface {
        visibleProjectionState.selectedAssistantSurface
    }

    var sessionSections: SessionSections {
        visibleProjectionState.sessionSections
    }

    var allSessionSections: SessionSections {
        canonicalSessionSections
    }

    var sessionIndex: SessionIndex {
        visibleProjectionState.sessionIndex
    }

    private var visibleSessionIndex: VisibleSessionIndex {
        visibleProjectionState.visibleSessionIndex
    }

    @ObservationIgnored private var canonicalSnapshot: MobileSnapshot?
    @ObservationIgnored private var canonicalSessionIndex: SessionIndex = .empty
    @ObservationIgnored private var canonicalSessionSections: SessionSections = .empty
    @ObservationIgnored private var visibleSurfaceProjections: [CompanionAssistantSurface: VisibleSurfaceProjection] = [:]
    @ObservationIgnored private var hasUserSelectedAssistantSurface = false
    @ObservationIgnored private var lastVisibleSnapshotFingerprint: VisibleSnapshotFingerprint?

    var hasSnapshot: Bool {
        snapshot != nil
    }

    var allSessions: [SessionSummary] {
        sessionIndex.allSessions
    }

    var sessionIndexIdentity: String {
        sessionIndex.identity
    }

    func reset() {
        visibleProjectionState = .empty()
        canonicalSnapshot = nil
        canonicalSessionIndex = .empty
        canonicalSessionSections = .empty
        visibleSurfaceProjections = [:]
        hasUserSelectedAssistantSurface = false
        lastVisibleSnapshotFingerprint = nil
    }

    @discardableResult
    func applyHostSyncTime(_ serverTime: String) -> Bool {
        let syncedAt = serverTime.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !syncedAt.isEmpty else {
            return false
        }

        var didChange = false
        if let nextCanonicalSnapshot = snapshotWithHostSyncTime(
            sourceSnapshotForProjection(),
            syncedAt: syncedAt
        ) {
            canonicalSnapshot = nextCanonicalSnapshot
            didChange = true
        }

        return didChange
    }

    func shouldSkipCachedRestore(onlyWhenSnapshotMissing: Bool) -> Bool {
        onlyWhenSnapshotMissing && hasSnapshot
    }

    @discardableResult
    func applySnapshot(
        _ nextSnapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface? = nil
    ) -> MobileSnapshot {
        applySnapshotResult(
            nextSnapshot,
            preferredSurface: preferredSurface
        ).visibleSnapshot
    }

    @discardableResult
    func applySnapshotResult(
        _ nextSnapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface? = nil
    ) -> CompanionSnapshotApplyResult {
        canonicalSnapshot = nextSnapshot
        refreshCanonicalProjectionCache(from: nextSnapshot)
        let surface = fallbackAssistantSurface(for: nextSnapshot, preferredSurface: preferredSurface)
        let previousFingerprint = lastVisibleSnapshotFingerprint
        let visibleSnapshot = applyVisibleSnapshot(nextSnapshot, surface: surface)
        return CompanionSnapshotApplyResult(
            visibleSnapshot: visibleSnapshot,
            didChangeVisibleSnapshot: previousFingerprint != lastVisibleSnapshotFingerprint
        )
    }

    @discardableResult
    func applyBroaderSnapshotPreservingCurrentSessions(
        _ cachedSnapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface? = nil
    ) -> CompanionSnapshotApplyResult? {
        guard let currentSnapshot = sourceSnapshotForProjection(),
              Self.sessionRouteKeys(in: cachedSnapshot)
              .isStrictSuperset(of: Self.sessionRouteKeys(in: currentSnapshot))
        else {
            return nil
        }

        let mergedSnapshot = Self.snapshotByAddingMissingSessions(
            from: cachedSnapshot,
            to: currentSnapshot
        )
        return applySnapshotResult(
            mergedSnapshot,
            preferredSurface: preferredSurface
        )
    }

    @discardableResult
    func applyVisibleAssistantSurface(_ surface: CompanionAssistantSurface) -> MobileSnapshot? {
        guard let snapshot = sourceSnapshotForProjection() else {
            visibleProjectionState = .empty(selectedSurface: surface)
            return nil
        }

        if let projection = visibleSurfaceProjections[surface] {
            return applyCachedVisibleSurfaceProjection(projection, selectedSurface: surface)
        }

        return applyVisibleSnapshot(snapshot, surface: surface)
    }

    @discardableResult
    func applyOptimisticVisibleSnapshot(
        _ visibleSnapshot: MobileSnapshot,
        selectedSurface: CompanionAssistantSurface
    ) -> Bool {
        applyVisibleProjection(
            visibleSnapshot,
            selectedSurface: selectedSurface,
            sessionIndex: canonicalSessionIndex
        )
    }

    @discardableResult
    func selectAssistantSurface(_ surface: CompanionAssistantSurface) -> Bool {
        let selection = SnapshotProjectionCodec.projectAssistantSurfaceSelection(
            currentSelectedSurface: selectedAssistantSurface,
            requestedSurface: surface,
            hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface
        )
        guard selection.didChange else {
            return false
        }

        let selectedSurface = SnapshotProjectionCodec.assistantSurface(
            from: selection.selectedAssistantSurface
        ) ?? surface
        hasUserSelectedAssistantSurface = selection.hasUserSelectedAssistantSurface
        applyVisibleAssistantSurface(selectedSurface)
        return true
    }

    @discardableResult
    func applyAcceptedAssistantSurface(_ surface: CompanionAssistantSurface) -> Bool {
        guard var nextSnapshot = sourceSnapshotForProjection() else {
            return false
        }
        nextSnapshot.globalSettings.assistantSurface = surface
        canonicalSnapshot = nextSnapshot

        guard selectedAssistantSurface != surface else {
            applyVisibleGlobalAssistantSurface(surface)
            return true
        }

        refreshCanonicalProjectionCache(from: nextSnapshot)
        applyVisibleAssistantSurface(surface)
        return true
    }

    @discardableResult
    func applyAcceptedSiriCurrentSession(
        sessionID: String,
        assistantSurface: CompanionAssistantSurface,
        updatedAt: Date = Date()
    ) -> Bool {
        applyGlobalSettingsMutation { settings in
            settings.siriCurrentSessionId = sessionID
            settings.siriCurrentAssistantSurface = assistantSurface
            settings.siriCurrentUpdatedAtMs = Self.millisecondsSinceEpoch(updatedAt)
        }
    }

    @discardableResult
    func applyAcceptedSiriDefaultSession(
        sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> Bool {
        applyGlobalSettingsMutation { settings in
            settings.siriDefaultSessionId = sessionID
            settings.siriDefaultAssistantSurface = assistantSurface
        }
    }

    @discardableResult
    func applyAcceptedDefaultPrompt(_ prompt: String) -> Bool {
        applyGlobalSettingsMutation { settings in
            settings.defaultPrompt = prompt
        }
    }

    func session(withID sessionID: String) -> SessionSummary? {
        guard let sourceSnapshot = sourceSnapshotForProjection() else {
            return visibleSession(withID: sessionID) ?? sessionIndex.session(withID: sessionID)
        }

        return sourceSnapshot.sessions(for: selectedAssistantSurface).first(where: { session in
            session.id == sessionID
        })
            ?? canonicalSessionIndex(for: sourceSnapshot).session(withID: sessionID)
            ?? visibleSession(withID: sessionID)
    }

    func session(
        withID sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> SessionSummary? {
        guard let sourceSnapshot = sourceSnapshotForProjection() else {
            guard selectedAssistantSurface == assistantSurface else {
                return nil
            }
            return visibleSession(withID: sessionID)
        }

        return sourceSnapshot.sessions(for: assistantSurface).first(where: { session in
            session.id == sessionID
        })
    }

    func containsSession(_ sessionID: String) -> Bool {
        session(withID: sessionID) != nil
    }

    func sessions(for surface: CompanionAssistantSurface) -> [SessionSummary] {
        sourceSnapshotForProjection()?.sessions(for: surface) ?? []
    }

    func assistantSurface(containingSessionID sessionID: String) -> CompanionAssistantSurface? {
        if visibleSession(withID: sessionID) != nil {
            return selectedAssistantSurface
        }
        return sessionIndex.assistantSurface(containingSessionID: sessionID)
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
        detail(for: sessionID, assistantSurface: selectedAssistantSurface)
    }

    func detail(
        for sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> SessionDetail? {
        guard let sourceSnapshot = sourceSnapshotForProjection(),
              let session = session(withID: sessionID, assistantSurface: assistantSurface)
        else {
            return nil
        }

        return SessionDetail(summary: session, snapshot: sourceSnapshot)
    }

    @discardableResult
    func refreshDetail(
        for sessionID: String,
        assistantSurface: CompanionAssistantSurface? = nil
    ) -> Bool {
        let targetSurface = assistantSurface ?? selectedAssistantSurface
        guard let sourceSnapshot = sourceSnapshotForProjection(),
              detail(for: sessionID, assistantSurface: targetSurface) != nil
        else {
            return false
        }

        visibleSurfaceProjections.removeValue(forKey: targetSurface)
        if targetSurface == selectedAssistantSurface {
            applyVisibleSnapshot(sourceSnapshot, surface: targetSurface)
        }
        return true
    }

    func hasDetail(for sessionID: String) -> Bool {
        detail(for: sessionID) != nil
    }

    @discardableResult
    private func applyVisibleProjection(
        _ visibleSnapshot: MobileSnapshot,
        selectedSurface: CompanionAssistantSurface,
        sessionIndex: SessionIndex
    ) -> Bool {
        let nextFingerprint = VisibleSnapshotFingerprint(snapshot: visibleSnapshot)
        guard nextFingerprint != lastVisibleSnapshotFingerprint else {
            return false
        }

        let reducedSections = SessionSections(localProjectionSessions: visibleSnapshot.sessions)
        let reducedVisibleSessionIndex = VisibleSessionIndex(sessions: visibleSnapshot.sessions)
        visibleSurfaceProjections[selectedSurface] = VisibleSurfaceProjection(
            visibleSnapshot: visibleSnapshot,
            sessionSections: reducedSections,
            sessionIndex: reducedVisibleSessionIndex,
            fingerprint: nextFingerprint
        )
        visibleProjectionState = CompanionVisibleProjectionState(
            snapshot: visibleSnapshot,
            selectedAssistantSurface: selectedSurface,
            sessionSections: reducedSections,
            visibleSessionIndex: reducedVisibleSessionIndex,
            sessionIndex: sessionIndex
        )
        lastVisibleSnapshotFingerprint = nextFingerprint
        return true
    }

    @discardableResult
    private func applyCachedVisibleSurfaceProjection(
        _ projection: VisibleSurfaceProjection,
        selectedSurface: CompanionAssistantSurface
    ) -> MobileSnapshot {
        let visibleSnapshot = visibleSnapshotWithCurrentHostSyncTime(projection.visibleSnapshot)
        visibleProjectionState = CompanionVisibleProjectionState(
            snapshot: visibleSnapshot,
            selectedAssistantSurface: selectedSurface,
            sessionSections: projection.sessionSections,
            visibleSessionIndex: projection.sessionIndex,
            sessionIndex: canonicalSessionIndex
        )
        lastVisibleSnapshotFingerprint = projection.fingerprint
        return visibleSnapshot
    }

    @discardableResult
    private func applyVisibleSnapshot(
        _ sourceSnapshot: MobileSnapshot,
        surface: CompanionAssistantSurface
    ) -> MobileSnapshot {
        let visibleSnapshot = sourceSnapshot.visibleSnapshot(for: surface)
        let visibleSections = SessionSections(localProjectionSessions: visibleSnapshot.sessions)
        let visibleSessionIndex = VisibleSessionIndex(sessions: visibleSnapshot.sessions)
        let fingerprint = VisibleSnapshotFingerprint(snapshot: visibleSnapshot)
        visibleSurfaceProjections[surface] = VisibleSurfaceProjection(
            visibleSnapshot: visibleSnapshot,
            sessionSections: visibleSections,
            sessionIndex: visibleSessionIndex,
            fingerprint: fingerprint
        )
        visibleProjectionState = CompanionVisibleProjectionState(
            snapshot: visibleSnapshot,
            selectedAssistantSurface: surface,
            sessionSections: visibleSections,
            visibleSessionIndex: visibleSessionIndex,
            sessionIndex: canonicalSessionIndex(for: sourceSnapshot)
        )
        lastVisibleSnapshotFingerprint = fingerprint
        return visibleSnapshot
    }

    private func applyVisibleGlobalAssistantSurface(_ surface: CompanionAssistantSurface) {
        guard var visibleSnapshot = snapshot else {
            return
        }

        visibleSnapshot.globalSettings.assistantSurface = surface
        visibleProjectionState = CompanionVisibleProjectionState(
            snapshot: visibleSnapshot,
            selectedAssistantSurface: surface,
            sessionSections: sessionSections,
            visibleSessionIndex: visibleSessionIndex,
            sessionIndex: sessionIndex
        )
        lastVisibleSnapshotFingerprint = VisibleSnapshotFingerprint(snapshot: visibleSnapshot)
    }

    @discardableResult
    private func applyGlobalSettingsMutation(
        preferredSurface: CompanionAssistantSurface? = nil,
        _ mutate: (inout GlobalSettings) -> Void
    ) -> Bool {
        var nextSnapshot = sourceSnapshotForProjection() ?? Self.emptyLocalSnapshot(
            selectedSurface: selectedAssistantSurface
        )

        mutate(&nextSnapshot.globalSettings)
        applySnapshot(nextSnapshot, preferredSurface: preferredSurface ?? selectedAssistantSurface)
        return true
    }

    private static func emptyLocalSnapshot(selectedSurface: CompanionAssistantSurface) -> MobileSnapshot {
        MobileSnapshot(
            host: HostSummary(
                id: CompanionSnapshotLocalProjectionDefaults.hostID,
                name: CompanionSnapshotLocalProjectionDefaults.hostName,
                address: "",
                isReachable: false,
                lastSyncedAt: ""
            ),
            globalSettings: GlobalSettings(
                defaultPrompt: "",
                globalMode: nil,
                scope: CompanionSnapshotLocalProjectionDefaults.scope,
                notificationLabel: nil,
                completionCheckLabel: nil,
                completionCheckWaitForReply: false,
                assistantSurface: selectedSurface
            ),
            sessions: [],
            surfaceSessions: [:],
            notifications: [],
            completionChecks: []
        )
    }

    private func refreshCanonicalProjectionCache(from sourceSnapshot: MobileSnapshot) {
        canonicalSessionIndex = SessionIndex(localSnapshot: sourceSnapshot)
        canonicalSessionSections = SessionSections(localProjectionSessions: canonicalSessionIndex.allSessions)
        visibleSurfaceProjections = [:]
    }

    private func snapshotWithHostSyncTime(
        _ snapshot: MobileSnapshot?,
        syncedAt: String
    ) -> MobileSnapshot? {
        guard var snapshot, snapshot.host.lastSyncedAt != syncedAt else {
            return nil
        }

        snapshot.host.lastSyncedAt = syncedAt
        return snapshot
    }

    private func visibleSnapshotWithCurrentHostSyncTime(
        _ visibleSnapshot: MobileSnapshot
    ) -> MobileSnapshot {
        guard let syncedAt = canonicalSnapshot?.host.lastSyncedAt else {
            return visibleSnapshot
        }

        return snapshotWithHostSyncTime(visibleSnapshot, syncedAt: syncedAt) ?? visibleSnapshot
    }

    private func canonicalSessionIndex(for sourceSnapshot: MobileSnapshot) -> SessionIndex {
        if canonicalSessionIndex == .empty {
            canonicalSessionIndex = SessionIndex(localSnapshot: sourceSnapshot)
            canonicalSessionSections = SessionSections(localProjectionSessions: canonicalSessionIndex.allSessions)
        }
        return canonicalSessionIndex
    }

    private static func millisecondsSinceEpoch(_ date: Date) -> Int64 {
        Int64(date.timeIntervalSince1970 * CompanionSnapshotSettingsTime.millisecondsPerSecond)
    }

    private static func snapshotByAddingMissingSessions(
        from cachedSnapshot: MobileSnapshot,
        to currentSnapshot: MobileSnapshot
    ) -> MobileSnapshot {
        var mergedSnapshot = currentSnapshot
        var mergedSurfaceSessions = surfaceSessionsByRawValue(in: currentSnapshot)
        let cachedSurfaceSessions = surfaceSessionsByRawValue(in: cachedSnapshot)

        for surface in CompanionAssistantSurface.allCases {
            let surfaceKey = surface.rawValue
            var mergedSessions = mergedSurfaceSessions[surfaceKey] ?? []
            var mergedSessionIDs = Set(mergedSessions.map(\.id))

            for cachedSession in cachedSurfaceSessions[surfaceKey] ?? [] where !mergedSessionIDs.contains(cachedSession.id) {
                mergedSessions.append(cachedSession)
                mergedSessionIDs.insert(cachedSession.id)
            }

            if mergedSessions.isEmpty {
                mergedSurfaceSessions.removeValue(forKey: surfaceKey)
            } else {
                mergedSurfaceSessions[surfaceKey] = mergedSessions
            }
        }

        mergedSnapshot.surfaceSessions = mergedSurfaceSessions
        mergedSnapshot.sessions = mergedSnapshot.sessions(for: mergedSnapshot.globalSettings.assistantSurface)
        return mergedSnapshot
    }

    private static func surfaceSessionsByRawValue(
        in snapshot: MobileSnapshot
    ) -> [String: [SessionSummary]] {
        var surfaceSessions: [String: [SessionSummary]] = [:]
        for surface in CompanionAssistantSurface.allCases {
            let sessions = snapshot.sessions(for: surface)
            guard !sessions.isEmpty else {
                continue
            }
            surfaceSessions[surface.rawValue] = sessions
        }
        return surfaceSessions
    }

    private static func sessionRouteKeys(in snapshot: MobileSnapshot) -> Set<String> {
        var keys: Set<String> = []
        for surface in CompanionAssistantSurface.allCases {
            for session in snapshot.sessions(for: surface) {
                keys.insert("\(surface.rawValue):\(session.id)")
            }
        }
        return keys
    }

    private func fallbackAssistantSurface(
        for snapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface?
    ) -> CompanionAssistantSurface {
        if let preferredSurface {
            return preferredSurface
        }
        if hasUserSelectedAssistantSurface {
            return selectedAssistantSurface
        }
        return snapshot.globalSettings.assistantSurface
    }

    private func sourceSnapshotForProjection() -> MobileSnapshot? {
        canonicalSnapshot ?? snapshot
    }

    private func visibleSession(withID sessionID: String) -> SessionSummary? {
        visibleSessionIndex.session(withID: sessionID)
    }

}

struct CompanionSnapshotApplyResult {
    let visibleSnapshot: MobileSnapshot
    let didChangeVisibleSnapshot: Bool
}

private enum SnapshotProjectionCodec {
    static func projectAssistantSurfaceSelection(
        currentSelectedSurface: CompanionAssistantSurface,
        requestedSurface: CompanionAssistantSurface,
        hasUserSelectedAssistantSurface: Bool
    ) -> ClientAssistantSurfaceSelection {
        reduceAssistantSurfaceSelection(
            currentSelectedAssistantSurface: currentSelectedSurface.rawValue,
            requestedAssistantSurface: requestedSurface.rawValue,
            hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface
        )
    }

    static func assistantSurface(from rawValue: String) -> CompanionAssistantSurface? {
        guard let surface = CompanionAssistantSurface(rawValue: rawValue) else {
            CompanionDiagnostics.record("snapshot:projection-unknown-surface surface=\(rawValue)")
            return nil
        }

        return surface
    }
}

private struct VisibleSnapshotFingerprint: Equatable {
    let byteCount: Int
    let contentHash: Int

    init(snapshot: MobileSnapshot) {
        var hasher = Hasher()
        var byteCount = 0
        Self.combine(snapshot.host, into: &hasher, byteCount: &byteCount)
        Self.combine(snapshot.globalSettings, into: &hasher, byteCount: &byteCount)
        Self.combine(snapshot.revision, into: &hasher, byteCount: &byteCount)
        hasher.combine(snapshot.sessions.count)
        byteCount += MemoryLayout<Int>.size
        for session in snapshot.sessions {
            hasher.combine(session)
            byteCount += Self.approximateByteCount(session)
        }

        self.byteCount = byteCount
        contentHash = hasher.finalize()
    }

    private static func combine(
        _ host: HostSummary,
        into hasher: inout Hasher,
        byteCount: inout Int
    ) {
        combine(host.id, into: &hasher, byteCount: &byteCount)
        combine(host.name, into: &hasher, byteCount: &byteCount)
        combine(host.address, into: &hasher, byteCount: &byteCount)
        combine(host.grpcAddress, into: &hasher, byteCount: &byteCount)
        hasher.combine(host.grpcAddresses)
        byteCount += host.grpcAddresses.reduce(0) { count, address in
            count + address.utf8.count
        }
        hasher.combine(host.isReachable)
        byteCount += MemoryLayout<Bool>.size
        combine(host.lastSyncedAt, into: &hasher, byteCount: &byteCount)
    }

    private static func combine(
        _ settings: GlobalSettings,
        into hasher: inout Hasher,
        byteCount: inout Int
    ) {
        combine(settings.defaultPrompt, into: &hasher, byteCount: &byteCount)
        combine(settings.globalMode?.rawValue, into: &hasher, byteCount: &byteCount)
        combine(settings.scope, into: &hasher, byteCount: &byteCount)
        combine(settings.notificationLabel, into: &hasher, byteCount: &byteCount)
        combine(settings.completionCheckLabel, into: &hasher, byteCount: &byteCount)
        hasher.combine(settings.completionCheckWaitForReply)
        byteCount += MemoryLayout<Bool>.size
        combine(settings.assistantSurface.rawValue, into: &hasher, byteCount: &byteCount)
        combine(settings.siriDefaultSessionId, into: &hasher, byteCount: &byteCount)
        combine(settings.siriDefaultAssistantSurface?.rawValue, into: &hasher, byteCount: &byteCount)
        combine(settings.siriCurrentSessionId, into: &hasher, byteCount: &byteCount)
        combine(settings.siriCurrentAssistantSurface?.rawValue, into: &hasher, byteCount: &byteCount)
        hasher.combine(settings.siriCurrentUpdatedAtMs)
        byteCount += MemoryLayout<Int64?>.size
    }

    private static func combine(
        _ value: String?,
        into hasher: inout Hasher,
        byteCount: inout Int
    ) {
        hasher.combine(value)
        byteCount += value?.utf8.count ?? 0
    }

    private static func approximateByteCount(_ session: SessionSummary) -> Int {
        session.id.utf8.count +
            session.ref.utf8.count +
            session.title.utf8.count +
            session.status.rawValue.utf8.count +
            (session.effectiveMode?.rawValue.utf8.count ?? 0) +
            session.lastUpdatedAt.utf8.count +
            session.lastActivityAt.utf8.count +
            (session.lastMessageAt?.utf8.count ?? 0) +
            (session.assistantPreview?.utf8.count ?? 0) +
            (session.promptDeliveryUnavailableReason?.utf8.count ?? 0) +
            session.assistantClient.rawValue.utf8.count
    }
}

private struct VisibleSurfaceProjection {
    let visibleSnapshot: MobileSnapshot
    let sessionSections: SessionSections
    let sessionIndex: VisibleSessionIndex
    let fingerprint: VisibleSnapshotFingerprint
}

private struct VisibleSessionIndex {
    static let empty = VisibleSessionIndex(sessions: [])

    private let sessionsByID: [String: SessionSummary]

    init(sessions: [SessionSummary]) {
        sessionsByID = Dictionary(uniqueKeysWithValues: sessions.map { ($0.id, $0) })
    }

    func session(withID sessionID: String) -> SessionSummary? {
        sessionsByID[sessionID]
    }
}

private struct CompanionVisibleProjectionState {
    let snapshot: MobileSnapshot?
    let selectedAssistantSurface: CompanionAssistantSurface
    let sessionSections: SessionSections
    let visibleSessionIndex: VisibleSessionIndex
    let sessionIndex: SessionIndex

    static func empty(
        selectedSurface: CompanionAssistantSurface = .defaultSurface
    ) -> CompanionVisibleProjectionState {
        CompanionVisibleProjectionState(
            snapshot: nil,
            selectedAssistantSurface: selectedSurface,
            sessionSections: .empty,
            visibleSessionIndex: .empty,
            sessionIndex: .empty
        )
    }
}

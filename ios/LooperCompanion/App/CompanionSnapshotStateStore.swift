import Foundation
import LooperClientCore
import LooperCompanionCore
import Observation

private enum CompanionSnapshotSettingsTime {
    static let millisecondsPerSecond: TimeInterval = 1_000
}

@MainActor
@Observable
final class CompanionSnapshotStateStore {
    var snapshot: MobileSnapshot?
    var selectedAssistantSurface = CompanionAssistantSurface.defaultSurface
    private(set) var sessionSections = SessionSections.empty
    private(set) var sessionIndex = SessionIndex.empty

    @ObservationIgnored private var canonicalSnapshot: MobileSnapshot?
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
        snapshot = nil
        canonicalSnapshot = nil
        selectedAssistantSurface = .defaultSurface
        sessionSections = .empty
        sessionIndex = .empty
        hasUserSelectedAssistantSurface = false
        lastVisibleSnapshotFingerprint = nil
    }

    @discardableResult
    func applyHostSyncTime(_ serverTime: String) -> Bool {
        let syncedAt = serverTime.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !syncedAt.isEmpty, var nextSnapshot = sourceSnapshotForProjection() else {
            return false
        }
        guard nextSnapshot.host.lastSyncedAt != syncedAt else {
            return false
        }

        nextSnapshot.host.lastSyncedAt = syncedAt
        canonicalSnapshot = nextSnapshot
        return true
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
        let projection = SnapshotProjectionCodec.reduceSnapshotProjection(
            snapshot: nextSnapshot,
            preferredSurface: preferredSurface,
            hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface,
            currentSelectedAssistantSurface: selectedAssistantSurface
        )
        guard let projection,
              let surface = SnapshotProjectionCodec.assistantSurface(
                from: projection.selectedAssistantSurface
              ),
              let visibleSnapshot = SnapshotProjectionCodec.decodeSnapshot(projection.visibleSnapshotJson)
        else {
            return applySnapshotWithoutProjection(nextSnapshot, preferredSurface: preferredSurface)
        }

        selectedAssistantSurface = surface
        return CompanionSnapshotApplyResult(
            visibleSnapshot: visibleSnapshot,
            didChangeVisibleSnapshot: applyReducedVisibleSnapshot(
                visibleSnapshot,
                projection: projection
            )
        )
    }

    @discardableResult
    func applyVisibleAssistantSurface(_ surface: CompanionAssistantSurface) -> MobileSnapshot? {
        guard let snapshot = sourceSnapshotForProjection() else {
            sessionSections = .empty
            sessionIndex = .empty
            selectedAssistantSurface = surface
            return nil
        }

        return applyVisibleSnapshot(snapshot, surface: surface)
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
        applyVisibleSnapshot(nextSnapshot, surface: surface)
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
        visibleSession(withID: sessionID) ?? sessionIndex.session(withID: sessionID)
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
        guard let sourceSnapshot = sourceSnapshotForProjection(),
              let session = visibleSession(withID: sessionID)
                ?? sessionIndex.session(withID: sessionID)
                ?? sourceSnapshot.session(withID: sessionID)
        else {
            return nil
        }

        return SessionDetail(summary: session, snapshot: sourceSnapshot)
    }

    func hasDetail(for sessionID: String) -> Bool {
        detail(for: sessionID) != nil
    }

    private func applyReducedVisibleSnapshot(
        _ visibleSnapshot: MobileSnapshot,
        projection: ClientSnapshotProjection
    ) -> Bool {
        let nextFingerprint = VisibleSnapshotFingerprint(json: projection.visibleSnapshotJson)
        guard nextFingerprint != lastVisibleSnapshotFingerprint else {
            return false
        }

        snapshot = visibleSnapshot
        sessionSections = SessionSections(
            projection: projection.sessionSections,
            sessions: visibleSnapshot.sessions
        )
        sessionIndex = SessionIndex(
            projection: projection.sessionIndex,
            snapshot: visibleSnapshot
        )
        lastVisibleSnapshotFingerprint = nextFingerprint
        return true
    }

    @discardableResult
    private func applyFallbackVisibleSnapshot(
        _ visibleSnapshot: MobileSnapshot,
        selectedSurface: CompanionAssistantSurface
    ) -> Bool {
        snapshot = visibleSnapshot
        selectedAssistantSurface = selectedSurface
        sessionSections = SessionSections(sessions: visibleSnapshot.sessions)
        sessionIndex = SessionIndex(snapshot: visibleSnapshot)
        lastVisibleSnapshotFingerprint = nil
        return true
    }

    @discardableResult
    private func applyVisibleSnapshot(
        _ sourceSnapshot: MobileSnapshot,
        surface: CompanionAssistantSurface
    ) -> MobileSnapshot {
        let visibleSnapshot = sourceSnapshot.visibleSnapshot(for: surface)
        snapshot = visibleSnapshot
        selectedAssistantSurface = surface
        sessionSections = SessionSections(sessions: visibleSnapshot.sessions)
        if sessionIndex == .empty {
            sessionIndex = SessionIndex(snapshot: sourceSnapshot)
        }
        lastVisibleSnapshotFingerprint = nil
        return visibleSnapshot
    }

    @discardableResult
    private func applyGlobalSettingsMutation(
        preferredSurface: CompanionAssistantSurface? = nil,
        _ mutate: (inout GlobalSettings) -> Void
    ) -> Bool {
        guard var nextSnapshot = sourceSnapshotForProjection() else {
            return false
        }

        mutate(&nextSnapshot.globalSettings)
        applySnapshot(nextSnapshot, preferredSurface: preferredSurface ?? selectedAssistantSurface)
        return true
    }

    private static func millisecondsSinceEpoch(_ date: Date) -> Int64 {
        Int64(date.timeIntervalSince1970 * CompanionSnapshotSettingsTime.millisecondsPerSecond)
    }

    private func applySnapshotWithoutProjection(
        _ nextSnapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface?
    ) -> CompanionSnapshotApplyResult {
        let surface = fallbackAssistantSurface(
            for: nextSnapshot,
            preferredSurface: preferredSurface
        )
        let visibleSnapshot = nextSnapshot.visibleSnapshot(for: surface)
        return CompanionSnapshotApplyResult(
            visibleSnapshot: visibleSnapshot,
            didChangeVisibleSnapshot: applyFallbackVisibleSnapshot(
                visibleSnapshot,
                selectedSurface: surface
            )
        )
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
        snapshot?.sessions.first { session in
            session.id == sessionID
        }
    }

}

struct CompanionSnapshotApplyResult {
    let visibleSnapshot: MobileSnapshot
    let didChangeVisibleSnapshot: Bool
}

private enum SnapshotProjectionCodec {
    static func reduceSnapshotProjection(
        snapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface?,
        hasUserSelectedAssistantSurface: Bool,
        currentSelectedAssistantSurface: CompanionAssistantSurface
    ) -> ClientSnapshotProjection? {
        guard let snapshotJSON = encode(snapshot) else {
            return nil
        }
        do {
            return try reduceMobileSnapshotProjection(
                snapshotJson: snapshotJSON,
                preferredAssistantSurface: preferredSurface?.rawValue ?? "",
                hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface,
                currentSelectedAssistantSurface: currentSelectedAssistantSurface.rawValue,
                assistantSurfaceOrder: CompanionAssistantSurface.allCases.map(\.rawValue)
            )
        } catch {
            recordFailure("snapshot:projection-failed", error: error)
            return nil
        }
    }

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

    static func decodeSnapshot(_ json: String) -> MobileSnapshot? {
        decode(MobileSnapshot.self, from: json)
    }

    private static func encode<Value: Encodable>(_ value: Value) -> String? {
        do {
            let data = try JSONEncoder().encode(value)
            guard let json = String(data: data, encoding: .utf8) else {
                CompanionDiagnostics.record("snapshot:projection-non-utf8")
                return nil
            }

            return json
        } catch {
            recordFailure("snapshot:projection-encode-failed", error: error)
            return nil
        }
    }

    private static func decode<Value: Decodable>(_ type: Value.Type, from json: String) -> Value? {
        do {
            return try JSONDecoder().decode(type, from: Data(json.utf8))
        } catch {
            recordFailure("snapshot:projection-decode-failed", error: error)
            return nil
        }
    }

    private static func recordFailure(_ message: String, error: Error) {
        CompanionDiagnostics.record("\(message) error=\(error.localizedDescription)")
    }
}

private struct VisibleSnapshotFingerprint: Equatable {
    let byteCount: Int
    let contentHash: Int

    init(json: String) {
        byteCount = json.utf8.count
        contentHash = json.hashValue
    }
}

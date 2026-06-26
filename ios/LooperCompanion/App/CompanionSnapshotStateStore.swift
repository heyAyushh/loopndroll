import Foundation
import LooperClientCore
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
        let projection = SnapshotProjectionCodec.reduceSnapshotProjection(
            snapshot: nextSnapshot,
            preferredSurface: preferredSurface,
            hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface,
            currentSelectedAssistantSurface: selectedAssistantSurface
        )
        let surface = SnapshotProjectionCodec.assistantSurface(from: projection.selectedAssistantSurface)
        let visibleSnapshot = SnapshotProjectionCodec.decodeSnapshot(projection.visibleSnapshotJson)
        selectedAssistantSurface = surface
        applyReducedVisibleSnapshot(visibleSnapshot)
        syncDetailCache(withVisibleSnapshotJSON: projection.visibleSnapshotJson)
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
        )
        hasUserSelectedAssistantSurface = selection.hasUserSelectedAssistantSurface
        pendingAssistantSurfaceSave = selection.hasPendingAssistantSurfaceSave
            ? SnapshotProjectionCodec.assistantSurface(from: selection.pendingAssistantSurface)
            : nil
        applyVisibleAssistantSurface(selectedSurface)
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

    private func applyReducedVisibleSnapshot(_ visibleSnapshot: MobileSnapshot) {
        snapshot = visibleSnapshot
        sessionSections = SessionSections(sessions: visibleSnapshot.sessions)
        sessionIndex = SessionIndex(snapshot: visibleSnapshot)
    }

    private func syncDetailCache(withVisibleSnapshotJSON visibleSnapshotJSON: String) {
        let projection = SnapshotProjectionCodec.reduceDetailCache(
            visibleSnapshotJSON: visibleSnapshotJSON,
            detailBySessionID: detailBySessionID
        )
        detailBySessionID = SnapshotProjectionCodec.decodeDetailMap(
            projection.detailBySessionIdJson
        )
    }

}

private enum SnapshotProjectionCodec {
    static func reduceSnapshotProjection(
        snapshot: MobileSnapshot,
        preferredSurface: CompanionAssistantSurface?,
        hasUserSelectedAssistantSurface: Bool,
        currentSelectedAssistantSurface: CompanionAssistantSurface
    ) -> ClientSnapshotProjection {
        do {
            return try reduceMobileSnapshotProjection(
                snapshotJson: encode(snapshot),
                preferredAssistantSurface: preferredSurface?.rawValue ?? "",
                hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface,
                currentSelectedAssistantSurface: currentSelectedAssistantSurface.rawValue
            )
        } catch {
            invariantFailure("Snapshot projection failed", error: error)
        }
    }

    static func reduceDetailCache(
        visibleSnapshotJSON: String,
        detailBySessionID: [String: SessionDetail]
    ) -> ClientDetailCacheProjection {
        do {
            return try reduceMobileSnapshotDetailCache(
                visibleSnapshotJson: visibleSnapshotJSON,
                detailBySessionIdJson: encode(detailBySessionID)
            )
        } catch {
            invariantFailure("Detail cache projection failed", error: error)
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

    static func assistantSurface(from rawValue: String) -> CompanionAssistantSurface {
        guard let surface = CompanionAssistantSurface(rawValue: rawValue) else {
            fatalError("Snapshot projection returned unknown assistant surface: \(rawValue)")
        }

        return surface
    }

    static func decodeSnapshot(_ json: String) -> MobileSnapshot {
        decode(MobileSnapshot.self, from: json)
    }

    static func decodeDetailMap(_ json: String) -> [String: SessionDetail] {
        decode([String: SessionDetail].self, from: json)
    }

    private static func encode<Value: Encodable>(_ value: Value) -> String {
        do {
            let data = try JSONEncoder().encode(value)
            guard let json = String(data: data, encoding: .utf8) else {
                fatalError("Client projection payload was not valid UTF-8")
            }

            return json
        } catch {
            invariantFailure("Client projection payload encoding failed", error: error)
        }
    }

    private static func decode<Value: Decodable>(_ type: Value.Type, from json: String) -> Value {
        do {
            return try JSONDecoder().decode(type, from: Data(json.utf8))
        } catch {
            invariantFailure("Client projection payload decoding failed", error: error)
        }
    }

    private static func invariantFailure(_ message: String, error: Error) -> Never {
        fatalError("\(message): \(error)")
    }
}

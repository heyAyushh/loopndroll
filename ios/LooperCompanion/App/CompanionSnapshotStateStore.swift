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
        let projection = SnapshotReducerCodec.reduceSnapshotProjection(
            snapshot: nextSnapshot,
            preferredSurface: preferredSurface,
            hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface,
            currentSelectedAssistantSurface: selectedAssistantSurface
        )
        let surface = SnapshotReducerCodec.assistantSurface(from: projection.selectedAssistantSurface)
        let visibleSnapshot = SnapshotReducerCodec.decodeSnapshot(projection.visibleSnapshotJson)
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
        let selection = SnapshotReducerCodec.projectAssistantSurfaceSelection(
            currentSelectedSurface: selectedAssistantSurface,
            requestedSurface: surface,
            hasUserSelectedAssistantSurface: hasUserSelectedAssistantSurface
        )
        guard selection.didChange else {
            return false
        }

        let selectedSurface = SnapshotReducerCodec.assistantSurface(
            from: selection.selectedAssistantSurface
        )
        hasUserSelectedAssistantSurface = selection.hasUserSelectedAssistantSurface
        pendingAssistantSurfaceSave = selection.hasPendingAssistantSurfaceSave
            ? SnapshotReducerCodec.assistantSurface(from: selection.pendingAssistantSurface)
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

    func rollbackState(for sessionID: String) -> ModeRollbackState {
        ModeRollbackState(
            snapshot: snapshot,
            detail: detailBySessionID[sessionID]
        )
    }

    @discardableResult
    func applyOptimisticMode(_ preset: SessionMode?, to sessionID: String) -> Bool {
        let detailJSON = SnapshotReducerCodec.encodeDetail(detailBySessionID[sessionID])

        guard let snapshot else {
            let projection = SnapshotReducerCodec.reduceDetailOptimisticMode(
                detailJSON: detailJSON,
                preset: preset
            )
            applyDetailModeProjection(projection, to: sessionID)
            return projection.didUpdate
        }

        let projection = SnapshotReducerCodec.reduceSnapshotOptimisticMode(
            snapshot: snapshot,
            detailJSON: detailJSON,
            sessionID: sessionID,
            preset: preset,
            selectedAssistantSurface: selectedAssistantSurface
        )
        applyReducedVisibleSnapshot(
            SnapshotReducerCodec.decodeSnapshot(projection.visibleSnapshotJson)
        )
        if projection.hasDetail {
            detailBySessionID[sessionID] = SnapshotReducerCodec.decodeDetail(
                projection.visibleDetailJson
            )
        }

        return projection.didUpdate
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

    private func applyReducedVisibleSnapshot(_ visibleSnapshot: MobileSnapshot) {
        snapshot = visibleSnapshot
        sessionSections = SessionSections(sessions: visibleSnapshot.sessions)
        sessionIndex = SessionIndex(snapshot: visibleSnapshot)
    }

    private func syncDetailCache(withVisibleSnapshotJSON visibleSnapshotJSON: String) {
        let projection = SnapshotReducerCodec.reduceDetailCache(
            visibleSnapshotJSON: visibleSnapshotJSON,
            detailBySessionID: detailBySessionID
        )
        detailBySessionID = SnapshotReducerCodec.decodeDetailMap(
            projection.detailBySessionIdJson
        )
    }

    private func applyDetailModeProjection(
        _ projection: ClientDetailModeProjection,
        to sessionID: String
    ) {
        guard projection.hasDetail else {
            return
        }

        detailBySessionID[sessionID] = SnapshotReducerCodec.decodeDetail(projection.detailJson)
    }
}

private enum SnapshotReducerCodec {
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
            invariantFailure("Snapshot projection reducer failed", error: error)
        }
    }

    static func reduceSnapshotOptimisticMode(
        snapshot: MobileSnapshot,
        detailJSON: String,
        sessionID: String,
        preset: SessionMode?,
        selectedAssistantSurface: CompanionAssistantSurface
    ) -> ClientOptimisticModeProjection {
        do {
            return try reduceMobileSnapshotOptimisticMode(
                snapshotJson: encode(snapshot),
                detailJson: detailJSON,
                sessionId: sessionID,
                preset: preset?.rawValue ?? "",
                selectedAssistantSurface: selectedAssistantSurface.rawValue
            )
        } catch {
            invariantFailure("Snapshot optimistic mode reducer failed", error: error)
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
            invariantFailure("Detail cache reducer failed", error: error)
        }
    }

    static func reduceDetailOptimisticMode(
        detailJSON: String,
        preset: SessionMode?
    ) -> ClientDetailModeProjection {
        do {
            return try reduceSessionDetailOptimisticMode(
                detailJson: detailJSON,
                preset: preset?.rawValue ?? ""
            )
        } catch {
            invariantFailure("Detail optimistic mode reducer failed", error: error)
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
            fatalError("Snapshot reducer returned unknown assistant surface: \(rawValue)")
        }

        return surface
    }

    static func encodeDetail(_ detail: SessionDetail?) -> String {
        guard let detail else {
            return ""
        }

        return encode(detail)
    }

    static func decodeSnapshot(_ json: String) -> MobileSnapshot {
        decode(MobileSnapshot.self, from: json)
    }

    static func decodeDetail(_ json: String) -> SessionDetail {
        decode(SessionDetail.self, from: json)
    }

    static func decodeDetailMap(_ json: String) -> [String: SessionDetail] {
        decode([String: SessionDetail].self, from: json)
    }

    private static func encode<Value: Encodable>(_ value: Value) -> String {
        do {
            let data = try JSONEncoder().encode(value)
            guard let json = String(data: data, encoding: .utf8) else {
                fatalError("Client reducer payload was not valid UTF-8")
            }

            return json
        } catch {
            invariantFailure("Client reducer payload encoding failed", error: error)
        }
    }

    private static func decode<Value: Decodable>(_ type: Value.Type, from json: String) -> Value {
        do {
            return try JSONDecoder().decode(type, from: Data(json.utf8))
        } catch {
            invariantFailure("Client reducer payload decoding failed", error: error)
        }
    }

    private static func invariantFailure(_ message: String, error: Error) -> Never {
        fatalError("\(message): \(error)")
    }
}

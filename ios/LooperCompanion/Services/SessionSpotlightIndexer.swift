import AppIntents
import CoreSpotlight
import UniformTypeIdentifiers

private enum SpotlightIndexing {
    static let batchSize = 100
    static let entityAssociationPriority = 1
}

enum SpotlightIdentifiers {
    static let domainIdentifier = "dev.looper.session"
    static let indexName = "LooperSessions"
}

struct SessionSpotlightRecord: Equatable, Sendable {
    let id: String
    let ref: String
    let title: String
    let status: SessionStatus
    let lastUpdatedAt: String
    let lastActivityAt: String
    let lastMessageAt: String?
    let assistantPreview: String?
    let isArchived: Bool
    let assistantClient: AssistantClient
    let metadata: SessionMetadata

    init(session: SessionSummary) {
        id = session.id
        ref = session.ref
        title = session.title
        status = session.status
        lastUpdatedAt = session.lastUpdatedAt
        lastActivityAt = session.lastActivityAt
        lastMessageAt = session.lastMessageAt
        assistantPreview = session.assistantPreview
        isArchived = session.isArchived
        assistantClient = session.assistantClient
        metadata = session.metadata
    }

    var searchableIdentifiers: [String] {
        Self.searchableIdentifiers(
            for: id,
            assistantSurface: assistantClient.spotlightAssistantSurface
        )
    }

    static func searchableIdentifiers(
        for sessionID: String,
        assistantSurface: CompanionAssistantSurface
    ) -> [String] {
        [
            sessionID,
            LooperSessionEntityIdentifier(
                assistantSurface: assistantSurface,
                sessionID: sessionID
            ).rawValue
        ]
    }

    static func legacyAndQualifiedSearchableIdentifiers(for sessionID: String) -> [String] {
        [sessionID] + CompanionAssistantSurface.allCases.map { assistantSurface in
            LooperSessionEntityIdentifier(
                assistantSurface: assistantSurface,
                sessionID: sessionID
            ).rawValue
        }
    }
}

enum SessionSpotlightIndexingPolicy {
    static func indexableSessions(from sessions: [SessionSummary]) -> [SessionSummary] {
        let sortedSessions = uniqueSessionsByID(sessions).sortedBySessionFreshness()
        var currentSessions: [SessionSummary] = []
        var stoppedSessions: [SessionSummary] = []

        for session in sortedSessions where shouldIndex(session) {
            if session.status == .stopped {
                stoppedSessions.append(session)
            } else {
                currentSessions.append(session)
            }
        }

        return (currentSessions + stoppedSessions.prefix(SessionDisplayPolicy.collapsedSectionLimit))
            .sortedBySessionFreshness()
    }

    private static func shouldIndex(_ session: SessionSummary) -> Bool {
        !session.isArchived && session.status != .archived
    }

    private static func uniqueSessionsByID(_ sessions: [SessionSummary]) -> [SessionSummary] {
        var seenSessionIDs = Set<String>()
        return sessions.filter { session in
            seenSessionIDs.insert(session.id).inserted
        }
    }
}

struct SessionSearchableItem {
    let session: SessionSummary

    var appEntity: LooperSessionEntity {
        LooperSessionEntity(
            session: session,
            assistantSurface: session.assistantClient.spotlightAssistantSurface
        )
    }

    var searchableItem: CSSearchableItem {
        let entity = appEntity
        let attributeSet = entity.attributeSet
        if let lastActivityDate = session.lastActivityDate {
            attributeSet.contentModificationDate = lastActivityDate
            attributeSet.lastUsedDate = lastActivityDate
        }
        attributeSet.textContent = searchableText(for: entity)
        attributeSet.keywords = searchableKeywords(for: entity)
        attributeSet.associateAppEntity(entity, priority: SpotlightIndexing.entityAssociationPriority)

        let item = CSSearchableItem(
            uniqueIdentifier: entity.id,
            domainIdentifier: SpotlightIdentifiers.domainIdentifier,
            attributeSet: attributeSet
        )
        item.expirationDate = Date.distantFuture

        return item
    }

    private func searchableText(for entity: LooperSessionEntity) -> String {
        [
            entity.searchableText,
            session.metadata.displayTitle,
            session.metadata.gitRepository?.remoteURL ?? "",
            session.metadata.pullRequestURL ?? "",
            session.metadata.kind.label,
            session.metadata.sources.map { "\($0.label) \($0.value)" }.joined(separator: " ")
        ].joined(separator: " ")
    }

    private func searchableKeywords(for entity: LooperSessionEntity) -> [String] {
        (
            entity.attributeSet.keywords ?? []
        ) + [
            session.status.rawValue,
            session.assistantClient.rawValue,
            session.metadata.kind.rawValue,
            session.metadata.taskKind.rawValue
        ] + session.assistantClient.searchKeywords + session.metadata.userFacingTags
    }
}

private extension AssistantClient {
    var spotlightAssistantSurface: CompanionAssistantSurface {
        switch self {
        case .claudeCode:
            return .claudeCode
        case .devin:
            return .devin
        case .grokBuild:
            return .grokBuild
        case .zed:
            return .zed
        case .unknown, .codex, .cursor, .superEngineering, .openclaw:
            return .codex
        }
    }
}

final class SessionSpotlightIndexer: @unchecked Sendable {
    static let shared = SessionSpotlightIndexer()

    private let index: CSSearchableIndex

    private init() {
        self.index = CSSearchableIndex(name: SpotlightIdentifiers.indexName)
    }

    // MARK: - Indexing

    func indexSessions(_ sessions: [SessionSummary]) async throws {
        guard !sessions.isEmpty else {
            return
        }

        var startIndex = sessions.startIndex
        while startIndex < sessions.endIndex {
            let endIndex = sessions.index(
                startIndex,
                offsetBy: SpotlightIndexing.batchSize,
                limitedBy: sessions.endIndex
            ) ?? sessions.endIndex
            let searchableItems = sessions[startIndex..<endIndex].map { session in
                SessionSearchableItem(session: session)
            }
            try await index.indexAppEntities(searchableItems.map(\.appEntity))
            let items = searchableItems.map(\.searchableItem)

            try await index.indexSearchableItems(items)
            startIndex = endIndex
        }
    }

    func indexSession(_ session: SessionSummary) async throws {
        let searchableItem = SessionSearchableItem(session: session)
        try await index.indexAppEntities([searchableItem.appEntity])
        let item = searchableItem.searchableItem
        try await index.indexSearchableItems([item])
    }

    func deleteSession(withId id: String) async throws {
        let identifiers = SessionSpotlightRecord.legacyAndQualifiedSearchableIdentifiers(for: id)
        try await index.deleteAppEntities(identifiedBy: identifiers, ofType: LooperSessionEntity.self)
        try await index.deleteSearchableItems(withIdentifiers: identifiers)
    }

    func deleteSessions(withIDs ids: [String]) async throws {
        guard !ids.isEmpty else {
            return
        }

        try await index.deleteAppEntities(identifiedBy: ids, ofType: LooperSessionEntity.self)
        try await index.deleteSearchableItems(withIdentifiers: ids)
    }

    func deleteAllSessions() async throws {
        try await index.deleteAppEntities(ofType: LooperSessionEntity.self)
        try await index.deleteSearchableItems(withDomainIdentifiers: [SpotlightIdentifiers.domainIdentifier])
    }
}

// MARK: - CSSearchableIndex Extension

extension CSSearchableIndex {
    func indexSearchableItems(_ items: [CSSearchableItem]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            indexSearchableItems(items) { error in
                if let error = error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume()
                }
            }
        }
    }

    func deleteSearchableItems(withIdentifiers identifiers: [String]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            deleteSearchableItems(withIdentifiers: identifiers) { error in
                if let error = error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume()
                }
            }
        }
    }

    func deleteSearchableItems(withDomainIdentifiers domainIdentifiers: [String]) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            deleteSearchableItems(withDomainIdentifiers: domainIdentifiers) { error in
                if let error = error {
                    continuation.resume(throwing: error)
                } else {
                    continuation.resume()
                }
            }
        }
    }
}

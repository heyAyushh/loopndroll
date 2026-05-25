import CoreSpotlight
import UniformTypeIdentifiers

enum SpotlightIdentifiers {
    static let domainIdentifier = "dev.looper.session"
    static let indexName = "LooperSessions"
}

struct SessionSpotlightRecord: Equatable, Sendable {
    let id: String
    let ref: String
    let title: String
    let status: SessionStatus
    let assistantPreview: String?
    let assistantClient: AssistantClient
    let metadata: SessionMetadata

    init(session: SessionSummary) {
        id = session.id
        ref = session.ref
        title = session.title
        status = session.status
        assistantPreview = session.assistantPreview
        assistantClient = session.assistantClient
        metadata = session.metadata
    }
}

struct SessionSearchableItem {
    let session: SessionSummary

    var searchableItem: CSSearchableItem {
        let attributeSet = CSSearchableItemAttributeSet(contentType: .text)
        attributeSet.title = session.title
        attributeSet.contentDescription = session.assistantPreview ?? "Session \(session.ref)"
        attributeSet.textContent = [
            session.ref,
            session.title,
            session.assistantPreview ?? "",
            session.status.label,
            session.assistantClient.displayTitle,
            session.metadata.displayTitle,
            session.metadata.projectPath ?? "",
            session.metadata.kind.label,
            session.metadata.installedPlugins.map(\.name).joined(separator: " ")
        ].joined(separator: " ")
        attributeSet.keywords = [
            session.ref,
            session.status.rawValue,
            session.assistantClient.rawValue,
            session.status.label,
            session.metadata.kind.rawValue
        ] + session.assistantClient.searchKeywords + session.metadata.tags
        attributeSet.displayName = session.title

        let item = CSSearchableItem(
            uniqueIdentifier: session.id,
            domainIdentifier: SpotlightIdentifiers.domainIdentifier,
            attributeSet: attributeSet
        )
        item.expirationDate = Date.distantFuture

        return item
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

        let items = sessions.map { SessionSearchableItem(session: $0).searchableItem }

        try await index.indexSearchableItems(items)
    }

    func indexSession(_ session: SessionSummary) async throws {
        let item = SessionSearchableItem(session: session).searchableItem
        try await index.indexSearchableItems([item])
    }

    func deleteSession(withId id: String) async throws {
        try await index.deleteSearchableItems(withIdentifiers: [id])
    }

    func deleteSessions(withIDs ids: [String]) async throws {
        guard !ids.isEmpty else {
            return
        }

        try await index.deleteSearchableItems(withIdentifiers: ids)
    }

    func deleteAllSessions() async throws {
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

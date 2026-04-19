import CoreSpotlight
import MobileCoreServices

enum SpotlightIdentifiers {
    static let domainIdentifier = "dev.looper.session"
    static let indexName = "LooperSessions"
}

struct SessionSearchableItem {
    let session: SessionSummary

    var searchableItem: CSSearchableItem {
        let attributeSet = CSSearchableItemAttributeSet(itemContentType: kUTTypeText as String)
        attributeSet.title = session.title
        attributeSet.contentDescription = session.assistantPreview ?? "Session \(session.ref)"
        attributeSet.textContent = "\(session.title) \(session.assistantPreview ?? "") \(session.status.label)"

        // Keywords for better search matching
        attributeSet.keywords = [
            session.ref,
            session.status.rawValue,
            session.assistantClient.rawValue,
            session.status.label
        ]

        // Display name and alternate names
        attributeSet.displayName = session.title

        let item = CSSearchableItem(
            uniqueIdentifier: session.id,
            domainIdentifier: SpotlightIdentifiers.domainIdentifier,
            attributeSet: attributeSet
        )

        // Mark as update to avoid overwriting existing attributes
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

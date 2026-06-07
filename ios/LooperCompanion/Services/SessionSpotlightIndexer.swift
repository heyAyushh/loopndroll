import CoreSpotlight
import UniformTypeIdentifiers

private enum SpotlightIndexing {
    static let batchSize = 100
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
        attributeSet.contentURL = LooperContinuationActivity.sessionDeepLinkURL(sessionID: session.id)
        attributeSet.textContent = [
            session.ref,
            session.title,
            session.assistantPreview ?? "",
            session.status.label,
            session.assistantClient.displayTitle,
            session.metadata.displayTitle,
            session.metadata.sourceDisplayName,
            session.metadata.projectPath ?? "",
            session.metadata.taskKind.label,
            session.metadata.gitRepository?.repositoryName ?? "",
            session.metadata.gitRepository?.remoteURL ?? "",
            session.metadata.gitRepository?.branch ?? "",
            session.metadata.pullRequestURL ?? "",
            session.metadata.kind.label,
            session.metadata.installedPlugins.map(\.name).joined(separator: " "),
            session.metadata.sources.map(\.value).joined(separator: " ")
        ].joined(separator: " ")
        attributeSet.keywords = [
            session.ref,
            session.status.rawValue,
            session.assistantClient.rawValue,
            session.status.label,
            session.metadata.kind.rawValue,
            session.metadata.taskKind.rawValue
        ] + session.assistantClient.searchKeywords + session.metadata.userFacingTags
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

        var startIndex = sessions.startIndex
        while startIndex < sessions.endIndex {
            let endIndex = sessions.index(
                startIndex,
                offsetBy: SpotlightIndexing.batchSize,
                limitedBy: sessions.endIndex
            ) ?? sessions.endIndex
            let items = sessions[startIndex..<endIndex].map {
                SessionSearchableItem(session: $0).searchableItem
            }

            try await index.indexSearchableItems(items)
            startIndex = endIndex
        }
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

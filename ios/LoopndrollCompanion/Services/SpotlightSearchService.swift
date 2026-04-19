import CoreSpotlight

@MainActor
final class SpotlightSearchService: ObservableObject {
    @Published var searchResults: [CSSearchableItem] = []
    @Published var isSearching = false

    private var currentQuery: CSUserQuery?

    // MARK: - Semantic Search

    func performSearch(query: String, maxRankedResults: Int = 5) async {
        if #available(iOS 26, *) {
            await performModernSearch(query: query, maxRankedResults: maxRankedResults)
        } else {
            await performLegacySearch(query: query)
        }
    }

    @available(iOS 26, *)
    private func performModernSearch(query: String, maxRankedResults: Int = 5) async {
        guard !query.isEmpty else {
            searchResults = []
            return
        }

        isSearching = true
        defer { isSearching = false }

        // Configure query context for semantic search
        let queryContext = CSUserQueryContext()
        queryContext.fetchAttributes = [
            "title",
            "contentDescription"
        ]

        // Enable ranked results for better relevance
        queryContext.enableRankedResults = true
        queryContext.maxRankedResultCount = maxRankedResults

        // Filter to only our domain
        queryContext.filterQueries = ["domainIdentifier == '\(SpotlightIdentifiers.domainIdentifier)'"]

        // Create and start query
        let userQuery = CSUserQuery(
            userQueryString: query,
            userQueryContext: queryContext
        )

        currentQuery = userQuery

        do {
            var results: [CSSearchableItem] = []

            for try await result in userQuery.results {
                results.append(result.item)
            }

            searchResults = results

        } catch {
            print("Search error: \(error)")
            searchResults = []
        }
    }

    @available(iOS, deprecated: 26)
    private func performLegacySearch(query: String) async {
        guard !query.isEmpty else {
            searchResults = []
            return
        }

        isSearching = true
        defer { isSearching = false }

        // Use CSSearchQuery for older iOS versions
        let context = CSSearchQueryContext()
        context.fetchAttributes = ["title", "contentDescription", "keywords"]

        let searchQuery = CSSearchQuery(
            queryString: "contentDescription ==[c] '*\(query)*' || title ==[c] '*\(query)*'c",
            queryContext: context
        )

        do {
            var results: [CSSearchableItem] = []
            for try await result in searchQuery.results {
                results.append(result.item)
            }
            searchResults = results
        } catch {
            print("Legacy search error: \(error)")
            searchResults = []
        }
    }

    func prepareForSearch() async {
        if #available(iOS 26, *) {
            await CSUserQuery.prepare()
        }
        // No-op for older iOS versions
    }

    func cancelSearch() {
        currentQuery?.cancel()
        currentQuery = nil
        isSearching = false
    }
}

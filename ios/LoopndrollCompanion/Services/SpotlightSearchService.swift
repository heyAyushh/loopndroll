import Combine
import CoreSpotlight

@MainActor
final class SpotlightSearchService: ObservableObject {
    @Published var searchResults: [CSSearchableItem] = []
    @Published var isSearching = false

    private var currentQuery: CSUserQuery?

    // MARK: - Semantic Search

    func performSearch(query: String, maxRankedResults: Int = 5) async {
        cancelSearch()

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

        let queryContext = CSUserQueryContext()
        queryContext.fetchAttributes = [
            "title",
            "contentDescription"
        ]
        queryContext.enableRankedResults = true
        queryContext.maxRankedResultCount = maxRankedResults
        queryContext.filterQueries = ["domainIdentifier == '\(SpotlightIdentifiers.domainIdentifier)'"]

        let userQuery = CSUserQuery(
            userQueryString: query,
            userQueryContext: queryContext
        )

        currentQuery = userQuery
        isSearching = true
        defer {
            if currentQuery === userQuery {
                isSearching = false
            }
        }

        do {
            var results: [CSSearchableItem] = []

            for try await result in userQuery.results {
                guard !Task.isCancelled, currentQuery === userQuery else {
                    return
                }

                results.append(result.item)
            }

            if currentQuery === userQuery {
                searchResults = results
            }

        } catch {
            if currentQuery === userQuery {
                print("Search error: \(error)")
                searchResults = []
            }
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

        let context = CSSearchQueryContext()
        context.fetchAttributes = ["title", "contentDescription", "keywords"]
        let escapedQuery = query.replacingOccurrences(of: "'", with: "\\'")

        let searchQuery = CSSearchQuery(
            queryString: "contentDescription ==[c] '*\(escapedQuery)*' || title ==[c] '*\(escapedQuery)*'",
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
            CSUserQuery.prepare()
        }
    }

    func cancelSearch() {
        currentQuery?.cancel()
        currentQuery = nil
        isSearching = false
    }
}

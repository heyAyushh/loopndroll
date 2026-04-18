import SwiftUI
import UIKit

private enum DeviceHubSheetPresentation {
    static let openFraction: CGFloat = 0.75
}

enum SessionSearchScope: String, CaseIterable, Identifiable {
    case all
    case sessions
    case actions
    case settings
    case device

    var id: String { rawValue }

    var title: String {
        switch self {
        case .all:
            return "All"
        case .sessions:
            return "Sessions"
        case .actions:
            return "Actions"
        case .settings:
            return "Settings"
        case .device:
            return "Device"
        }
    }

    /// Native symbols for search scope controls (see Apple Human Interface Guidelines: Search Fields).
    var systemImage: String {
        switch self {
        case .all:
            return "square.grid.2x2"
        case .sessions:
            return "bubble.left.and.bubble.right"
        case .actions:
            return "bolt.circle"
        case .settings:
            return "gearshape"
        case .device:
            return "iphone.gen3"
        }
    }
}

enum GlobalSearchActionCategory: String {
    case quickActions
    case device
}

enum GlobalSearchAction: String, CaseIterable, Hashable, Identifiable {
    case openDeviceHub
    case sendTestAlert
    case openNotificationSettings

    var id: String { rawValue }

    var category: GlobalSearchActionCategory {
        switch self {
        case .openDeviceHub:
            return .device
        case .sendTestAlert, .openNotificationSettings:
            return .quickActions
        }
    }

    var title: String {
        switch self {
        case .openDeviceHub:
            return "Open Device Hub"
        case .sendTestAlert:
            return "Send Test Alert"
        case .openNotificationSettings:
            return "Open Notification Settings"
        }
    }

    var subtitle: String {
        switch self {
        case .openDeviceHub:
            return "Open the device sheet for iPhone details and orb tools."
        case .sendTestAlert:
            return "Check alert delivery on this iPhone."
        case .openNotificationSettings:
            return "Open the iOS settings page for looper notifications."
        }
    }

    var systemImage: String {
        switch self {
        case .openDeviceHub:
            return "iphone.gen3"
        case .sendTestAlert:
            return "bell.badge"
        case .openNotificationSettings:
            return "gearshape"
        }
    }

    var keywords: [String] {
        switch self {
        case .openDeviceHub:
            return ["device", "device hub", "iphone", "orb", "scan", "scanner"]
        case .sendTestAlert:
            return ["send test alert", "test alert", "alert", "notification", "push", "bell"]
        case .openNotificationSettings:
            return ["notification settings", "notifications", "settings", "permission", "alerts"]
        }
    }
}

enum GlobalSearchResult: Identifiable, Hashable {
    case session(SessionSummary)
    case settings(SettingsSearchTarget)
    case action(GlobalSearchAction)

    var id: String {
        switch self {
        case let .session(session):
            return "session:\(session.id)"
        case let .settings(target):
            return "settings:\(target.rawValue)"
        case let .action(action):
            return "action:\(action.rawValue)"
        }
    }
}

private struct SearchSuggestionItem: Identifiable, Hashable {
    let id: String
    let title: String
    let subtitle: String
    let systemImage: String
    let completion: String
}

private func normalizedSessionSearchQuery(_ searchText: String) -> String {
    searchText.trimmingCharacters(in: .whitespacesAndNewlines)
}

private func bestSearchMatchScore(
    query: String,
    candidates: [String]
) -> Int? {
    let normalizedQuery = normalizedSessionSearchQuery(query).localizedLowercase

    guard !normalizedQuery.isEmpty else {
        return 0
    }

    var bestScore: Int?

    for candidate in candidates {
        let normalizedCandidate = candidate.localizedLowercase

        let score: Int?
        if normalizedCandidate == normalizedQuery {
            score = 0
        } else if normalizedCandidate.hasPrefix(normalizedQuery) {
            score = 1
        } else if normalizedCandidate.contains(normalizedQuery) {
            score = 2
        } else {
            score = nil
        }

        if let score {
            bestScore = min(bestScore ?? score, score)
        }
    }

    return bestScore
}

private func filterSessions(
    _ sessions: [SessionSummary],
    matching searchText: String
) -> [SessionSummary] {
    let trimmedSearchText = normalizedSessionSearchQuery(searchText)

    guard !trimmedSearchText.isEmpty else {
        return sessions
    }

    let scoredSessions: [(session: SessionSummary, score: Int)] = sessions.compactMap { session in
        let score = bestSearchMatchScore(
            query: trimmedSearchText,
            candidates: [
                session.ref,
                session.title,
                session.assistantPreview ?? "",
                session.status.label,
                session.status.rawValue,
                session.status == .waiting || session.status == .stopped ? "needs attention" : "",
                session.assistantClient.displayTitle
            ] + session.assistantClient.searchKeywords
        )

        guard let score else {
            return nil
        }

        return (session: session, score: score)
    }

    return scoredSessions
        .sorted { lhs, rhs in
            if lhs.score != rhs.score {
                return lhs.score < rhs.score
            }

            return lhs.session.lastUpdatedAt > rhs.session.lastUpdatedAt
        }
        .map(\.session)
}

private enum CompanionSearchStorage {
    static let recentQueriesKey = "dev.looper.search.recentQueries"
}

private func companionPersistRecentSearchQuery(_ query: String) {
    let trimmedQuery = normalizedSessionSearchQuery(query)
    guard !trimmedQuery.isEmpty else {
        return
    }

    let existing = UserDefaults.standard.string(forKey: CompanionSearchStorage.recentQueriesKey) ?? ""
    let recent = existing.split(separator: "\n").map(String.init).filter { !$0.isEmpty }
    let updatedQueries = [trimmedQuery] + recent.filter { $0 != trimmedQuery }
    UserDefaults.standard.set(
        Array(updatedQueries.prefix(6)).joined(separator: "\n"),
        forKey: CompanionSearchStorage.recentQueriesKey
    )
}

private func companionTabSearchUniquedSearchResults(_ results: [GlobalSearchResult]) -> [GlobalSearchResult] {
    var seenIDs = Set<String>()
    var uniqueResults: [GlobalSearchResult] = []

    for result in results {
        guard seenIDs.insert(result.id).inserted else {
            continue
        }

        uniqueResults.append(result)
    }

    return uniqueResults
}

private func companionTabSearchSuggestedTopResults(model: CompanionAppModel) -> [GlobalSearchResult] {
    let suggestedSessions = Array(model.needsAttentionSessions.prefix(2)).map(GlobalSearchResult.session)
    let suggestedActions: [GlobalSearchResult] = [
        .action(.openDeviceHub),
        .action(.sendTestAlert),
        .settings(.connection),
        .settings(.continuePrompt)
    ]

    return companionTabSearchUniquedSearchResults(suggestedSessions + suggestedActions)
}

private func companionTabSearchSuggestionItem(for result: GlobalSearchResult) -> SearchSuggestionItem? {
    switch result {
    case let .session(session):
        return SearchSuggestionItem(
            id: "session:\(session.id)",
            title: session.title,
            subtitle: "\(session.ref) · \(session.assistantClient.displayTitle)",
            systemImage: session.assistantClient.systemImageName,
            completion: session.ref
        )
    case let .settings(target):
        return SearchSuggestionItem(
            id: "settings:\(target.rawValue)",
            title: target.title,
            subtitle: target.subtitle,
            systemImage: target.systemImage,
            completion: target.title
        )
    case let .action(action):
        return SearchSuggestionItem(
            id: "action:\(action.rawValue)",
            title: action.title,
            subtitle: action.subtitle,
            systemImage: action.systemImage,
            completion: action.title
        )
    }
}

private func companionTabSearchUniquedSuggestionItems(_ items: [SearchSuggestionItem]) -> [SearchSuggestionItem] {
    var seenIDs = Set<String>()
    var seenCompletions = Set<String>()
    var uniqueItems: [SearchSuggestionItem] = []

    for item in items {
        let normalizedCompletion = item.completion.localizedLowercase

        guard
            seenIDs.insert(item.id).inserted,
            seenCompletions.insert(normalizedCompletion).inserted
        else {
            continue
        }

        uniqueItems.append(item)
    }

    return uniqueItems
}

/// Suggestions attached to `TabView.searchable` so they participate in the system search field (Liquid Glass search tab).
private struct CompanionTabSearchSuggestionsPanel: View {
    let model: CompanionAppModel
    @Binding var searchText: String
    @Environment(\.isSearching) private var isSearching

    @ViewBuilder
    var body: some View {
        if isSearching && normalizedSessionSearchQuery(searchText).isEmpty {
            suggestionRows
        }
    }

    @ViewBuilder
    private var suggestionRows: some View {
        let items = buildEmptyQuerySuggestionItems()
        if !items.isEmpty {
            ForEach(Array(items.prefix(8))) { suggestion in
                Button {
                    searchText = suggestion.completion
                } label: {
                    SearchSuggestionRow(suggestion: suggestion)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func buildEmptyQuerySuggestionItems() -> [SearchSuggestionItem] {
        let raw = UserDefaults.standard.string(forKey: CompanionSearchStorage.recentQueriesKey) ?? ""
        let recentSearches = raw.split(separator: "\n").map(String.init).filter { !$0.isEmpty }
        let recentItems = recentSearches.prefix(4).map { query in
            SearchSuggestionItem(
                id: "recent:\(query)",
                title: query,
                subtitle: "Recent Search",
                systemImage: "clock.arrow.circlepath",
                completion: query
            )
        }

        let suggestedItems = companionTabSearchSuggestedTopResults(model: model).compactMap { companionTabSearchSuggestionItem(for: $0) }
        return companionTabSearchUniquedSuggestionItems(Array(recentItems + suggestedItems))
    }
}

extension View {
    /// Mirrors Apple’s `TabView` + `.searchable` + `tabViewSearchActivation` pattern so search uses the system Liquid Glass search tab chrome.
    func companionTabViewSearchChrome(
        model: CompanionAppModel,
        searchText: Binding<String>,
        selectedScope: Binding<SessionSearchScope>
    ) -> some View {
        self
            .searchable(
                text: searchText,
                placement: .automatic,
                prompt: String(localized: "Sessions, settings, or actions")
            )
            .searchPresentationToolbarBehavior(.automatic)
            .searchScopes(selectedScope) {
                ForEach(SessionSearchScope.allCases) { scope in
                    Label(scope.title, systemImage: scope.systemImage).tag(scope)
                }
            }
            .searchSuggestions {
                CompanionTabSearchSuggestionsPanel(
                    model: model,
                    searchText: searchText
                )
            }
            .onSubmit(of: .search) {
                companionPersistRecentSearchQuery(searchText.wrappedValue)
            }
    }
}

struct SessionsScreen: View {
    let model: CompanionAppModel
    let openSettings: () -> Void

    @State private var isDeviceHubPresented = false

    private var shouldShowArchivedSection: Bool {
        !model.archivedSessions.isEmpty
    }

    private var hasVisibleSessions: Bool {
        !model.needsAttentionSessions.isEmpty ||
            !model.runningSessions.isEmpty ||
            !model.archivedSessions.isEmpty
    }

    var body: some View {
        NavigationStack {
            List {
                connectionSection

                if !model.needsAttentionSessions.isEmpty {
                    sessionSection(title: "Needs Attention", sessions: model.needsAttentionSessions)
                }

                if !model.runningSessions.isEmpty {
                    sessionSection(title: "Active", sessions: model.runningSessions)
                }

                if shouldShowArchivedSection, !model.archivedSessions.isEmpty {
                    sessionSection(title: "Archived", sessions: model.archivedSessions)
                }
            }
            .listStyle(.insetGrouped)
            .contentMargins(.top, 0, for: .scrollContent)
            .companionListSurface()
            .navigationTitle("Sessions")
            .navigationDestination(for: SessionSummary.self) { session in
                SessionDetailScreen(model: model, session: session)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        isDeviceHubPresented = true
                    } label: {
                        SessionsToolbarOrbButton()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Open device hub")
                }
            }
            .refreshable {
                await model.refresh()
            }
            .overlay {
                overlayState
            }
        }
        .sheet(isPresented: $isDeviceHubPresented) {
            SessionsDeviceHubSheet(model: model)
                .deviceHubSheetPresentation()
        }
    }

    @ViewBuilder
    private var overlayState: some View {
        if model.isLoading && model.snapshot == nil {
            ProgressView("Loading Looper")
        } else if !hasVisibleSessions {
            ContentUnavailableView(
                unavailableStateTitle,
                systemImage: unavailableStateSymbolName,
                description: Text(unavailableStateMessage)
            )
        }
    }

    private var connectionSection: some View {
        Section {
            Button {
                openSettings()
            } label: {
                SessionConnectionRow(
                    title: model.connectivityHeadline,
                    subtitle: connectionSubtitle,
                    statusText: model.connectionState.label,
                    statusTint: CompanionTint.tint(for: model.connectionState)
                )
            }
            .buttonStyle(.plain)
            .companionCardRowSurface()
        } footer: {
            if let errorMessage = model.errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .foregroundStyle(.red)
            }
        }
    }

    private var connectionSubtitle: String {
        guard let host = model.snapshot?.host else {
            return model.connectivitySummary
        }

        return "Last synced \(ModelFormatting.relativeTimestamp(host.lastSyncedAt))"
    }

    private func sessionSection(
        title: String,
        sessions: [SessionSummary]
    ) -> some View {
        Section(title) {
            ForEach(sessions) { session in
                NavigationLink(value: session) {
                    SessionRow(session: session)
                }
                .companionCardRowSurface()
            }
        }
    }

    private var unavailableStateTitle: String {
        switch model.connectionState {
        case .connected:
            return "No Sessions"
        case .connecting:
            return "Connecting to Your Mac"
        case .offline:
            return "Mac Offline"
        case .unauthorized:
            return "Connection Needs Approval"
        case .unpaired:
            return "Set Up Your Mac Link"
        }
    }

    private var unavailableStateMessage: String {
        model.connectivitySummary
    }

    private var unavailableStateSymbolName: String {
        model.connectionState.symbolName
    }
}

struct SessionSearchScreen: View {
    let model: CompanionAppModel
    @Binding var searchText: String
    @Binding var selectedScope: SessionSearchScope

    @Environment(\.openURL) private var openURL
    @Environment(\.isSearching) private var isSearching
    @AppStorage(CompanionSearchStorage.recentQueriesKey) private var recentSearchesStorage = ""
    @State private var isDeviceHubPresented = false

    var body: some View {
        NavigationStack {
            List {
                if trimmedSearchText.isEmpty {
                    if !isSearching {
                        if !recentSearches.isEmpty {
                            recentSearchesSection
                        }

                        if !topResults.isEmpty {
                            globalSearchTopResultsSection(title: "Suggested")
                        }
                    }
                } else {
                    if !topResults.isEmpty {
                        globalSearchTopResultsSection(title: "Top Hits")
                    }

                    if shouldShowSessionResults, !visibleNeedsAttentionSessions.isEmpty {
                        globalSearchSessionSection(title: "Needs Attention", sessions: visibleNeedsAttentionSessions)
                    }

                    if shouldShowSessionResults, !visibleRunningSessions.isEmpty {
                        globalSearchSessionSection(title: "Active", sessions: visibleRunningSessions)
                    }

                    if shouldShowSessionResults, !visibleArchivedSessions.isEmpty {
                        globalSearchSessionSection(title: "Archived", sessions: visibleArchivedSessions)
                    }

                    if shouldShowQuickActionResults, !visibleQuickActions.isEmpty {
                        globalSearchActionSection(title: "Quick Actions", actions: visibleQuickActions)
                    }

                    if shouldShowSettingsResults, !visibleSettingsTargets.isEmpty {
                        globalSearchSettingsSection(title: "Settings", targets: visibleSettingsTargets)
                    }

                    if shouldShowDeviceResults, !visibleDeviceActions.isEmpty {
                        globalSearchActionSection(title: "Device", actions: visibleDeviceActions)
                    }
                }
            }
            .listStyle(.insetGrouped)
            .scrollDismissesKeyboard(.interactively)
            .contentMargins(.top, 0, for: .scrollContent)
            .companionListSurface()
            .navigationTitle("")
            .navigationBarTitleDisplayMode(.inline)
            .navigationDestination(for: SessionSummary.self) { session in
                SessionDetailScreen(model: model, session: session)
            }
            .navigationDestination(for: SettingsSearchTarget.self) { target in
                globalSearchSettingsDestinationView(for: target)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        isDeviceHubPresented = true
                    } label: {
                        SessionsToolbarOrbButton()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Open device hub")
                }
            }
            .refreshable {
                await model.refresh()
            }
            .overlay {
                overlayState
            }
        }
        .sheet(isPresented: $isDeviceHubPresented) {
            SessionsDeviceHubSheet(model: model)
                .deviceHubSheetPresentation()
        }
    }

    @ViewBuilder
    private var overlayState: some View {
        if model.isLoading && model.snapshot == nil {
            ProgressView("Loading Search")
        } else if trimmedSearchText.isEmpty && isSearching {
            EmptyView()
        } else if !hasVisibleSearchResults {
            ContentUnavailableView(
                searchUnavailableStateTitle,
                systemImage: searchUnavailableStateSymbolName,
                description: Text(searchUnavailableStateMessage)
            )
        }
    }

    private var allSessions: [SessionSummary] {
        model.snapshot?.sessions ?? []
    }

    private var filteredAllSessions: [SessionSummary] {
        filterSessions(allSessions, matching: searchText)
    }

    private var filteredNeedsAttentionSessions: [SessionSummary] {
        filterSessions(model.needsAttentionSessions, matching: searchText)
    }

    private var filteredRunningSessions: [SessionSummary] {
        filterSessions(model.runningSessions, matching: searchText)
    }

    private var filteredArchivedSessions: [SessionSummary] {
        filterSessions(model.archivedSessions, matching: searchText)
    }

    private var trimmedSearchText: String {
        normalizedSessionSearchQuery(searchText)
    }

    private var filteredSettingsTargets: [SettingsSearchTarget] {
        matchingSettingsTargets(for: trimmedSearchText)
    }

    private var filteredQuickActions: [GlobalSearchAction] {
        matchingActions(in: .quickActions, for: trimmedSearchText)
    }

    private var filteredDeviceActions: [GlobalSearchAction] {
        matchingActions(in: .device, for: trimmedSearchText)
    }

    private var recentSearches: [String] {
        recentSearchesStorage
            .split(separator: "\n")
            .map(String.init)
            .filter { !$0.isEmpty }
    }

    private var shouldShowSessionResults: Bool {
        selectedScope == .all || selectedScope == .sessions
    }

    private var shouldShowQuickActionResults: Bool {
        selectedScope == .all || selectedScope == .actions
    }

    private var shouldShowSettingsResults: Bool {
        selectedScope == .all || selectedScope == .settings
    }

    private var shouldShowDeviceResults: Bool {
        selectedScope == .all || selectedScope == .device
    }

    private var topResults: [GlobalSearchResult] {
        guard selectedScope == .all else {
            return []
        }

        if trimmedSearchText.isEmpty {
            return suggestedTopResults()
        }

        let sessionResults = filteredAllSessions.prefix(3).map(GlobalSearchResult.session)
        let actionResults = filteredQuickActions.prefix(2).map(GlobalSearchResult.action)
        let settingsResults = filteredSettingsTargets.prefix(2).map(GlobalSearchResult.settings)
        let deviceResults = filteredDeviceActions.prefix(1).map(GlobalSearchResult.action)

        return uniquedSearchResults(
            Array(sessionResults + actionResults + settingsResults + deviceResults)
        )
    }

    private var topResultIDs: Set<String> {
        Set(topResults.map(\.id))
    }

    private var visibleNeedsAttentionSessions: [SessionSummary] {
        filteredNeedsAttentionSessions.filter { session in
            !topResultIDs.contains(GlobalSearchResult.session(session).id)
        }
    }

    private var visibleRunningSessions: [SessionSummary] {
        filteredRunningSessions.filter { session in
            !topResultIDs.contains(GlobalSearchResult.session(session).id)
        }
    }

    private var visibleArchivedSessions: [SessionSummary] {
        filteredArchivedSessions.filter { session in
            !topResultIDs.contains(GlobalSearchResult.session(session).id)
        }
    }

    private var visibleQuickActions: [GlobalSearchAction] {
        filteredQuickActions.filter { action in
            !topResultIDs.contains(GlobalSearchResult.action(action).id)
        }
    }

    private var visibleDeviceActions: [GlobalSearchAction] {
        filteredDeviceActions.filter { action in
            !topResultIDs.contains(GlobalSearchResult.action(action).id)
        }
    }

    private var visibleSettingsTargets: [SettingsSearchTarget] {
        filteredSettingsTargets.filter { target in
            !topResultIDs.contains(GlobalSearchResult.settings(target).id)
        }
    }

    private var hasVisibleSearchResults: Bool {
        if trimmedSearchText.isEmpty {
            if isSearching {
                return !searchSuggestionItems.isEmpty
            }

            return !topResults.isEmpty || !recentSearches.isEmpty
        }

        return (shouldShowSessionResults && (
            !filteredNeedsAttentionSessions.isEmpty ||
                !filteredRunningSessions.isEmpty ||
                !filteredArchivedSessions.isEmpty
        )) ||
            (shouldShowQuickActionResults && !filteredQuickActions.isEmpty) ||
            (shouldShowDeviceResults && !filteredDeviceActions.isEmpty) ||
            (shouldShowSettingsResults && !filteredSettingsTargets.isEmpty) ||
            !topResults.isEmpty
    }

    private var searchSuggestionItems: [SearchSuggestionItem] {
        if trimmedSearchText.isEmpty {
            let recentItems = recentSearches.prefix(4).map { query in
                SearchSuggestionItem(
                    id: "recent:\(query)",
                    title: query,
                    subtitle: "Recent Search",
                    systemImage: "clock.arrow.circlepath",
                    completion: query
                )
            }
            let suggestedItems = suggestedTopResults().compactMap(searchSuggestionItem(for:))
            return uniquedSuggestionItems(Array(recentItems + suggestedItems))
        }

        let resultSuggestions = topResults.compactMap(searchSuggestionItem(for:))
        let completionSuggestions = searchCompletions
            .filter { completion in
                completion.localizedCaseInsensitiveContains(trimmedSearchText)
            }
            .map { completion in
                SearchSuggestionItem(
                    id: "completion:\(completion)",
                    title: completion,
                    subtitle: "Suggested Query",
                    systemImage: "magnifyingglass",
                    completion: completion
                )
            }

        return uniquedSuggestionItems(Array(resultSuggestions + completionSuggestions))
    }

    private var recentSearchesSection: some View {
        Section("Recent Searches") {
            ForEach(recentSearches, id: \.self) { query in
                Button {
                    searchText = query
                } label: {
                    SearchSuggestionRow(
                        suggestion: SearchSuggestionItem(
                            id: "recent:\(query)",
                            title: query,
                            subtitle: "Recent Search",
                            systemImage: "clock.arrow.circlepath",
                            completion: query
                        )
                    )
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func globalSearchSessionSection(
        title: String,
        sessions: [SessionSummary]
    ) -> some View {
        Section(title) {
            ForEach(sessions) { session in
                NavigationLink(value: session) {
                    SearchSessionRow(session: session)
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func globalSearchTopResultsSection(title: String) -> some View {
        Section(title) {
            ForEach(topResults) { result in
                searchResultRow(for: result)
            }
        }
    }

    private func globalSearchSettingsSection(
        title: String,
        targets: [SettingsSearchTarget]
    ) -> some View {
        Section(title) {
            ForEach(targets) { target in
                NavigationLink(value: target) {
                    SearchCommandRow(
                        title: target.title,
                        subtitle: target.subtitle,
                        systemImage: target.systemImage,
                        categoryLabel: "Settings"
                    )
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func globalSearchActionSection(
        title: String,
        actions: [GlobalSearchAction]
    ) -> some View {
        Section(title) {
            ForEach(actions) { action in
                Button {
                    runSearchAction(action)
                } label: {
                    SearchCommandRow(
                        title: action.title,
                        subtitle: action.subtitle,
                        systemImage: action.systemImage,
                        categoryLabel: title
                    )
                }
                .buttonStyle(.plain)
            }
        }
    }

    @ViewBuilder
    private func searchResultRow(for result: GlobalSearchResult) -> some View {
        switch result {
        case let .session(session):
            NavigationLink(value: session) {
                SearchSessionRow(session: session)
            }
            .buttonStyle(.plain)
        case let .settings(target):
            NavigationLink(value: target) {
                SearchCommandRow(
                    title: target.title,
                    subtitle: target.subtitle,
                    systemImage: target.systemImage,
                    categoryLabel: "Settings"
                )
            }
            .buttonStyle(.plain)
        case let .action(action):
            Button {
                runSearchAction(action)
            } label: {
                SearchCommandRow(
                    title: action.title,
                    subtitle: action.subtitle,
                    systemImage: action.systemImage,
                    categoryLabel: action.category == .device ? "Device" : "Quick Action"
                )
            }
            .buttonStyle(.plain)
        }
    }

    @ViewBuilder
    private func globalSearchSettingsDestinationView(for target: SettingsSearchTarget) -> some View {
        switch target {
        case .notificationRoutes:
            SettingsRoutesScreen(model: model)
        case .completionChecks:
            SettingsCompletionChecksScreen(model: model)
        case .connection, .continuePrompt, .stopQuickActions:
            SettingsScreen(
                model: model,
                initialSearchTarget: target,
                embedsInNavigationStack: false
            )
        }
    }

    private func matchingSettingsTargets(for query: String) -> [SettingsSearchTarget] {
        let scoredTargets: [(target: SettingsSearchTarget, score: Int)] = SettingsSearchTarget.allCases.compactMap { target in
            let score = bestSearchMatchScore(
                query: query,
                candidates: [target.title, target.subtitle, "settings"] + target.keywords
            )

            guard let score else {
                return nil
            }

            return (target: target, score: score)
        }

        return scoredTargets
            .sorted { lhs, rhs in
                if lhs.score != rhs.score {
                    return lhs.score < rhs.score
                }

                return lhs.target.title < rhs.target.title
            }
            .map(\.target)
    }

    private func matchingActions(
        in category: GlobalSearchActionCategory,
        for query: String
    ) -> [GlobalSearchAction] {
        let scoredActions: [(action: GlobalSearchAction, score: Int)] = GlobalSearchAction.allCases
            .filter { $0.category == category }
            .compactMap { action in
                let score = bestSearchMatchScore(
                    query: query,
                    candidates: [action.title, action.subtitle] + action.keywords
                )

                guard let score else {
                    return nil
                }

                return (action: action, score: score)
            }

        return scoredActions
            .sorted { lhs, rhs in
                if lhs.score != rhs.score {
                    return lhs.score < rhs.score
                }

                return lhs.action.title < rhs.action.title
            }
            .map(\.action)
    }

    private func suggestedTopResults() -> [GlobalSearchResult] {
        companionTabSearchSuggestedTopResults(model: model)
    }

    private func uniquedSearchResults(_ results: [GlobalSearchResult]) -> [GlobalSearchResult] {
        companionTabSearchUniquedSearchResults(results)
    }

    private var searchCompletions: [String] {
        [
            "Needs Attention",
            "Archived",
            "Connect to Mac",
            "Continue Prompt",
            "Open Device Hub",
            "Notification Routes",
            "Send Test Alert"
        ]
    }

    private func runSearchAction(_ action: GlobalSearchAction) {
        switch action {
        case .openDeviceHub:
            isDeviceHubPresented = true
        case .sendTestAlert:
            Task {
                await model.sendTestAlert()
            }
        case .openNotificationSettings:
            guard let settingsURL = URL(string: UIApplication.openSettingsURLString) else {
                return
            }

            openURL(settingsURL)
        }
    }

    private func storeRecentSearch(_ query: String) {
        companionPersistRecentSearchQuery(query)
    }

    private func searchSuggestionItem(for result: GlobalSearchResult) -> SearchSuggestionItem? {
        companionTabSearchSuggestionItem(for: result)
    }

    private func uniquedSuggestionItems(_ items: [SearchSuggestionItem]) -> [SearchSuggestionItem] {
        companionTabSearchUniquedSuggestionItems(items)
    }

    private var searchUnavailableStateTitle: String {
        if trimmedSearchText.isEmpty {
            return "Search Looper"
        }

        return "No Results"
    }

    private var searchUnavailableStateMessage: String {
        if trimmedSearchText.isEmpty {
            return "Find sessions, settings, and device actions."
        }

        return "Try a session ref, a setting like Continue Prompt, or an action like Send Test Alert."
    }

    private var searchUnavailableStateSymbolName: String {
        "magnifyingglass"
    }
}

private extension View {
    func deviceHubSheetPresentation() -> some View {
        presentationDetents([.fraction(DeviceHubSheetPresentation.openFraction), .large])
            .presentationDragIndicator(.visible)
    }
}

private struct SessionConnectionRow: View {
    let title: String
    let subtitle: String
    let statusText: String
    let statusTint: Color

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline)
                    .foregroundStyle(.primary)

                Text(subtitle)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)
            }

            Spacer(minLength: 12)

            StatusPill(text: statusText, tint: statusTint)
        }
        .padding(.vertical, 4)
    }
}

private struct SearchCommandRow: View {
    let title: String
    let subtitle: String
    let systemImage: String
    let categoryLabel: String?

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: systemImage)
                .font(.body.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 24)

            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.body.weight(.medium))
                    .foregroundStyle(.primary)

                if let categoryLabel {
                    Text(categoryLabel)
                        .font(.caption2.weight(.medium))
                        .foregroundStyle(.tertiary)
                }

                Text(subtitle)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)
            }

            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .padding(.vertical, 4)
    }
}

private struct SearchSessionRow: View {
    let session: SessionSummary

    private var tint: Color {
        CompanionTint.tint(for: session.status)
    }

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(spacing: 6) {
                AssistantClientGlyph(client: session.assistantClient)
                Image(systemName: session.status.symbolName)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(tint)
                    .accessibilityHidden(true)
            }
            .frame(width: 28)

            VStack(alignment: .leading, spacing: 4) {
                Text(session.title)
                    .font(.body.weight(.medium))
                    .foregroundStyle(.primary)
                    .lineLimit(2)

                Text(session.ref)
                    .font(.footnote.monospaced())
                    .foregroundStyle(.secondary)

                if let assistantPreview = session.assistantPreview, !assistantPreview.isEmpty {
                    Text(assistantPreview)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }

            Spacer(minLength: 12)

            VStack(alignment: .trailing, spacing: 4) {
                Text(ModelFormatting.relativeTimestamp(session.lastUpdatedAt))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                Text(session.status.label)
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(tint)
            }
        }
        .contentShape(Rectangle())
        .padding(.vertical, 4)
    }
}

private struct SearchSuggestionRow: View {
    let suggestion: SearchSuggestionItem

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: suggestion.systemImage)
                .font(.body.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: 24, height: 24)

            VStack(alignment: .leading, spacing: 4) {
                Text(suggestion.title)
                    .font(.body.weight(.medium))
                    .foregroundStyle(.primary)

                Text(suggestion.subtitle)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer(minLength: 0)
        }
    }
}

#Preview {
    let model = CompanionAppModel(environment: CompanionEnvironment(service: MockCompanionService()))
    model.snapshot = PreviewFixtures.snapshot

    return SessionsScreen(model: model, openSettings: {})
}

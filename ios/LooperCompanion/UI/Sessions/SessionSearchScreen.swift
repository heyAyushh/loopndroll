import SwiftUI
import UIKit

struct SessionSearchScreen: View {
    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator
    @Binding var searchText: String
    @Binding var selectedScope: SessionSearchScope
    @ObservedObject var searchService: SpotlightSearchService

    @Environment(\.openURL) private var openURL
    @Environment(\.dismissSearch) private var dismissSearch
    @AppStorage(CompanionSearchStorage.recentQueriesKey) private var recentSearchesStorage = ""
    @State private var searchPath = NavigationPath()
    @State private var isDeviceHubPresented = false

    var body: some View {
        NavigationStack(path: $searchPath) {
            List {
                if trimmedSearchText.isEmpty {
                    recentSearchesSection
                    topResultsSection(title: "Suggested")
                } else {
                    topResultsSection(title: "Top Hits")
                    searchSections
                }
            }
            .listStyle(.insetGrouped)
            .scrollDismissesKeyboard(.interactively)
            .contentMargins(.top, 0, for: .scrollContent)
            .companionListSurface()
            .navigationTitle("Search")
            .navigationDestination(for: SessionSummary.self) { session in
                SessionDetailScreen(model: model, session: session)
            }
            .navigationDestination(for: SettingsSearchTarget.self) { target in
                settingsDestination(for: target)
            }
            .toolbar {
                searchToolbar
            }
            .refreshable {
                await model.refresh()
            }
            .overlay {
                overlayState
            }
            .task(id: searchTaskID) {
                await performSemanticSearch()
            }
            .task {
                await searchService.prepareForSearch()
            }
            .onDisappear {
                searchService.cancelSearch()
            }
            .searchable(
                text: $searchText,
                placement: .automatic,
                prompt: Text("Sessions, settings, actions")
            )
            .searchScopes($selectedScope, activation: .onTextEntry) {
                ForEach(SessionSearchScope.allCases) { scope in
                    Label(scope.title, systemImage: scope.systemImage).tag(scope)
                }
            }
            .searchSuggestions {
                ForEach(recentSearches.prefix(3), id: \.self) { query in
                    Button(query) {
                        searchText = query
                    }
                    .searchCompletion(query)
                }
            }
            .onSubmit(of: .search) {
                SessionSearchEngine.persistRecentQuery(searchText)
            }
        }
        .sheet(isPresented: $isDeviceHubPresented) {
            SessionsDeviceHubSheet(model: model)
                .deviceHubSheetPresentation()
        }
    }

    @ToolbarContentBuilder
    private var searchToolbar: some ToolbarContent {
        ToolbarItem(placement: .topBarTrailing) {
            Button {
                isDeviceHubPresented = true
            } label: {
                SessionsToolbarOrbButton()
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open device hub")
        }

        ToolbarItemGroup(placement: .keyboard) {
            Menu {
                Picker("Scope", selection: $selectedScope) {
                    ForEach(SessionSearchScope.allCases) { scope in
                        Label(scope.title, systemImage: scope.systemImage).tag(scope)
                    }
                }
            } label: {
                Label(selectedScope.title, systemImage: selectedScope.systemImage)
            }
            .accessibilityLabel("Filter search scope")

            Spacer()

            Button("Done") {
                dismissSearch()
            }
        }
    }

    @ViewBuilder
    private var searchSections: some View {
        if shouldShowSessionResults {
            searchSessionSection(title: "Needs Attention", sessions: visibleNeedsAttentionSessions)
            searchSessionSection(title: "Active", sessions: visibleRunningSessions)
            searchSessionSection(title: "Recent", sessions: visibleStoppedSessions)
            searchSessionSection(title: "Archived", sessions: visibleArchivedSessions)
        }

        if shouldShowQuickActionResults {
            actionSection(title: "Quick Actions", actions: visibleQuickActions)
        }

        if shouldShowSettingsResults {
            settingsSection(title: "Settings", targets: visibleSettingsTargets)
        }

        if shouldShowDeviceResults {
            actionSection(title: "Device", actions: visibleDeviceActions)
        }
    }

    @ViewBuilder
    private var overlayState: some View {
        if model.isLoading && model.snapshot == nil {
            ProgressView("Loading Search")
        } else if !hasVisibleSearchResults {
            ContentUnavailableView(
                trimmedSearchText.isEmpty ? "Search Looper" : "No Results",
                systemImage: "magnifyingglass",
                description: Text(searchUnavailableMessage)
            )
        }
    }

    @ViewBuilder
    private var recentSearchesSection: some View {
        if !recentSearches.isEmpty {
            Section("Recent Searches") {
                ForEach(recentSearches, id: \.self) { query in
                    let match = bestRecentSearchResult(for: query)
                    Button {
                        activateRecentSearch(query)
                    } label: {
                        RecentSearchRow(
                            query: query,
                            breadcrumb: recentSearchBreadcrumb(for: match),
                            systemImage: recentSearchSystemImage(for: match)
                        )
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    @ViewBuilder
    private func topResultsSection(title: String) -> some View {
        if !topResults.isEmpty {
            Section(title) {
                ForEach(topResults) { result in
                    resultRow(for: result)
                }
            }
        }
    }

    @ViewBuilder
    private func searchSessionSection(title: String, sessions: [SessionSummary]) -> some View {
        if !sessions.isEmpty {
            Section(title) {
                ForEach(sessions) { session in
                    NavigationLink(value: session) {
                        SearchSessionRow(
                            session: session,
                            assistantSurface: model.selectedAssistantSurface
                        )
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func settingsSection(title: String, targets: [SettingsSearchTarget]) -> some View {
        if !targets.isEmpty {
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
                }
            }
        }
    }

    @ViewBuilder
    private func actionSection(title: String, actions: [GlobalSearchAction]) -> some View {
        if !actions.isEmpty {
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
    }

    @ViewBuilder
    private func resultRow(for result: GlobalSearchResult) -> some View {
        switch result {
        case let .session(session):
            NavigationLink(value: session) {
                SearchSessionRow(
                    session: session,
                    assistantSurface: model.selectedAssistantSurface
                )
            }
        case let .settings(target):
            NavigationLink(value: target) {
                SearchCommandRow(
                    title: target.title,
                    subtitle: target.subtitle,
                    systemImage: target.systemImage,
                    categoryLabel: "Settings"
                )
            }
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
    private func settingsDestination(for target: SettingsSearchTarget) -> some View {
        switch target {
        case .notificationRoutes:
            SettingsRoutesScreen(model: model)
        case .completionChecks:
            SettingsCompletionChecksScreen(model: model)
        case .connection, .continuePrompt, .stopQuickActions, .security:
            SettingsScreen(
                model: model,
                authenticator: authenticator,
                initialSearchTarget: target,
                embedsInNavigationStack: false
            )
        }
    }

    @MainActor
    private func performSemanticSearch() async {
        guard selectedScope == .all || selectedScope == .sessions else {
            searchService.cancelSearch()
            return
        }

        guard !trimmedSearchText.isEmpty else {
            searchService.cancelSearch()
            return
        }

        try? await Task.sleep(for: .milliseconds(120))
        guard !Task.isCancelled else {
            return
        }

        await searchService.performSearch(query: trimmedSearchText)
    }

    private var trimmedSearchText: String {
        SessionSearchEngine.normalized(searchText)
    }

    private var searchTaskID: String {
        "\(selectedScope.rawValue):\(trimmedSearchText)"
    }

    private var allSessions: [SessionSummary] {
        model.snapshot?.sessionsAcrossSurfaces ?? []
    }

    private var localSessionResults: [SessionSummary] {
        SessionSearchEngine.sessions(allSessions, matching: trimmedSearchText)
    }

    private var spotlightSessionResults: [SessionSummary] {
        guard !trimmedSearchText.isEmpty else {
            return []
        }

        let spotlightSessionIDs = Set(
            searchService.searchResults.compactMap { result in
                LooperSessionEntityIdentifier(rawValue: result.uniqueIdentifier)?.sessionID
                    ?? result.uniqueIdentifier
            }
        )
        return allSessions.filter { spotlightSessionIDs.contains($0.id) }
    }

    private var filteredAllSessions: [SessionSummary] {
        var combined = spotlightSessionResults
        let spotlightIDs = Set(spotlightSessionResults.map(\.id))
        combined.append(contentsOf: localSessionResults.filter { !spotlightIDs.contains($0.id) })
        return combined
    }

    private var filteredNeedsAttentionSessions: [SessionSummary] {
        filteredSessions(from: model.needsAttentionSessions)
    }

    private var filteredRunningSessions: [SessionSummary] {
        filteredSessions(from: model.runningSessions)
    }

    private var filteredStoppedSessions: [SessionSummary] {
        filteredSessions(from: model.stoppedSessions)
    }

    private var filteredArchivedSessions: [SessionSummary] {
        filteredSessions(from: model.archivedSessions)
    }

    private var filteredSettingsTargets: [SettingsSearchTarget] {
        SessionSearchEngine.settingsTargets(for: trimmedSearchText)
    }

    private var filteredQuickActions: [GlobalSearchAction] {
        SessionSearchEngine.actions(in: .quickActions, for: trimmedSearchText)
    }

    private var filteredDeviceActions: [GlobalSearchAction] {
        SessionSearchEngine.actions(in: .device, for: trimmedSearchText)
    }

    private var recentSearches: [String] {
        SessionSearchEngine.recentQueries(from: recentSearchesStorage)
    }

    private var topResults: [GlobalSearchResult] {
        guard selectedScope == .all else {
            return []
        }

        if trimmedSearchText.isEmpty {
            return SessionSearchEngine.suggestedTopResults(model: model)
        }

        return SessionSearchEngine.uniqueResults(
            Array(filteredAllSessions.prefix(3).map(GlobalSearchResult.session)) +
                Array(filteredQuickActions.prefix(2).map(GlobalSearchResult.action)) +
                Array(filteredSettingsTargets.prefix(2).map(GlobalSearchResult.settings)) +
                Array(filteredDeviceActions.prefix(1).map(GlobalSearchResult.action))
        )
    }

    private var topResultIDs: Set<String> {
        Set(topResults.map(\.id))
    }

    private var visibleNeedsAttentionSessions: [SessionSummary] {
        withoutTopResults(filteredNeedsAttentionSessions)
    }

    private var visibleRunningSessions: [SessionSummary] {
        withoutTopResults(filteredRunningSessions)
    }

    private var visibleStoppedSessions: [SessionSummary] {
        withoutTopResults(filteredStoppedSessions)
    }

    private var visibleArchivedSessions: [SessionSummary] {
        withoutTopResults(filteredArchivedSessions)
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

    private var hasVisibleSearchResults: Bool {
        if trimmedSearchText.isEmpty {
            return !topResults.isEmpty || !recentSearches.isEmpty
        }

        return (shouldShowSessionResults && (
            !filteredNeedsAttentionSessions.isEmpty ||
                !filteredRunningSessions.isEmpty ||
                !filteredStoppedSessions.isEmpty ||
                !filteredArchivedSessions.isEmpty
        )) ||
            (shouldShowQuickActionResults && !filteredQuickActions.isEmpty) ||
            (shouldShowDeviceResults && !filteredDeviceActions.isEmpty) ||
            (shouldShowSettingsResults && !filteredSettingsTargets.isEmpty) ||
            !topResults.isEmpty
    }

    private var searchUnavailableMessage: String {
        if trimmedSearchText.isEmpty {
            return "Find sessions, settings, and device actions."
        }

        return "Try a session ref, a setting like Continue Prompt, or an action like Send Test Alert."
    }

    private func filteredSessions(from sessions: [SessionSummary]) -> [SessionSummary] {
        SessionSearchEngine.sessions(sessions, matching: trimmedSearchText)
    }

    private func withoutTopResults(_ sessions: [SessionSummary]) -> [SessionSummary] {
        sessions.filter { session in
            !topResultIDs.contains(GlobalSearchResult.session(session).id)
        }
    }

    private func runSearchAction(_ action: GlobalSearchAction) {
        dismissSearch()
        Haptics.selectionChanged()

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

    private func activateRecentSearch(_ query: String) {
        let trimmedQuery = SessionSearchEngine.normalized(query)

        guard !trimmedQuery.isEmpty else {
            return
        }

        SessionSearchEngine.persistRecentQuery(trimmedQuery)
        searchText = trimmedQuery

        if let result = bestRecentSearchResult(for: trimmedQuery) {
            activateSearchResult(result)
        } else {
            Haptics.selectionChanged()
        }
    }

    private func bestRecentSearchResult(for query: String) -> GlobalSearchResult? {
        SessionSearchEngine.bestRecentSearchResult(for: query, allSessions: allSessions)
    }

    private func activateSearchResult(_ result: GlobalSearchResult) {
        switch result {
        case let .session(session):
            dismissSearch()
            Haptics.selectionChanged()
            searchPath.append(session)
        case let .settings(target):
            dismissSearch()
            Haptics.selectionChanged()
            searchPath.append(target)
        case let .action(action):
            runSearchAction(action)
        }
    }

    private func recentSearchBreadcrumb(for result: GlobalSearchResult?) -> String {
        guard let result else {
            return "Recent search"
        }

        switch result {
        case let .session(session):
            return "Sessions -> \(session.ref)"
        case let .settings(target):
            return "Settings -> \(target.title)"
        case let .action(action):
            let category = action.category == .device ? "Device" : "Actions"
            return "\(category) -> \(action.title)"
        }
    }

    private func recentSearchSystemImage(for result: GlobalSearchResult?) -> String {
        guard let result else {
            return "clock.arrow.circlepath"
        }

        switch result {
        case .session:
            return "bubble.left.and.bubble.right"
        case let .settings(target):
            return target.systemImage
        case let .action(action):
            return action.systemImage
        }
    }
}

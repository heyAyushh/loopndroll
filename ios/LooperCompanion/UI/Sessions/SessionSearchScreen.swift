import SwiftUI
import UIKit

private enum SearchScopeFilterLayout {
    static let horizontalSpacing: CGFloat = 8
    static let horizontalPadding: CGFloat = 12
    static let verticalPadding: CGFloat = 6
}

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
    @State private var renderedResults = SessionSearchResults.empty

    var body: some View {
        NavigationStack(path: $searchPath) {
            List {
                scopeFilterSection

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
            .task(id: searchResultsTaskID) {
                await updateRenderedSearchResults()
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
                    Label(scope.title, systemImage: scope.systemImage)
                        .tag(scope)
                        .accessibilityIdentifier("search.scope.\(scope.rawValue)")
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
            .accessibilityIdentifier("search.open-device-hub")
        }

        ToolbarItemGroup(placement: .keyboard) {
            searchScopeMenu

            Spacer()

            Button("Done") {
                dismissSearch()
            }
        }
    }

    private var searchScopeMenu: some View {
        Menu {
            Picker("Scope", selection: $selectedScope) {
                ForEach(SessionSearchScope.allCases) { scope in
                    Label(scope.title, systemImage: scope.systemImage)
                        .tag(scope)
                        .accessibilityIdentifier("search.scope.\(scope.rawValue)")
                }
            }
        } label: {
            Label(selectedScope.title, systemImage: selectedScope.systemImage)
        }
        .accessibilityLabel("Filter search scope")
        .accessibilityIdentifier("search.scope.menu")
    }

    private var scopeFilterSection: some View {
        Section {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: SearchScopeFilterLayout.horizontalSpacing) {
                    ForEach(SessionSearchScope.allCases) { scope in
                        searchScopeButton(for: scope)
                    }
                }
                .padding(.vertical, SearchScopeFilterLayout.verticalPadding)
            }
            .accessibilityIdentifier("search.scope.scroller")
            .listRowInsets(
                EdgeInsets(
                    top: 0,
                    leading: SearchScopeFilterLayout.horizontalPadding,
                    bottom: 0,
                    trailing: SearchScopeFilterLayout.horizontalPadding
                )
            )
        }
    }

    private func searchScopeButton(for scope: SessionSearchScope) -> some View {
        Button {
            selectedScope = scope
            Haptics.selectionChanged()
        } label: {
            Label(scope.title, systemImage: scope.systemImage)
                .font(.footnote.weight(scope == selectedScope ? .semibold : .regular))
                .padding(.horizontal, SearchScopeFilterLayout.horizontalPadding)
                .padding(.vertical, SearchScopeFilterLayout.verticalPadding)
                .background(
                    scope == selectedScope ?
                        Color.accentColor.opacity(0.16) :
                        Color.secondary.opacity(0.10),
                    in: Capsule()
                )
                .foregroundStyle(scope == selectedScope ? Color.accentColor : Color.primary)
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("search.scope.\(scope.rawValue)")
        .accessibilityLabel("\(scope.title) search scope")
        .accessibilityValue(scope == selectedScope ? "Selected" : "")
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
                    .accessibilityIdentifier("search.recent-query.\(query)")
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
                    .accessibilityIdentifier("search.settings.\(target.rawValue)")
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
                .accessibilityIdentifier("search.action.\(action.rawValue)")
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
            .accessibilityIdentifier("search.settings.\(target.rawValue)")
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
            .accessibilityIdentifier("search.action.\(action.rawValue)")
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

    private var searchResultsTaskID: String {
        [
            selectedScope.rawValue,
            trimmedSearchText,
            model.sessionIndex.identity,
            spotlightResultIDs.joined(separator: ","),
        ].joined(separator: "|")
    }

    private var spotlightResultIDs: [String] {
        searchService.searchResults.map(\.uniqueIdentifier)
    }

    @MainActor
    private func updateRenderedSearchResults() async {
        let searchText = trimmedSearchText
        let currentScope = selectedScope
        let allSessions = model.sessionIndex.allSessions
        let needsAttentionSessions = model.needsAttentionSessions
        let runningSessions = model.runningSessions
        let stoppedSessions = model.stoppedSessions
        let archivedSessions = model.archivedSessions
        let spotlightResultSessionIDs = spotlightResultIDs

        let nextResults = await Task.detached(priority: .userInitiated) {
            SessionSearchResults(
                searchText: searchText,
                selectedScope: currentScope,
                allSessions: allSessions,
                needsAttentionSessions: needsAttentionSessions,
                runningSessions: runningSessions,
                stoppedSessions: stoppedSessions,
                archivedSessions: archivedSessions,
                spotlightResultSessionIDs: spotlightResultSessionIDs
            )
        }.value

        guard !Task.isCancelled else {
            return
        }

        renderedResults = nextResults
    }

    private var allSessions: [SessionSummary] {
        model.sessionIndex.allSessions
    }

    private var recentSearches: [String] {
        SessionSearchEngine.recentQueries(from: recentSearchesStorage)
    }

    private var topResults: [GlobalSearchResult] {
        renderedResults.topResults
    }

    private var visibleNeedsAttentionSessions: [SessionSummary] {
        renderedResults.visibleNeedsAttentionSessions
    }

    private var visibleRunningSessions: [SessionSummary] {
        renderedResults.visibleRunningSessions
    }

    private var visibleStoppedSessions: [SessionSummary] {
        renderedResults.visibleStoppedSessions
    }

    private var visibleArchivedSessions: [SessionSummary] {
        renderedResults.visibleArchivedSessions
    }

    private var visibleQuickActions: [GlobalSearchAction] {
        renderedResults.visibleQuickActions
    }

    private var visibleDeviceActions: [GlobalSearchAction] {
        renderedResults.visibleDeviceActions
    }

    private var visibleSettingsTargets: [SettingsSearchTarget] {
        renderedResults.visibleSettingsTargets
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
            renderedResults.hasSessionResults
        )) ||
            (shouldShowQuickActionResults && renderedResults.hasQuickActionResults) ||
            (shouldShowDeviceResults && renderedResults.hasDeviceResults) ||
            (shouldShowSettingsResults && renderedResults.hasSettingsResults) ||
            !topResults.isEmpty
    }

    private var searchUnavailableMessage: String {
        if trimmedSearchText.isEmpty {
            return "Find sessions, settings, and device actions."
        }

        return "Try a session ref, a setting like Continue Prompt, or an action like Send Test Alert."
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

import Foundation

enum SessionSearchScope: String, CaseIterable, Identifiable, Sendable {
    case all
    case sessions
    case actions
    case settings
    case device

    var id: String {
        rawValue
    }

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

enum GlobalSearchActionCategory: String, Sendable {
    case quickActions
    case device
}

enum GlobalSearchAction: String, CaseIterable, Hashable, Identifiable, Sendable {
    case openDeviceHub
    case sendTestAlert
    case openNotificationSettings

    var id: String {
        rawValue
    }

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
            return "Open iPhone details and orb tools."
        case .sendTestAlert:
            return "Check alert delivery on this iPhone."
        case .openNotificationSettings:
            return "Open the iOS settings page for Looper notifications."
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

enum GlobalSearchResult: Identifiable, Hashable, Sendable {
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

struct SessionSearchResults: Sendable {
    static let empty = SessionSearchResults(
        topResults: [],
        visibleNeedsAttentionSessions: [],
        visibleRunningSessions: [],
        visibleStoppedSessions: [],
        visibleArchivedSessions: [],
        visibleQuickActions: [],
        visibleDeviceActions: [],
        visibleSettingsTargets: [],
        hasSessionResults: false,
        hasQuickActionResults: false,
        hasDeviceResults: false,
        hasSettingsResults: false
    )

    let topResults: [GlobalSearchResult]
    let visibleNeedsAttentionSessions: [SessionSummary]
    let visibleRunningSessions: [SessionSummary]
    let visibleStoppedSessions: [SessionSummary]
    let visibleArchivedSessions: [SessionSummary]
    let visibleQuickActions: [GlobalSearchAction]
    let visibleDeviceActions: [GlobalSearchAction]
    let visibleSettingsTargets: [SettingsSearchTarget]
    let hasSessionResults: Bool
    let hasQuickActionResults: Bool
    let hasDeviceResults: Bool
    let hasSettingsResults: Bool

    init(
        searchText: String,
        selectedScope: SessionSearchScope,
        allSessions: [SessionSummary],
        needsAttentionSessions: [SessionSummary],
        runningSessions: [SessionSummary],
        stoppedSessions: [SessionSummary],
        archivedSessions: [SessionSummary],
        spotlightResultSessionIDs: [String]
    ) {
        let spotlightSessionResults = Self.spotlightSessionResults(
            allSessions: allSessions,
            spotlightResultSessionIDs: spotlightResultSessionIDs,
            searchText: searchText
        )
        let localSessionResults = SessionSearchEngine.sessions(allSessions, matching: searchText)
        let spotlightIDs = Set(spotlightSessionResults.map(\.id))
        let filteredAllSessions = spotlightSessionResults +
            localSessionResults.filter { !spotlightIDs.contains($0.id) }
        let filteredNeedsAttentionSessions = SessionSearchEngine.sessions(
            needsAttentionSessions,
            matching: searchText
        )
        let filteredRunningSessions = SessionSearchEngine.sessions(
            runningSessions,
            matching: searchText
        )
        let filteredStoppedSessions = SessionSearchEngine.sessions(
            stoppedSessions,
            matching: searchText
        )
        let filteredArchivedSessions = SessionSearchEngine.sessions(
            archivedSessions,
            matching: searchText
        )
        let filteredQuickActions = SessionSearchEngine.actions(in: .quickActions, for: searchText)
        let filteredDeviceActions = SessionSearchEngine.actions(in: .device, for: searchText)
        let filteredSettingsTargets = SessionSearchEngine.settingsTargets(for: searchText)
        let topResults = Self.topResults(
            searchText: searchText,
            selectedScope: selectedScope,
            filteredAllSessions: filteredAllSessions,
            filteredQuickActions: filteredQuickActions,
            filteredDeviceActions: filteredDeviceActions,
            filteredSettingsTargets: filteredSettingsTargets,
            needsAttentionSessions: needsAttentionSessions,
            stoppedSessions: stoppedSessions
        )
        let topResultIDs = Set(topResults.map(\.id))

        self.init(
            topResults: topResults,
            visibleNeedsAttentionSessions: Self.withoutTopResults(
                filteredNeedsAttentionSessions,
                topResultIDs: topResultIDs
            ),
            visibleRunningSessions: Self.withoutTopResults(
                filteredRunningSessions,
                topResultIDs: topResultIDs
            ),
            visibleStoppedSessions: Self.withoutTopResults(
                filteredStoppedSessions,
                topResultIDs: topResultIDs
            ),
            visibleArchivedSessions: Self.withoutTopResults(
                filteredArchivedSessions,
                topResultIDs: topResultIDs
            ),
            visibleQuickActions: filteredQuickActions.filter { action in
                !topResultIDs.contains(GlobalSearchResult.action(action).id)
            },
            visibleDeviceActions: filteredDeviceActions.filter { action in
                !topResultIDs.contains(GlobalSearchResult.action(action).id)
            },
            visibleSettingsTargets: filteredSettingsTargets.filter { target in
                !topResultIDs.contains(GlobalSearchResult.settings(target).id)
            },
            hasSessionResults: !filteredNeedsAttentionSessions.isEmpty ||
                !filteredRunningSessions.isEmpty ||
                !filteredStoppedSessions.isEmpty ||
                !filteredArchivedSessions.isEmpty,
            hasQuickActionResults: !filteredQuickActions.isEmpty,
            hasDeviceResults: !filteredDeviceActions.isEmpty,
            hasSettingsResults: !filteredSettingsTargets.isEmpty
        )
    }

    private init(
        topResults: [GlobalSearchResult],
        visibleNeedsAttentionSessions: [SessionSummary],
        visibleRunningSessions: [SessionSummary],
        visibleStoppedSessions: [SessionSummary],
        visibleArchivedSessions: [SessionSummary],
        visibleQuickActions: [GlobalSearchAction],
        visibleDeviceActions: [GlobalSearchAction],
        visibleSettingsTargets: [SettingsSearchTarget],
        hasSessionResults: Bool,
        hasQuickActionResults: Bool,
        hasDeviceResults: Bool,
        hasSettingsResults: Bool
    ) {
        self.topResults = topResults
        self.visibleNeedsAttentionSessions = visibleNeedsAttentionSessions
        self.visibleRunningSessions = visibleRunningSessions
        self.visibleStoppedSessions = visibleStoppedSessions
        self.visibleArchivedSessions = visibleArchivedSessions
        self.visibleQuickActions = visibleQuickActions
        self.visibleDeviceActions = visibleDeviceActions
        self.visibleSettingsTargets = visibleSettingsTargets
        self.hasSessionResults = hasSessionResults
        self.hasQuickActionResults = hasQuickActionResults
        self.hasDeviceResults = hasDeviceResults
        self.hasSettingsResults = hasSettingsResults
    }

    private static func topResults(
        searchText: String,
        selectedScope: SessionSearchScope,
        filteredAllSessions: [SessionSummary],
        filteredQuickActions: [GlobalSearchAction],
        filteredDeviceActions: [GlobalSearchAction],
        filteredSettingsTargets: [SettingsSearchTarget],
        needsAttentionSessions: [SessionSummary],
        stoppedSessions: [SessionSummary]
    ) -> [GlobalSearchResult] {
        guard selectedScope == .all else {
            return []
        }

        if searchText.isEmpty {
            return SessionSearchEngine.suggestedTopResults(
                needsAttentionSessions: needsAttentionSessions,
                stoppedSessions: stoppedSessions
            )
        }

        return SessionSearchEngine.uniqueResults(
            Array(filteredAllSessions.prefix(3).map(GlobalSearchResult.session)) +
                Array(filteredQuickActions.prefix(2).map(GlobalSearchResult.action)) +
                Array(filteredSettingsTargets.prefix(2).map(GlobalSearchResult.settings)) +
                Array(filteredDeviceActions.prefix(1).map(GlobalSearchResult.action))
        )
    }

    private static func spotlightSessionResults(
        allSessions: [SessionSummary],
        spotlightResultSessionIDs: [String],
        searchText: String
    ) -> [SessionSummary] {
        guard !searchText.isEmpty else {
            return []
        }

        let sessionsByID = allSessions.sortedBySessionFreshness().reduce(into: [String: SessionSummary]()) { sessionsByID, session in
            if sessionsByID[session.id] != nil {
                return
            }

            sessionsByID[session.id] = session
        }
        var seenSessionIDs = Set<String>()
        let sessions = spotlightResultSessionIDs.compactMap { resultID -> SessionSummary? in
            let sessionID = LooperSessionEntityIdentifier(rawValue: resultID)?.sessionID ?? resultID
            guard seenSessionIDs.insert(sessionID).inserted else {
                return nil
            }
            return sessionsByID[sessionID]
        }
        return sessions.sortedBySessionFreshness()
    }

    private static func withoutTopResults(
        _ sessions: [SessionSummary],
        topResultIDs: Set<String>
    ) -> [SessionSummary] {
        sessions.filter { session in
            !topResultIDs.contains(GlobalSearchResult.session(session).id)
        }
    }
}

enum CompanionSearchStorage {
    static let recentQueriesKey = "dev.looper.search.recentQueries"
}

enum SessionSearchEngine {
    private struct ScoredSessionMatch {
        let session: SessionSummary
        let score: Int
        let freshnessRank: Int
    }

    static func normalized(_ searchText: String) -> String {
        searchText.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static func sessions(
        _ sessions: [SessionSummary],
        matching searchText: String
    ) -> [SessionSummary] {
        scoredSessions(sessions, matching: searchText).map(\.session)
    }

    static func scoredSessions(
        _ sessions: [SessionSummary],
        matching searchText: String
    ) -> [(session: SessionSummary, score: Int)] {
        let query = normalized(searchText)

        guard !query.isEmpty else {
            return sessions.map { session in
                (session: session, score: SearchMatchScore.exact)
            }
        }

        let matches: [ScoredSessionMatch] = sessions
            .sortedBySessionFreshness()
            .enumerated()
            .compactMap { freshnessRank, session in
            let score = bestMatchScore(
                query: query,
                candidates: [
                    session.ref,
                    session.title,
                    session.assistantPreview ?? "",
                    session.status.label,
                    session.status.rawValue,
                    session.status == .waiting ? "needs attention" : "",
                    session.assistantClient.displayTitle,
                    session.metadata.displayTitle,
                    session.metadata.sourceDisplayName,
                    session.metadata.projectPath ?? "",
                    session.metadata.taskKind.label,
                    session.metadata.taskKind.rawValue,
                    session.metadata.gitRepository?.repositoryName ?? "",
                    session.metadata.gitRepository?.remoteURL ?? "",
                    session.metadata.gitRepository?.branch ?? "",
                    session.metadata.pullRequestURL ?? "",
                    session.metadata.kind.label,
                    session.metadata.kind.rawValue,
                    session.metadata.installedPlugins.map(\.name).joined(separator: " "),
                    session.metadata.sources.map(\.value).joined(separator: " "),
                ] + session.assistantClient.searchKeywords + session.metadata.userFacingTags
            )

            guard let score else {
                return nil
            }

            return ScoredSessionMatch(
                session: session,
                score: score,
                freshnessRank: freshnessRank
            )
        }

        return matches.sorted { lhs, rhs in
            if lhs.score != rhs.score {
                return lhs.score < rhs.score
            }

            return lhs.freshnessRank < rhs.freshnessRank
        }
        .map { match in
            (session: match.session, score: match.score)
        }
    }

    static func settingsTargets(for query: String) -> [SettingsSearchTarget] {
        scoredSettingsTargets(for: query).map(\.target)
    }

    static func actions(
        in category: GlobalSearchActionCategory? = nil,
        for query: String
    ) -> [GlobalSearchAction] {
        scoredActions(in: category, for: query).map(\.action)
    }

    static func recentQueries(from storage: String) -> [String] {
        storage
            .split(separator: "\n")
            .map(String.init)
            .filter { !$0.isEmpty }
    }

    static func persistRecentQuery(_ query: String) {
        let trimmedQuery = normalized(query)

        guard !trimmedQuery.isEmpty else {
            return
        }

        let key = CompanionSearchStorage.recentQueriesKey
        let existing = UserDefaults.standard.string(forKey: key) ?? ""
        let recent = recentQueries(from: existing)
        let nextQueries = [trimmedQuery] + recent.filter { $0 != trimmedQuery }
        UserDefaults.standard.set(Array(nextQueries.prefix(6)).joined(separator: "\n"), forKey: key)
    }

    static func suggestedTopResults(
        needsAttentionSessions: [SessionSummary],
        stoppedSessions: [SessionSummary]
    ) -> [GlobalSearchResult] {
        let suggestedSessions = needsAttentionSessions.isEmpty
            ? stoppedSessions
            : needsAttentionSessions
        let sessions = suggestedSessions.prefix(2).map(GlobalSearchResult.session)
        let actions: [GlobalSearchResult] = [
            .action(.openDeviceHub),
            .action(.sendTestAlert),
            .settings(.connection),
            .settings(.continuePrompt),
        ]

        return uniqueResults(Array(sessions) + actions)
    }

    @MainActor
    static func suggestedTopResults(model: CompanionAppModel) -> [GlobalSearchResult] {
        suggestedTopResults(
            needsAttentionSessions: model.viewState.needsAttentionSessions,
            stoppedSessions: model.viewState.stoppedSessions
        )
    }

    static func uniqueResults(_ results: [GlobalSearchResult]) -> [GlobalSearchResult] {
        var seenIDs = Set<String>()
        var uniqueResults: [GlobalSearchResult] = []

        for result in results where seenIDs.insert(result.id).inserted {
            uniqueResults.append(result)
        }

        return uniqueResults
    }

    static func bestRecentSearchResult(
        for query: String,
        allSessions: [SessionSummary]
    ) -> GlobalSearchResult? {
        let commandResults = scoredCommandResults(for: query)

        if let exactCommandResult = commandResults.first(where: { $0.score == SearchMatchScore.exact }) {
            return exactCommandResult.result
        }

        if let session = scoredSessions(allSessions, matching: query).first,
           session.score <= SearchMatchScore.prefix
        {
            return .session(session.session)
        }

        return commandResults.first?.result
    }

    private static func bestMatchScore(query: String, candidates: [String]) -> Int? {
        let normalizedQuery = normalized(query).localizedLowercase

        guard !normalizedQuery.isEmpty else {
            return SearchMatchScore.exact
        }

        var bestScore: Int?

        for candidate in candidates {
            let normalizedCandidate = candidate.localizedLowercase
            let score: Int?

            if normalizedCandidate == normalizedQuery {
                score = SearchMatchScore.exact
            } else if normalizedCandidate.hasPrefix(normalizedQuery) {
                score = SearchMatchScore.prefix
            } else if normalizedCandidate.contains(normalizedQuery) {
                score = SearchMatchScore.contains
            } else {
                score = nil
            }

            if let score {
                bestScore = min(bestScore ?? score, score)
            }
        }

        return bestScore
    }

    private static func scoredSettingsTargets(
        for query: String
    ) -> [(target: SettingsSearchTarget, score: Int)] {
        SettingsSearchTarget.allCases.compactMap { target in
            let score = bestMatchScore(
                query: query,
                candidates: [target.title, target.subtitle, "settings"] + target.keywords
            )

            guard let score else {
                return nil
            }

            return (target: target, score: score)
        }
        .sorted { lhs, rhs in
            if lhs.score != rhs.score {
                return lhs.score < rhs.score
            }

            return lhs.target.title < rhs.target.title
        }
    }

    private static func scoredActions(
        in category: GlobalSearchActionCategory? = nil,
        for query: String
    ) -> [(action: GlobalSearchAction, score: Int)] {
        GlobalSearchAction.allCases
            .filter { category == nil || $0.category == category }
            .compactMap { action in
                let score = bestMatchScore(
                    query: query,
                    candidates: [action.title, action.subtitle] + action.keywords
                )

                guard let score else {
                    return nil
                }

                return (action: action, score: score)
            }
            .sorted { lhs, rhs in
                if lhs.score != rhs.score {
                    return lhs.score < rhs.score
                }

                return lhs.action.title < rhs.action.title
            }
    }

    private static func scoredCommandResults(
        for query: String
    ) -> [(result: GlobalSearchResult, score: Int)] {
        let settingsResults = scoredSettingsTargets(for: query).map { result in
            (result: GlobalSearchResult.settings(result.target), score: result.score)
        }
        let actionResults = scoredActions(for: query).map { result in
            (result: GlobalSearchResult.action(result.action), score: result.score)
        }

        return (settingsResults + actionResults)
            .sorted { lhs, rhs in
                if lhs.score != rhs.score {
                    return lhs.score < rhs.score
                }

                return lhs.result.id < rhs.result.id
            }
    }
}

private enum SearchMatchScore {
    static let exact = 0
    static let prefix = 1
    static let contains = 2
}

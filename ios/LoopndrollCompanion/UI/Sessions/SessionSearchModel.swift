import Foundation

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

enum CompanionSearchStorage {
    static let recentQueriesKey = "dev.looper.search.recentQueries"
}

enum SessionSearchEngine {
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

        return sessions.compactMap { session in
            let score = bestMatchScore(
                query: query,
                candidates: [
                    session.ref,
                    session.title,
                    session.assistantPreview ?? "",
                    session.status.label,
                    session.status.rawValue,
                    session.status == .waiting || session.status == .stopped ? "needs attention" : "",
                    session.assistantClient.displayTitle,
                    session.metadata.displayTitle,
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
                    session.metadata.sources.map(\.value).joined(separator: " ")
                ] + session.assistantClient.searchKeywords + session.metadata.tags
            )

            guard let score else {
                return nil
            }

            return (session: session, score: score)
        }
        .sorted { lhs, rhs in
            if lhs.score != rhs.score {
                return lhs.score < rhs.score
            }

            return lhs.session.lastUpdatedAt > rhs.session.lastUpdatedAt
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

    @MainActor
    static func suggestedTopResults(model: CompanionAppModel) -> [GlobalSearchResult] {
        let sessions = model.needsAttentionSessions.prefix(2).map(GlobalSearchResult.session)
        let actions: [GlobalSearchResult] = [
            .action(.openDeviceHub),
            .action(.sendTestAlert),
            .settings(.connection),
            .settings(.continuePrompt)
        ]

        return uniqueResults(Array(sessions) + actions)
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
           session.score <= SearchMatchScore.prefix {
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

import AppIntents
import Foundation
import SwiftUI

private enum LooperSiriIntentConstants {
    static let searchResultDialogLimit = 3
    static let snippetPreviewLineLimit = 4
    static let snippetPromptLineLimit = 2
    static let snippetCornerRadius: CGFloat = 8
    static let snippetSpacing: CGFloat = 10
}

private enum LooperDefaultSessionUpdate {
    case unchanged
    case clear
    case set(LooperSessionEntity)
}

private func sendLooperPrompt(
    _ prompt: String,
    to session: LooperSessionEntity,
    action: String
) async throws -> LooperPromptResultEntity {
    let client = LooperSiriSessionClient()
    try await client.sendPrompt(prompt, to: session)

    return LooperPromptResultEntity(
        session: session,
        action: action,
        prompt: prompt,
        result: "Looper sent the prompt to \(session.title)."
    )
}

struct OpenLooperSessionIntent: OpenIntent {
    static let title: LocalizedStringResource = "Open Looper Session"
    static let description = IntentDescription("Open a Looper session from the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    static let openAppWhenRun = true
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Session")
    var target: LooperSessionEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Open \(\.$target)")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog & ShowsSnippetView {
        try LooperSiriOpenSessionRequestStore.save(
            LooperSiriOpenSessionRequest(
                sessionID: target.sessionID,
                assistantSurfaceRawValue: target.assistantSurfaceRawValue
            )
        )

        return .result(dialog: "Opening Looper session \(target.title)") {
            LooperSessionSnippetView(
                label: "Opening",
                session: target,
                detail: "Looper will open this session on iPhone."
            )
        }
    }
}

struct SummarizeLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Summarize Looper Session"
    static let description = IntentDescription("Summarize a Looper session using Apple Foundation Models when available.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Summarize \(\.$session)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()
        let detail = try await client.loadSessionDetail(for: session)
        let summary = await LooperFoundationSessionSummarizer().summarize(detail)

        return .result(
            value: summary,
            dialog: "Looper summarized \(session.title)"
        ) {
            LooperSessionSnippetView(label: "Summary", session: session, detail: summary)
        }
    }
}

struct SearchLooperSessionsIntent: AppIntent {
    static let title: LocalizedStringResource = "Search Looper Sessions"
    static let description = IntentDescription("Search Looper sessions from the paired Mac without sending a prompt.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

    @Parameter(title: "Search Text", requestValueDialog: "What should Looper search for?")
    var searchText: String

    static var parameterSummary: some ParameterSummary {
        Summary("Search Looper sessions for \(\.$searchText)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<[LooperSessionEntity]> & ProvidesDialog & ShowsSnippetView {
        let entities = try await searchEntities(matching: searchText)

        return .result(
            value: entities,
            dialog: "\(dialog(for: entities))"
        ) {
            LooperSessionSearchSnippetView(searchText: searchText, sessions: entities)
        }
    }

    private func searchEntities(matching searchText: String) async throws -> [LooperSessionEntity] {
        if #available(iOS 26.0, macOS 26.0, watchOS 26.0, tvOS 26.0, visionOS 26.0, *) {
            return try await LooperSessionValueQuery().values(for: searchText)
        }

        return try await LooperSiriSessionClient().entities(matching: searchText)
    }

    private func dialog(for entities: [LooperSessionEntity]) -> String {
        guard !entities.isEmpty else {
            return "Looper found no matching sessions."
        }

        let titles = entities
            .prefix(LooperSiriIntentConstants.searchResultDialogLimit)
            .map(\.title)
            .joined(separator: ", ")

        if entities.count == 1 {
            return "Looper found \(titles)."
        }

        return "Looper found \(entities.count) sessions. Top matches: \(titles)."
    }
}

struct CreateLooperPromptIntent: AppIntent {
    static let title: LocalizedStringResource = "Create Looper Prompt"
    static let description = IntentDescription("Create and send a prompt to a Looper session through the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    @Parameter(title: "Prompt", requestValueDialog: "What should Looper send?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Create prompt \(\.$prompt) in \(\.$session)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<LooperPromptResultEntity> & ProvidesDialog & ShowsSnippetView {
        let result = try await sendLooperPrompt(prompt, to: session, action: "Prompt Sent")

        return .result(
            value: result,
            dialog: "Looper sent the prompt to \(session.title)"
        ) {
            LooperPromptResultSnippetView(result: result)
        }
    }
}

struct AskLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Looper Session"
    static let description = IntentDescription("Send a prompt to a Looper session through the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    @Parameter(title: "Prompt", requestValueDialog: "What should Looper send?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Ask \(\.$session) \(\.$prompt)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<LooperPromptResultEntity> & ProvidesDialog & ShowsSnippetView {
        let result = try await sendLooperPrompt(prompt, to: session, action: "Prompt Sent")

        return .result(
            value: result,
            dialog: "Looper sent the prompt to \(session.title)"
        ) {
            LooperPromptResultSnippetView(result: result)
        }
    }
}

struct SetDefaultLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Set Default Looper Session"
    static let description = IntentDescription("Choose the Looper session Siri uses when no session is named.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Use \(\.$session) by default")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()
        try await client.saveDefaultSiriSession(session)

        return .result(dialog: "Looper will use \(session.title) as the default Siri session") {
            LooperSessionSnippetView(
                label: "Default Siri Session",
                session: session,
                detail: "Siri will use this session when you do not name one."
            )
        }
    }
}

struct UpdateDefaultLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Update Default Looper Session"
    static let description = IntentDescription("Change, clear, or leave unchanged the Looper session Siri uses by default.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Session")
    var session: LooperSessionEntity?

    static var parameterSummary: some ParameterSummary {
        Summary("Update default Looper session to \(\.$session)")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()

        switch defaultSessionUpdate {
        case .unchanged:
            let currentSession = try? await client.defaultSiriSessionEntity()
            return .result(dialog: "Looper left the default Siri session unchanged.") {
                LooperDefaultSessionSnippetView(
                    label: "No Change",
                    detail: "The current Siri default session was not changed.",
                    session: currentSession
                )
            }
        case .clear:
            try await client.saveDefaultSiriSession(nil)
            return .result(dialog: "Looper cleared the default Siri session.") {
                LooperDefaultSessionSnippetView(
                    label: "Default Cleared",
                    detail: "Siri will ask for a session when one is needed.",
                    session: nil
                )
            }
        case .set(let selectedSession):
            try await client.saveDefaultSiriSession(selectedSession)
            return .result(dialog: "Looper will use \(selectedSession.title) as the default Siri session.") {
                LooperDefaultSessionSnippetView(
                    label: "Default Updated",
                    detail: "Siri will use this session when you do not name one.",
                    session: selectedSession
                )
            }
        }
    }

    private var defaultSessionUpdate: LooperDefaultSessionUpdate {
        if #available(iOS 18.2, macOS 15.2, watchOS 11.2, tvOS 18.2, visionOS 2.2, *) {
            switch $session.valueState {
            case .unset:
                return .unchanged
            case .set(let selectedSession):
                if let selectedSession {
                    return .set(selectedSession)
                }

                return .clear
            @unknown default:
                return .unchanged
            }
        }

        if let session {
            return .set(session)
        }

        return .clear
    }
}

struct AskDefaultLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Default Looper Session"
    static let description = IntentDescription("Send a prompt to the default Looper session through the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Prompt", requestValueDialog: "What should Looper send?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Ask default Looper session \(\.$prompt)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<LooperPromptResultEntity> & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()
        let session = try await client.defaultSiriSessionEntity()
        let result = try await sendLooperPrompt(prompt, to: session, action: "Default Prompt Sent")

        return .result(
            value: result,
            dialog: "Looper sent the prompt to default session \(session.title)"
        ) {
            LooperPromptResultSnippetView(result: result)
        }
    }
}

struct AskCurrentLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Current Looper Session"
    static let description = IntentDescription(
        "Send a prompt to the Looper session currently visible in the app, falling back to the default session."
    )
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Prompt", requestValueDialog: "What should Looper send?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Ask current Looper session \(\.$prompt)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<LooperPromptResultEntity> & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()
        let session = try await client.currentSiriSessionEntity()
        let result = try await sendLooperPrompt(prompt, to: session, action: "Current Prompt Sent")

        return .result(
            value: result,
            dialog: "Looper sent the prompt to current session \(session.title)"
        ) {
            LooperPromptResultSnippetView(result: result)
        }
    }
}

struct AskContextualCurrentLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Current Looper Session With Context"
    static let description = IntentDescription(
        "Send a prompt to the current Looper session with local session context attached."
    )
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Prompt", requestValueDialog: "What should Looper send with context?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Ask current Looper session with context \(\.$prompt)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<LooperPromptResultEntity> & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()
        let session = try await client.currentSiriSessionEntity()
        let detail = try await client.loadSessionDetail(for: session)
        let contextualPrompt = await LooperSessionContextEngine()
            .contextualPrompt(userPrompt: prompt, detail: detail)
        try await client.sendPrompt(contextualPrompt, to: session)
        let result = LooperPromptResultEntity(
            session: session,
            action: "Context Prompt Sent",
            prompt: prompt,
            result: "Looper attached current session context and sent the prompt."
        )

        return .result(
            value: result,
            dialog: "Looper sent the contextual prompt to \(session.title)"
        ) {
            LooperPromptResultSnippetView(result: result)
        }
    }
}

struct SuggestLooperPromptIntent: AppIntent {
    static let title: LocalizedStringResource = "Suggest Looper Prompt"
    static let description = IntentDescription("Suggest a short next prompt for a Looper session.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Suggest a prompt for \(\.$session)")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog & ShowsSnippetView {
        let client = LooperSiriSessionClient()
        let detail = try await client.loadSessionDetail(for: session)
        let suggestion = await LooperSessionContextEngine()
            .suggestions(for: detail)
            .first ?? "Continue from the latest result."

        return .result(
            value: suggestion,
            dialog: "Looper suggests: \(suggestion)"
        ) {
            LooperPromptSuggestionSnippetView(session: session, suggestion: suggestion)
        }
    }
}

struct DeleteLooperSessionsIntent: DeleteIntent {
    static let title: LocalizedStringResource = "Delete Looper Sessions"
    static let description = IntentDescription("Delete Looper sessions from the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication
    @available(iOS 27.0, macOS 27.0, watchOS 27.0, tvOS 27.0, visionOS 27.0, *)
    static var allowedExecutionTargets: IntentExecutionTargets { .main }

    @Parameter(title: "Sessions")
    var entities: [LooperSessionEntity]

    static var parameterSummary: some ParameterSummary {
        Summary("Delete \(\.$entities)")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog & ShowsSnippetView {
        guard !entities.isEmpty else {
            return .result(dialog: "Looper did not receive any sessions to delete.") {
                LooperDeleteSnippetView(deletedCount: 0, sessions: [])
            }
        }

        try await requestConfirmation(
            actionName: .custom(
                acceptLabel: "Delete",
                acceptAlternatives: [],
                denyLabel: "Cancel",
                denyAlternatives: [],
                destructive: true
            ),
            dialog: "Delete \(deleteDialogSubject)?"
        )

        let deletedCount = try await LooperSiriSessionClient().deleteSessions(entities)

        return .result(dialog: "\(deleteDialogPrefix(for: deletedCount)) deleted.") {
            LooperDeleteSnippetView(deletedCount: deletedCount, sessions: entities)
        }
    }

    private var deleteDialogSubject: String {
        if entities.count == 1, let entity = entities.first {
            return "Looper session \(entity.title)"
        }

        return "\(entities.count) Looper sessions"
    }

    private func deleteDialogPrefix(for count: Int) -> String {
        count == 1 ? "1 Looper session" : "\(count) Looper sessions"
    }
}

private struct LooperSessionSnippetView: View {
    let label: String
    let session: LooperSessionEntity
    let detail: String

    var body: some View {
        VStack(alignment: .leading, spacing: LooperSiriIntentConstants.snippetSpacing) {
            LooperSnippetHeader(label: label, title: session.title)

            HStack(spacing: 8) {
                Label(session.assistant, systemImage: "sparkles")
                Label(session.status, systemImage: "circle.fill")
            }
            .font(.caption)
            .foregroundStyle(.secondary)

            if !detail.isEmpty {
                Text(detail)
                    .font(.callout)
                    .foregroundStyle(.primary)
                    .lineLimit(LooperSiriIntentConstants.snippetPreviewLineLimit)
            }
        }
        .looperSnippetSurface()
    }
}

private struct LooperSessionSearchSnippetView: View {
    let searchText: String
    let sessions: [LooperSessionEntity]

    var body: some View {
        VStack(alignment: .leading, spacing: LooperSiriIntentConstants.snippetSpacing) {
            LooperSnippetHeader(label: "Search", title: searchText)

            if sessions.isEmpty {
                Text("No matching Looper sessions.")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(Array(sessions.prefix(LooperSiriIntentConstants.searchResultDialogLimit)), id: \.id) { session in
                    LooperSnippetSessionRow(session: session)
                }
            }
        }
        .looperSnippetSurface()
    }
}

private struct LooperPromptResultSnippetView: View {
    let result: LooperPromptResultEntity

    var body: some View {
        VStack(alignment: .leading, spacing: LooperSiriIntentConstants.snippetSpacing) {
            LooperSnippetHeader(
                label: result.action,
                title: result.session?.title ?? "Looper Session"
            )

            Text(result.prompt)
                .font(.callout.weight(.medium))
                .foregroundStyle(.primary)
                .lineLimit(LooperSiriIntentConstants.snippetPromptLineLimit)

            Text(result.result)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(LooperSiriIntentConstants.snippetPreviewLineLimit)
        }
        .looperSnippetSurface()
    }
}

private struct LooperPromptSuggestionSnippetView: View {
    let session: LooperSessionEntity
    let suggestion: String

    var body: some View {
        VStack(alignment: .leading, spacing: LooperSiriIntentConstants.snippetSpacing) {
            LooperSnippetHeader(label: "Suggested Prompt", title: session.title)

            Text(suggestion)
                .font(.callout.weight(.medium))
                .foregroundStyle(.primary)
                .lineLimit(LooperSiriIntentConstants.snippetPreviewLineLimit)
        }
        .looperSnippetSurface()
    }
}

private struct LooperDefaultSessionSnippetView: View {
    let label: String
    let detail: String
    let session: LooperSessionEntity?

    var body: some View {
        VStack(alignment: .leading, spacing: LooperSiriIntentConstants.snippetSpacing) {
            LooperSnippetHeader(label: label, title: session?.title ?? "No Default Session")

            if let session {
                LooperSnippetSessionRow(session: session)
            }

            Text(detail)
                .font(.callout)
                .foregroundStyle(.secondary)
                .lineLimit(LooperSiriIntentConstants.snippetPreviewLineLimit)
        }
        .looperSnippetSurface()
    }
}

private struct LooperDeleteSnippetView: View {
    let deletedCount: Int
    let sessions: [LooperSessionEntity]

    var body: some View {
        VStack(alignment: .leading, spacing: LooperSiriIntentConstants.snippetSpacing) {
            LooperSnippetHeader(label: "Deleted", title: deletedTitle)

            ForEach(Array(sessions.prefix(LooperSiriIntentConstants.searchResultDialogLimit)), id: \.id) { session in
                LooperSnippetSessionRow(session: session)
            }

            if sessions.count > LooperSiriIntentConstants.searchResultDialogLimit {
                Text("\(sessions.count - LooperSiriIntentConstants.searchResultDialogLimit) more sessions")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .looperSnippetSurface()
    }

    private var deletedTitle: String {
        if deletedCount == 0 {
            return "No sessions"
        }

        return deletedCount == 1 ? "1 session" : "\(deletedCount) sessions"
    }
}

private struct LooperSnippetHeader: View {
    let label: String
    let title: String

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label.uppercased())
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)

            Text(title)
                .font(.headline)
                .foregroundStyle(.primary)
                .lineLimit(2)
        }
    }
}

private struct LooperSnippetSessionRow: View {
    let session: LooperSessionEntity

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "circle.hexagongrid")
                .font(.caption)
                .foregroundStyle(Color.accentColor)

            VStack(alignment: .leading, spacing: 2) {
                Text(session.title)
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                Text("\(session.assistant) - \(session.status)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
    }
}

private extension View {
    func looperSnippetSurface() -> some View {
        padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(.thinMaterial, in: RoundedRectangle(cornerRadius: LooperSiriIntentConstants.snippetCornerRadius))
    }
}

struct LooperSiriShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenLooperSessionIntent(),
            phrases: [
                "Open a session in \(.applicationName)",
                "Show my session in \(.applicationName)"
            ],
            shortTitle: "Open Session",
            systemImageName: "arrow.up.forward.app"
        )

        AppShortcut(
            intent: SummarizeLooperSessionIntent(),
            phrases: [
                "Summarize a session in \(.applicationName)",
                "What happened in my session in \(.applicationName)"
            ],
            shortTitle: "Summarize Session",
            systemImageName: "text.badge.checkmark"
        )

        AppShortcut(
            intent: SearchLooperSessionsIntent(),
            phrases: [
                "Search sessions in \(.applicationName)",
                "Find a session in \(.applicationName)"
            ],
            shortTitle: "Search Sessions",
            systemImageName: "magnifyingglass"
        )

        AppShortcut(
            intent: CreateLooperPromptIntent(),
            phrases: [
                "Create a prompt in \(.applicationName)",
                "Add a prompt in \(.applicationName)"
            ],
            shortTitle: "Create Prompt",
            systemImageName: "plus.message"
        )

        AppShortcut(
            intent: UpdateDefaultLooperSessionIntent(),
            phrases: [
                "Set the default session in \(.applicationName)",
                "Update the default session in \(.applicationName)",
                "Clear the default session in \(.applicationName)"
            ],
            shortTitle: "Update Default",
            systemImageName: "pin.slash"
        )

        AppShortcut(
            intent: AskDefaultLooperSessionIntent(),
            phrases: [
                "Ask the default session in \(.applicationName)",
                "Tell the default session in \(.applicationName)"
            ],
            shortTitle: "Ask Default",
            systemImageName: "sparkles"
        )

        AppShortcut(
            intent: AskCurrentLooperSessionIntent(),
            phrases: [
                "Ask the current session in \(.applicationName)",
                "Tell this session in \(.applicationName)"
            ],
            shortTitle: "Ask Current",
            systemImageName: "target"
        )

        AppShortcut(
            intent: AskContextualCurrentLooperSessionIntent(),
            phrases: [
                "Ask the current session in \(.applicationName) with context",
                "Tell this session in \(.applicationName) with context"
            ],
            shortTitle: "Ask Context",
            systemImageName: "text.append"
        )

        AppShortcut(
            intent: SuggestLooperPromptIntent(),
            phrases: [
                "Suggest a prompt in \(.applicationName)",
                "Suggest what to ask in \(.applicationName)"
            ],
            shortTitle: "Suggest Prompt",
            systemImageName: "quote.bubble"
        )

        AppShortcut(
            intent: DeleteLooperSessionsIntent(),
            phrases: [
                "Delete a session in \(.applicationName)",
                "Remove a session from \(.applicationName)"
            ],
            shortTitle: "Delete Session",
            systemImageName: "trash"
        )
    }

    static let shortcutTileColor: ShortcutTileColor = .blue
}

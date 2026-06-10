import AppIntents
import Foundation

struct OpenLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Open Looper Session"
    static let description = IntentDescription("Open a Looper session from the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    static var parameterSummary: some ParameterSummary {
        Summary("Open \(\.$session)")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog & OpensIntent {
        guard let url = LooperContinuationActivity.sessionDeepLinkURL(sessionID: session.id) else {
            throw LooperSiriError.invalidSessionURL
        }

        return .result(
            opensIntent: OpenURLIntent(url),
            dialog: "Opening \(session.title)"
        )
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

    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog {
        let client = LooperSiriSessionClient()
        let detail = try await client.loadSessionDetail(for: session)
        let summary = await LooperFoundationSessionSummarizer().summarize(detail)

        return .result(
            value: summary,
            dialog: "Summarized \(session.title)"
        )
    }
}

struct AskLooperSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Looper Session"
    static let description = IntentDescription("Send a prompt to a Looper session through the paired Mac.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

    @Parameter(title: "Session")
    var session: LooperSessionEntity

    @Parameter(title: "Prompt", requestValueDialog: "What should Looper send?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Ask \(\.$session) \(\.$prompt)")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = LooperSiriSessionClient()
        try await client.sendPrompt(prompt, to: session)

        return .result(dialog: "Sent to \(session.title)")
    }
}

struct AskLatestCodexSessionIntent: AppIntent {
    static let title: LocalizedStringResource = "Ask Latest Codex Session"
    static let description = IntentDescription("Send a prompt to the newest Codex Desktop session through Looper.")
    static let authenticationPolicy: IntentAuthenticationPolicy = .requiresAuthentication

    @Parameter(title: "Prompt", requestValueDialog: "What should Codex do?")
    var prompt: String

    static var parameterSummary: some ParameterSummary {
        Summary("Ask latest Codex session \(\.$prompt)")
    }

    func perform() async throws -> some IntentResult & ProvidesDialog {
        let client = LooperSiriSessionClient()
        let session = try await client.latestCodexSessionEntity()
        try await client.sendPrompt(prompt, to: session)

        return .result(dialog: "Sent to latest Codex session")
    }
}

struct LooperSiriShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenLooperSessionIntent(),
            phrases: [
                "Open a Looper session in \(.applicationName)",
                "Show my Looper session in \(.applicationName)"
            ],
            shortTitle: "Open Session",
            systemImageName: "arrow.up.forward.app"
        )

        AppShortcut(
            intent: SummarizeLooperSessionIntent(),
            phrases: [
                "Summarize a Looper session in \(.applicationName)",
                "What happened in my Looper session in \(.applicationName)"
            ],
            shortTitle: "Summarize Session",
            systemImageName: "text.badge.checkmark"
        )

        AppShortcut(
            intent: AskLooperSessionIntent(),
            phrases: [
                "Ask a Looper session in \(.applicationName)",
                "Send a prompt with \(.applicationName)"
            ],
            shortTitle: "Ask Session",
            systemImageName: "paperplane"
        )

        AppShortcut(
            intent: AskLatestCodexSessionIntent(),
            phrases: [
                "Ask latest Codex session in \(.applicationName)",
                "Tell Codex with \(.applicationName)"
            ],
            shortTitle: "Ask Codex",
            systemImageName: "sparkles"
        )
    }

    static let shortcutTileColor: ShortcutTileColor = .blue
}

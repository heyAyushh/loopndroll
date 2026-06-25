import SwiftUI

struct SessionDetailScreen: View {
    let model: CompanionAppModel
    let session: SessionSummary

    @Environment(\.dismiss) private var dismiss
    @State private var draftPrompt = ""
    @State private var contextualPromptSuggestions: [String] = []
    @State private var isSendingPrompt = false
    @State private var showingDeleteConfirmation = false
    @FocusState private var focusedInput: SessionDetailInput?

    private var detail: SessionDetail? {
        model.viewState.detail(for: session.id)
    }

    private var currentStatus: SessionStatus {
        detail?.status ?? session.status
    }

    private var currentMode: SessionMode? {
        detail?.effectiveMode ?? session.effectiveMode
    }

    private var currentMessage: String? {
        detail?.latestAssistantMessage ?? session.assistantPreview
    }

    private var currentTitle: String {
        detail?.title ?? session.title
    }

    private var firstUserPromptText: String? {
        guard let prompt = detail?.firstUserPrompt?.trimmingCharacters(in: .whitespacesAndNewlines),
              !prompt.isEmpty
        else {
            return nil
        }
        return prompt
    }

    private var currentMetadata: SessionMetadata {
        detail?.metadata ?? session.metadata
    }

    private var currentGoal: SessionGoalSummary? {
        detail?.goal ?? session.goal
    }

    private var currentLastActivityAt: String {
        detail?.lastActivityAt ?? session.lastActivityAt
    }

    private var currentLastMessageAt: String? {
        detail?.lastMessageAt ?? session.lastMessageAt
    }

    private var availableNotifications: [NotificationDestination] {
        detail?.availableNotifications ?? model.snapshot?.notifications ?? []
    }

    private var selectedCompletionCheck: CompletionCheckSummary? {
        detail?.availableCompletionChecks.first(where: { $0.id == detail?.completionCheckID })
    }

    private var isMutatingSession: Bool {
        model.viewState.isMutatingSession(session.id)
    }

    var body: some View {
        Form {
            summarySection
            assistantReplySection
            promptSection
            modeSection
            notificationsSection
            completionCheckSection
            manageSection
        }
        .navigationTitle(session.ref)
        .navigationBarTitleDisplayMode(.inline)
        .userActivity(LooperContinuationActivity.activityType, isActive: true) { activity in
            LooperContinuationActivity.configureContinuationActivity(
                activity,
                sessionID: session.id,
                assistantSurface: model.siriAssistantSurface(for: session.id),
                handoffBaseURL: URL(string: model.configuredBaseURL)
            )
        }
        .looperAppEntityIdentifier(
            LooperContinuationActivity.appEntityIdentifier(
                sessionID: session.id,
                assistantSurface: model.siriAssistantSurface(for: session.id)
            )
        )
        .scrollDismissesKeyboard(.interactively)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    setSiriDefaultSession()
                } label: {
                    Label("Use with Siri", systemImage: "pin")
                }
            }

            ToolbarItemGroup(placement: .keyboard) {
                if focusedInput == .prompt {
                    PromptSuggestionKeyboardBar(
                        suggestions: keyboardPromptSuggestions,
                        onSelect: usePromptSuggestion
                    )
                }

                Spacer()

                Button("Done") {
                    focusedInput = nil
                }
                .accessibilityIdentifier("session-detail.keyboard-done")
            }
        }
        .task {
            await model.refreshSessionDetail(id: session.id)
            await markCurrentSiriSession()
            await model.donateOpenedSiriSession(session)
        }
        .task(id: promptSuggestionContextKey) {
            await refreshPromptSuggestions()
        }
        .refreshable {
            await model.refreshSessionDetail(id: session.id)
            await markCurrentSiriSession()
        }
        .confirmationDialog(
            "Delete Session",
            isPresented: $showingDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete Session", role: .destructive) {
                guard !isMutatingSession else {
                    return
                }

                Task {
                    await model.deleteSession(session.id)
                    if model.errorMessage == nil {
                        dismiss()
                    }
                }
            }
            .accessibilityIdentifier("session-detail.delete-confirm")

            Button("Keep Session") {
                showingDeleteConfirmation = false
            }
                .accessibilityIdentifier("session-detail.delete-cancel")
        } message: {
            Text("This removes the session from the looper list on Mac and iPhone.")
        }
    }

    private var summarySection: some View {
        Section("Summary") {
            LabeledContent {
                HStack(spacing: 8) {
                    AssistantClientGlyph(
                        client: detail?.assistantClient ?? session.assistantClient,
                        isWorking: (detail?.status ?? session.status) == .active
                    )
                    Text((detail?.assistantClient ?? session.assistantClient).displayTitle)
                }
            } label: {
                Text("Assistant")
            }
            LabeledContent("Title", value: currentTitle)
            if let firstUserPromptText {
                LabeledContent("First Prompt") {
                    Text(firstUserPromptText)
                        .multilineTextAlignment(.trailing)
                        .lineLimit(4)
                }
            }
            LabeledContent("Kind", value: currentMetadata.kind.label)

            if let projectPath = currentMetadata.projectPath {
                LabeledContent("Project") {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(currentMetadata.projectName ?? projectPath)
                        Text(projectPath)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }

            LabeledContent("Started From", value: currentMetadata.sourceDisplayName)
            LabeledContent("Task Type", value: currentMetadata.taskKind.label)
            LabeledContent("Transcript") {
                Text(currentMetadata.transcriptAvailable ? "Available" : "Not Available")
            }

            if let gitRepository = currentMetadata.gitRepository {
                LabeledContent("Git Repo") {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(gitRepository.repositoryName)
                        if let branch = gitRepository.branch {
                            Text(branch)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }

            if let pullRequestURL = currentMetadata.pullRequestURL {
                LabeledContent("Pull Request", value: pullRequestURL)
            }

            if currentMetadata.supportsSubagents {
                LabeledContent("Subagents", value: "Supported")
            }

            if !currentMetadata.installedPlugins.isEmpty {
                LabeledContent(
                    "Plugins",
                    value: currentMetadata.installedPlugins.map(\.name).joined(separator: ", ")
                )
            }

            if !currentMetadata.sources.isEmpty {
                LabeledContent(
                    "Sources",
                    value: currentMetadata.sources.map(\.label).joined(separator: ", ")
                )
            }

            LabeledContent("Status", value: currentStatus.label)
            if let currentGoal {
                LabeledContent("Goal") {
                    SessionGoalStatusDetail(goal: currentGoal)
                }
            }
            if let currentLastMessageAt {
                LabeledContent("Last Message") {
                    Text(ModelFormatting.relativeTimestamp(currentLastMessageAt))
                }
            }
            LabeledContent("Last Active") {
                Text(ModelFormatting.relativeTimestamp(currentLastActivityAt))
            }
            if let lastSyncedAt = model.snapshot?.host.lastSyncedAt, !lastSyncedAt.isEmpty {
                LabeledContent("Last Synced") {
                    Text(ModelFormatting.relativeTimestamp(lastSyncedAt))
                }
            }
            LabeledContent("Mode", value: ModelFormatting.friendlyMode(currentMode))
        }
    }

    private var assistantReplySection: some View {
        Section("Latest Assistant Reply") {
            if let currentMessage, !currentMessage.isEmpty {
                MarkdownMessageView(markdown: currentMessage)
                    .padding(.vertical, 4)
            } else {
                Text("No assistant reply has been captured yet.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var promptSection: some View {
        Section {
            TextEditor(text: $draftPrompt)
                .frame(minHeight: CompanionMetrics.editorMinHeight)
                .focused($focusedInput, equals: .prompt)
                .accessibilityIdentifier("session-detail.prompt-editor")

            Button {
                sendPrompt()
            } label: {
                if isSendingPrompt || isMutatingSession {
                    ProgressView()
                } else {
                    Label("Send Prompt", systemImage: "paperplane")
                }
            }
            .disabled(!canSendPrompt)
            .accessibilityIdentifier("session-detail.send-prompt")
        } header: {
            Text("Prompt")
        } footer: {
            Text(promptFooterText)
        }
    }

    private var modeSection: some View {
        Section {
            ForEach(SessionMode.allCases, id: \.rawValue) { mode in
                Button {
                    model.beginApplyMode(mode, to: session.id)
                } label: {
                    HStack(spacing: 12) {
                        Label(mode.label, systemImage: mode.symbolName)
                            .foregroundStyle(.primary)

                        Spacer()

                        if currentMode == mode {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                        }
                    }
                }
                .disabled(isMutatingSession)
                .accessibilityIdentifier(mode.detailAccessibilityIdentifier)
            }

            Button {
                model.beginApplyMode(nil, to: session.id)
            } label: {
                HStack {
                    Label("Use Global Default", systemImage: "dial.low")
                    Spacer()
                    if currentMode == nil {
                        Image(systemName: "checkmark")
                            .foregroundStyle(.tint)
                    }
                }
            }
            .accessibilityIdentifier("session-detail.mode.global-default")
        } header: {
            Text("Mode")
        } footer: {
            Text(currentMode?.summary ?? "This session follows the global Looper default.")
        }
    }

    private var notificationsSection: some View {
        Section("Notification Routes") {
            if availableNotifications.isEmpty {
                Text("No notification routes configured on the Mac.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(availableNotifications) { notification in
                    NotificationDestinationRow(
                        destination: notification,
                        isSelected: detail?.notificationIds.contains(notification.id) == true,
                        showsSelection: true
                    )
                }
            }
        }
    }

    private var completionCheckSection: some View {
        Section("Completion Check") {
            if let selectedCompletionCheck {
                LabeledContent("Rule", value: selectedCompletionCheck.label)
                LabeledContent(
                    "Reply Requirement",
                    value: detail?.completionCheckWaitForReply == true
                        ? "Waits for reply"
                        : "Ready immediately"
                )
            } else {
                Text("No completion check attached to this session.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var manageSection: some View {
        Section {
            Button {
                Task {
                    await model.setSessionArchived(
                        !(detail?.isArchived ?? session.isArchived),
                        sessionID: session.id
                    )
                }
            } label: {
                Label(
                    (detail?.isArchived ?? session.isArchived)
                        ? "Unarchive Session"
                        : "Archive Session",
                    systemImage: (detail?.isArchived ?? session.isArchived)
                        ? "tray.and.arrow.up"
                        : "archivebox"
                )
            }
            .disabled(isMutatingSession)
            .accessibilityIdentifier("session-detail.archive-toggle")

            Button("Delete Session", role: .destructive) {
                showingDeleteConfirmation = true
            }
            .disabled(isMutatingSession)
            .accessibilityIdentifier("session-detail.delete")

            if isMutatingSession {
                ProgressView("Updating Session")
            }
        } header: {
            Text("Manage")
        } footer: {
            Text("Archive keeps the history. Delete removes the session from the Mac and this iPhone.")
        }
    }

    private var trimmedPrompt: String {
        draftPrompt.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var canSendPrompt: Bool {
        !isSendingPrompt &&
            !isMutatingSession &&
            !trimmedPrompt.isEmpty &&
            promptDeliveryIsAvailable &&
            currentMode != nil &&
            !(detail?.isArchived ?? session.isArchived)
    }

    private var promptFooterText: String {
        if !promptDeliveryIsAvailable {
            return promptDeliveryUnavailableReason ?? "This session cannot receive prompts from Looper."
        }

        return currentMode == nil
            ? "Set a mode before sending a prompt."
            : "Prompt is queued for this session mode."
    }

    private var promptDeliveryIsAvailable: Bool {
        detail?.canSendPrompt ?? session.canSendPrompt
    }

    private var promptDeliveryUnavailableReason: String? {
        detail?.promptDeliveryUnavailableReason ?? session.promptDeliveryUnavailableReason
    }

    private var fallbackPromptSuggestions: [String] {
        LooperSessionContextEngine.fallbackSuggestions(
            title: detail?.title ?? session.title,
            status: currentStatus,
            assistantName: (detail?.assistantClient ?? session.assistantClient).displayTitle,
            taskKind: currentMetadata.taskKind
        )
    }

    private var promptSuggestions: [String] {
        let suggestions = contextualPromptSuggestions.isEmpty
            ? fallbackPromptSuggestions
            : contextualPromptSuggestions
        return Array(suggestions.prefix(SessionPromptSuggestionLayout.visibleSuggestionLimit))
    }

    private var keyboardPromptSuggestions: [String] {
        focusedInput == .prompt ? promptSuggestions : []
    }

    private var promptSuggestionContextKey: String {
        [
            session.id,
            detail?.lastActivityAt ?? session.lastActivityAt,
            detail?.lastMessageAt ?? session.lastMessageAt ?? "",
            currentStatus.rawValue,
            currentMetadata.taskKind.rawValue,
            currentTitle,
        ]
            .joined(separator: "|")
    }

    private func refreshPromptSuggestions() async {
        contextualPromptSuggestions = fallbackPromptSuggestions
        guard let detail else {
            return
        }

        let suggestions = await LooperSessionContextEngine().suggestions(for: detail)
        guard !Task.isCancelled else {
            return
        }
        contextualPromptSuggestions = suggestions.isEmpty ? fallbackPromptSuggestions : suggestions
    }

    private func usePromptSuggestion(_ suggestion: String) {
        draftPrompt = suggestion
        focusedInput = .prompt
    }

    private func sendPrompt() {
        guard canSendPrompt else {
            return
        }

        let prompt = trimmedPrompt
        draftPrompt = ""
        focusedInput = nil
        let sendTask = model.beginSendSessionPrompt(prompt, to: session.id)
        isSendingPrompt = true
        Task {
            let didSend = await sendTask.value
            await MainActor.run {
                if !didSend {
                    draftPrompt = prompt
                    focusedInput = .prompt
                }
                isSendingPrompt = false
            }
        }
    }

    private func setSiriDefaultSession() {
        guard !isMutatingSession else {
            return
        }

        Task {
            await model.setSiriDefaultSession(session)
        }
    }

    private func markCurrentSiriSession() async {
        await model.markCurrentSiriSession(session)
    }

}

private enum SessionDetailInput {
    case prompt
}

private struct SessionGoalStatusDetail: View {
    let goal: SessionGoalSummary

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            Label(goal.displayStatusLabel, systemImage: goal.displayStatusSymbolName)
                .font(.callout.weight(.semibold))
                .foregroundStyle(SessionGoalStatusVisuals.tint(for: goal))
                .accessibilityIdentifier("session-detail.goal-status")

            Text(goal.title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.trailing)
                .lineLimit(2)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(goal.displayStatusLabel)
        .accessibilityValue(goal.title)
    }
}

private enum SessionPromptSuggestionLayout {
    static let visibleSuggestionLimit = 3
    static let iconSpacing: CGFloat = 6
    static let separatorHeight: CGFloat = 24
    static let minimumSuggestionWidth: CGFloat = 96
    static let maximumSuggestionWidth: CGFloat = 220
}

private struct PromptSuggestionKeyboardBar: View {
    let suggestions: [String]
    let onSelect: (String) -> Void

    var body: some View {
        HStack(spacing: SessionPromptSuggestionLayout.iconSpacing) {
            Image(systemName: "sparkles")
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)

            ForEach(Array(suggestions.enumerated()), id: \.offset) { index, suggestion in
                if index > 0 {
                    Divider()
                        .frame(height: SessionPromptSuggestionLayout.separatorHeight)
                }

                Button {
                    onSelect(suggestion)
                } label: {
                    Text(suggestion)
                        .lineLimit(1)
                        .frame(
                            minWidth: SessionPromptSuggestionLayout.minimumSuggestionWidth,
                            maxWidth: SessionPromptSuggestionLayout.maximumSuggestionWidth
                        )
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("session-detail.prompt-suggestion.\(index)")
            }
        }
        .accessibilityIdentifier("session-detail.prompt-suggestion-bar")
    }
}

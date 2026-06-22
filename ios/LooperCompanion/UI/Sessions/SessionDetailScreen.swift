import SwiftUI

struct SessionDetailScreen: View {
    let model: CompanionAppModel
    let session: SessionSummary

    @Environment(\.dismiss) private var dismiss
    @State private var draftPrompt = ""
    @State private var isSendingPrompt = false
    @State private var showingDeleteConfirmation = false
    @FocusState private var focusedInput: SessionDetailInput?

    private var detail: SessionDetail? {
        model.detail(for: session.id)
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
        model.isMutatingSession(session.id)
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
                .disabled(isMutatingSession)
            }

            ToolbarItemGroup(placement: .keyboard) {
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

            ForEach(Array(promptSuggestions.enumerated()), id: \.element) { index, suggestion in
                Button {
                    usePromptSuggestion(suggestion)
                } label: {
                    Label(suggestion, systemImage: "quote.bubble")
                        .lineLimit(2)
                }
                .accessibilityIdentifier("session-detail.prompt-suggestion.\(index)")
            }

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
                    Task {
                        await model.applyMode(mode, to: session.id)
                    }
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
            }

            Button {
                Task {
                    await model.applyMode(nil, to: session.id)
                }
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
            .disabled(isMutatingSession)
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

    private var promptSuggestions: [String] {
        LooperSessionContextEngine.fallbackSuggestions(
            title: detail?.title ?? session.title,
            status: currentStatus,
            assistantName: (detail?.assistantClient ?? session.assistantClient).displayTitle,
            taskKind: currentMetadata.taskKind
        )
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
        isSendingPrompt = true
        Task {
            let didSend = await model.sendSessionPrompt(prompt, to: session.id)
            await MainActor.run {
                if didSend {
                    draftPrompt = ""
                    focusedInput = nil
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

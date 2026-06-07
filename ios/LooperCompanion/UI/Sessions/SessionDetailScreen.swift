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

    private var currentMetadata: SessionMetadata {
        detail?.metadata ?? session.metadata
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
                handoffBaseURL: URL(string: model.configuredBaseURL)
            )
        }
        .scrollDismissesKeyboard(.interactively)
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()

                Button("Done") {
                    focusedInput = nil
                }
            }
        }
        .task {
            await model.refreshSessionDetail(id: session.id)
        }
        .refreshable {
            await model.refreshSessionDetail(id: session.id)
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
            LabeledContent("Title", value: session.title)
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
            LabeledContent("Updated") {
                Text(ModelFormatting.relativeTimestamp(detail?.lastUpdatedAt ?? session.lastUpdatedAt))
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
                    HStack(spacing: 12) {
                        Label(notification.label, systemImage: channelSymbolName(notification.channel))
                            .foregroundStyle(.primary)

                        Spacer()

                        if detail?.notificationIds.contains(notification.id) == true {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                        } else {
                            Text(notification.channel.capitalized)
                                .foregroundStyle(.secondary)
                        }
                    }
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

            Button("Delete Session", role: .destructive) {
                showingDeleteConfirmation = true
            }
            .disabled(isMutatingSession)

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
            currentMode != nil &&
            !(detail?.isArchived ?? session.isArchived)
    }

    private var promptFooterText: String {
        currentMode == nil
            ? "Set a mode before sending a prompt."
            : "Prompt is queued for this session mode."
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

    private func channelSymbolName(_ channel: String) -> String {
        switch channel.lowercased() {
        case "telegram":
            return "paperplane"
        case "slack":
            return "bubble.left.and.bubble.right"
        default:
            return "bell"
        }
    }
}

private enum SessionDetailInput {
    case prompt
}

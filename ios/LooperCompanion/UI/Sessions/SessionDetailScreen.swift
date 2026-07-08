import SwiftUI

struct SessionDetailScreen: View {
    let model: CompanionAppModel
    let route: SessionDetailRoute

    @Environment(\.dismiss) private var dismiss
    @State private var draftPrompt = ""
    @State private var draftPromptIntent = CompanionPromptIntent.steer
    @State private var draftMode: SessionMode?
    @State private var hasDraftModeSelection = false
    @State private var contextualPromptSuggestions: [String] = []
    @State private var isSendingPrompt = false
    @State private var showingDeleteConfirmation = false
    @State private var openedLifecycleSessionID: String?
    @State private var openedLifecycleTask: Task<Void, Never>?
    @State private var cachedPresentation: SessionDetailPresentation?
    @State private var lastReplyStreamActivityAt: Date?
    @State private var isPinnedToLatestReply = true
    @FocusState private var focusedInput: SessionDetailInput?

    private var sessionID: String {
        route.sessionID
    }

    private var streamingReplyBuffer: StreamingReplyBuffer {
        model.streamingReplyBuffer(forSessionID: sessionID)
    }

    var body: some View {
        let snapshotPresentation = model.viewState.snapshotDetailPresentation(for: route)
        let presentation = cachedPresentation(for: route) ?? snapshotPresentation
        let detailRefreshKey = SessionDetailPresentationRefreshKey(
            routeID: route.id,
            detailRevision: model.sessionDetailRevision,
            snapshotIdentity: presentationRefreshIdentity(snapshotPresentation)
        )
        let buffer = streamingReplyBuffer

        ScrollViewReader { scrollProxy in
            List {
                if presentation.hasResolvedSession {
                    summarySection(presentation)
                    if presentation.latestAssistantReply != nil || buffer.isStreaming {
                        assistantReplySection(presentation)
                    }
                    promptSection(presentation)
                    modeSection(presentation)
                    notificationsSection(presentation)
                    completionCheckSection(presentation)
                    detailsSection(presentation)
                    manageSection(presentation)
                } else {
                    missingSessionSection
                }
            }
            .listStyle(.insetGrouped)
            .contentMargins(.top, 0, for: .scrollContent)
            .safeAreaPadding(.bottom, CompanionMetrics.rowSpacing)
            .companionListSurface()
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentOffset.y >=
                    geometry.contentSize.height - geometry.containerSize.height -
                    SessionDetailScrollMetrics.bottomPinTolerance
            } action: { _, isAtBottom in
                isPinnedToLatestReply = isAtBottom
            }
            .onChange(of: buffer.text) {
                guard isPinnedToLatestReply, buffer.isStreaming else {
                    return
                }
                withAnimation(.easeOut(duration: SessionDetailScrollMetrics.autoscrollDuration)) {
                    scrollProxy.scrollTo(SessionDetailScrollAnchor.latestReply, anchor: .bottom)
                }
            }
        }
        .navigationTitle(presentation.ref)
        .navigationBarTitleDisplayMode(.inline)
        .userActivity(LooperContinuationActivity.activityType, isActive: true) { activity in
            LooperContinuationActivity.configureContinuationActivity(
                activity,
                sessionID: sessionID,
                assistantSurface: route.assistantSurface,
                handoffBaseURL: URL(string: model.configuredBaseURL)
            )
        }
        .looperAppEntityIdentifier(
            LooperContinuationActivity.appEntityIdentifier(
                sessionID: sessionID,
                assistantSurface: route.assistantSurface
            )
        )
        .scrollDismissesKeyboard(.interactively)
        .onAppear {
            syncDraftModeFromCurrentModeIfNeeded(presentation.effectiveMode)
        }
        .onChange(of: route.id) {
            cachedPresentation = snapshotPresentation
            lastReplyStreamActivityAt = nil
            isPinnedToLatestReply = true
            resetDraftMode(presentation.effectiveMode)
            openedLifecycleSessionID = nil
            openedLifecycleTask?.cancel()
            openedLifecycleTask = nil
        }
        .onChange(of: presentation.effectiveMode) {
            syncDraftModeFromCurrentModeIfNeeded(presentation.effectiveMode)
        }
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
                        suggestions: keyboardPromptSuggestions(presentation),
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
        .task(id: route.id) {
            runOpenedSessionLifecycleIfNeeded()
        }
        .task(id: detailRefreshKey) {
            await refreshCachedPresentation(from: snapshotPresentation)
        }
        .task(id: lastReplyStreamActivityAt) {
            await expireReplyStreamActivityAfterLinger()
        }
        .task(id: promptSuggestionContextKey(presentation)) {
            await refreshPromptSuggestions(presentation)
        }
        .refreshable {
            model.refreshSessionDetail(
                id: sessionID,
                assistantSurface: route.assistantSurface
            )
            await markCurrentSiriSessionIfNeeded()
        }
        .confirmationDialog(
            "Delete Session",
            isPresented: $showingDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete Session", role: .destructive) {
                Task {
                    await model.deleteSession(sessionID)
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
        .onDisappear {
            openedLifecycleTask?.cancel()
            openedLifecycleTask = nil
        }
    }

    private var missingSessionSection: some View {
        Section {
            ContentUnavailableView(
                "Session unavailable",
                systemImage: "exclamationmark.magnifyingglass",
                description: Text("This session is not in the local detail state for \(route.assistantSurface.displayTitle).")
            )
        }
    }

    // Summary keeps only what a user acts on at a glance; provenance
    // and capability rows live in the Details section at the bottom.
    private func summarySection(_ presentation: SessionDetailPresentation) -> some View {
        let metadata = presentation.metadata
        return Section("Summary") {
            LabeledContent {
                HStack(spacing: 8) {
                    AssistantSurfaceLogoMark(surface: presentation.assistantSurface)
                        .frame(width: 28, height: 28)
                    Text(presentation.assistantTitle)
                }
            } label: {
                Text("Assistant")
            }
            LabeledContent("Title", value: presentation.title)

            if let projectPath = metadata.projectPath {
                LabeledContent("Project") {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text(metadata.projectName ?? projectPath)
                        Text(projectPath)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }

            if let gitRepository = metadata.gitRepository {
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

            LabeledContent("Status", value: presentation.status.label)
            if let currentGoal = presentation.goal {
                LabeledContent("Goal") {
                    SessionGoalStatusDetail(goal: currentGoal)
                }
            }
            if let currentLastActivityAt = presentation.lastActivityAt {
                LabeledContent("Last Active") {
                    Text(ModelFormatting.relativeTimestamp(currentLastActivityAt))
                }
            }
            LabeledContent("Mode", value: ModelFormatting.friendlyMode(presentation.effectiveMode))
        }
    }

    private func detailsSection(_ presentation: SessionDetailPresentation) -> some View {
        let metadata = presentation.metadata
        return Section("Details") {
            if let firstUserPromptText = presentation.firstUserPromptText {
                LabeledContent("First Prompt") {
                    Text(firstUserPromptText)
                        .multilineTextAlignment(.trailing)
                        .lineLimit(4)
                }
            }
            LabeledContent("Kind", value: metadata.kind.label)
            LabeledContent("Started From", value: metadata.sourceDisplayName)
            if metadata.taskKind != .unknown {
                LabeledContent("Task Type", value: metadata.taskKind.label)
            }
            LabeledContent("Transcript") {
                Text(metadata.transcriptAvailable ? "Available" : "Not Available")
            }

            if let pullRequestURL = metadata.pullRequestURL {
                LabeledContent("Pull Request", value: pullRequestURL)
            }

            if metadata.supportsSubagents {
                LabeledContent("Subagents", value: "Supported")
            }

            if !metadata.installedPlugins.isEmpty {
                LabeledContent(
                    "Plugins",
                    value: metadata.installedPlugins.map(\.name).joined(separator: ", ")
                )
            }

            if !metadata.sources.isEmpty {
                LabeledContent(
                    "Sources",
                    value: metadata.sources.map(\.label).joined(separator: ", ")
                )
            }

            LabeledContent("Connection", value: model.viewState.deviceHubConnectionStatusLabel)
        }
    }

    private func assistantReplySection(_ presentation: SessionDetailPresentation) -> some View {
        let buffer = streamingReplyBuffer
        return Section {
            if buffer.isStreaming {
                StreamingText(buffer: buffer)
                    .padding(.vertical, 4)
                    .id(SessionDetailScrollAnchor.latestReply)
            } else if let latestAssistantReply = presentation.latestAssistantReply {
                MarkdownMessageView(markdown: latestAssistantReply)
                    .padding(.vertical, 4)
                    .contentTransition(.opacity)
                    .animation(.default, value: latestAssistantReply)
                    .id(SessionDetailScrollAnchor.latestReply)
            }
            if let currentLastMessageAt = presentation.lastMessageAt {
                Text("Last message \(ModelFormatting.relativeTimestamp(currentLastMessageAt))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("session-detail.latest-reply-timestamp")
            }
        } header: {
            assistantReplyHeader(presentation)
        }
    }

    private func assistantReplyHeader(_ presentation: SessionDetailPresentation) -> some View {
        HStack(spacing: SessionDetailStreamingIndicatorMetrics.spacing) {
            Text("Latest Assistant Reply")
            if isStreamingAssistantReply(presentation) {
                Image(systemName: SessionDetailStreamingIndicatorMetrics.systemImage)
                    .foregroundStyle(Color.accentColor)
                    .symbolEffect(.variableColor.iterative, isActive: true)
                    .accessibilityLabel("Streaming")
            }
        }
    }

    private func promptSection(_ presentation: SessionDetailPresentation) -> some View {
        Section {
            Picker("Prompt Action", selection: $draftPromptIntent) {
                ForEach(CompanionPromptIntent.allCases) { intent in
                    Text(intent.label)
                        .tag(intent)
                }
            }
            .pickerStyle(.segmented)
            .accessibilityIdentifier("session-detail.prompt-intent")

            TextEditor(text: $draftPrompt)
                .frame(minHeight: CompanionMetrics.editorMinHeight)
                .scrollDisabled(focusedInput != .prompt)
                .focused($focusedInput, equals: .prompt)
                .accessibilityIdentifier("session-detail.prompt-editor")

            Button {
                sendPrompt(presentation)
            } label: {
                if isSendingPrompt {
                    ProgressView()
                } else {
                    Label(draftPromptIntent.sendButtonTitle, systemImage: draftPromptIntent.symbolName)
                }
            }
            .disabled(!canSendPrompt(presentation))
            .accessibilityIdentifier("session-detail.send-prompt")

            if let pendingPromptDelivery = presentation.pendingPromptDelivery {
                HStack {
                    StatusPill(
                        text: pendingPromptDelivery.label,
                        tint: pendingPromptTint(for: pendingPromptDelivery)
                    )
                    .accessibilityIdentifier("session-detail.pending-prompt")
                    Spacer(minLength: 0)
                }
            }
        } header: {
            Text("Prompt")
        } footer: {
            Text(promptFooterText(presentation))
        }
    }

    private func modeSection(_ presentation: SessionDetailPresentation) -> some View {
        let selectedMode = selectedPromptMode(presentation)
        let modeSelection = Binding<SessionMode?>(
            get: { selectedMode },
            set: { selectDraftMode($0) }
        )
        return Section {
            Picker("Mode", selection: modeSelection) {
                ForEach(SessionMode.allCases, id: \.rawValue) { mode in
                    Label(mode.label, systemImage: mode.symbolName)
                        .tag(mode as SessionMode?)
                }
                Label("Use Global Default", systemImage: "dial.low")
                    .tag(nil as SessionMode?)
            }
            .pickerStyle(.wheel)
            .labelsHidden()
            .accessibilityIdentifier("session-detail.mode.wheel")
        } header: {
            Text("Mode")
        } footer: {
            Text(selectedMode?.summary ?? "This session follows the global Looper default.")
        }
    }

    private func notificationsSection(_ presentation: SessionDetailPresentation) -> some View {
        let detail = presentation.detail
        let availableNotifications = detail?.availableNotifications ?? model.viewState.availableNotifications
        return Section("Notification Routes") {
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

    private func completionCheckSection(_ presentation: SessionDetailPresentation) -> some View {
        let detail = presentation.detail
        let selectedCompletionCheck = detail?.availableCompletionChecks.first(where: { $0.id == detail?.completionCheckID })
        return Section("Completion Check") {
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

    private func manageSection(_ presentation: SessionDetailPresentation) -> some View {
        let isArchived = presentation.isArchived
        return Section {
            Button {
                Task {
                    await model.setSessionArchived(
                        !isArchived,
                        sessionID: sessionID
                    )
                }
            } label: {
                Label(
                    isArchived
                        ? "Unarchive Session"
                        : "Archive Session",
                    systemImage: isArchived
                        ? "tray.and.arrow.up"
                        : "archivebox"
                )
            }
            .accessibilityIdentifier("session-detail.archive-toggle")

            Button("Delete Session", role: .destructive) {
                showingDeleteConfirmation = true
            }
            .accessibilityIdentifier("session-detail.delete")
        } header: {
            Text("Manage")
        } footer: {
            Text("Archive keeps the history. Delete removes the session from the Mac and this iPhone.")
        }
    }

    private var trimmedPrompt: String {
        draftPrompt.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private func canSendPrompt(_ presentation: SessionDetailPresentation) -> Bool {
        !isSendingPrompt &&
            !trimmedPrompt.isEmpty &&
            presentation.canSendPrompt &&
            (draftPromptIntent == .steer || selectedPromptMode(presentation) != nil) &&
            !presentation.isArchived
    }

    private func promptFooterText(_ presentation: SessionDetailPresentation) -> String {
        guard presentation.canSendPrompt else {
            return presentation.promptDeliveryUnavailableReason ?? "This session cannot receive prompts from Looper."
        }

        let selectedMode = selectedPromptMode(presentation)
        switch draftPromptIntent {
        case .steer:
            return selectedMode == nil
                ? "Send now to the running agent."
                : "Apply this mode, then steer the running agent."
        case .queue:
            return selectedMode == nil
                ? "Choose a continuation mode before queueing."
                : "Queue this prompt for the selected continuation mode."
        }
    }

    private func pendingPromptTint(for presentation: PendingPromptDeliveryPresentation) -> Color {
        switch presentation.status {
        case .sending:
            return .accentColor
        case .notDeliveredRetry:
            return .orange
        }
    }

    private func fallbackPromptSuggestions(_ presentation: SessionDetailPresentation) -> [String] {
        LooperSessionContextEngine.fallbackSuggestions(
            title: presentation.title,
            status: presentation.status,
            assistantName: presentation.assistantTitle,
            taskKind: presentation.metadata.taskKind
        )
    }

    private func promptSuggestions(_ presentation: SessionDetailPresentation) -> [String] {
        let suggestions = contextualPromptSuggestions.isEmpty
            ? fallbackPromptSuggestions(presentation)
            : contextualPromptSuggestions
        return Array(suggestions.prefix(SessionPromptSuggestionLayout.visibleSuggestionLimit))
    }

    private func keyboardPromptSuggestions(_ presentation: SessionDetailPresentation) -> [String] {
        focusedInput == .prompt ? promptSuggestions(presentation) : []
    }

    private func selectedPromptMode(_ presentation: SessionDetailPresentation) -> SessionMode? {
        hasDraftModeSelection ? draftMode : presentation.effectiveMode
    }

    private func needsDraftModeApplyBeforePrompt(_ presentation: SessionDetailPresentation) -> Bool {
        hasDraftModeSelection && draftMode != presentation.effectiveMode
    }

    private func promptSuggestionContextKey(_ presentation: SessionDetailPresentation) -> String {
        let detail = presentation.detail
        let summary = presentation.summary
        return [
            route.id,
            detail?.lastActivityAt ?? summary?.lastActivityAt ?? "",
            detail?.lastMessageAt ?? summary?.lastMessageAt ?? "",
            presentation.status.rawValue,
            presentation.metadata.taskKind.rawValue,
            presentation.title,
        ]
            .joined(separator: "|")
    }

    private func cachedPresentation(for route: SessionDetailRoute) -> SessionDetailPresentation? {
        guard cachedPresentation?.route == route else {
            return nil
        }
        return cachedPresentation
    }

    @MainActor
    private func refreshCachedPresentation(from snapshotPresentation: SessionDetailPresentation) async {
        let previousReply = cachedPresentation(for: route)?.latestAssistantReply
        cachedPresentation = snapshotPresentation
        let refreshedPresentation = model.viewState.detailPresentation(for: route)
        guard !Task.isCancelled else {
            return
        }
        cachedPresentation = refreshedPresentation
        markReplyStreamActivityIfNeeded(
            previousReply: previousReply,
            currentReply: refreshedPresentation.latestAssistantReply
        )
    }

    /// The indicator tracks the reply text itself: it turns on only when the
    /// displayed reply actually changed (not on first load), and a linger task
    /// turns it off once chunks stop arriving. Connection liveness alone is
    /// not "streaming".
    private func markReplyStreamActivityIfNeeded(previousReply: String?, currentReply: String?) {
        guard let previousReply, let currentReply, previousReply != currentReply else {
            return
        }
        lastReplyStreamActivityAt = Date()
    }

    private func expireReplyStreamActivityAfterLinger() async {
        guard lastReplyStreamActivityAt != nil else {
            return
        }
        try? await Task.sleep(for: SessionDetailStreamingIndicatorMetrics.linger)
        guard !Task.isCancelled else {
            return
        }
        lastReplyStreamActivityAt = nil
    }

    private func presentationRefreshIdentity(_ presentation: SessionDetailPresentation) -> Int {
        var hasher = Hasher()
        hasher.combine(presentation.route)
        combineSummaryIdentity(presentation.summary, into: &hasher)
        combineDetailIdentity(presentation.detail, into: &hasher)
        return hasher.finalize()
    }

    private func combineSummaryIdentity(_ summary: SessionSummary?, into hasher: inout Hasher) {
        hasher.combine(summary?.id)
        hasher.combine(summary?.title)
        hasher.combine(summary?.status)
        hasher.combine(summary?.effectiveMode)
        hasher.combine(summary?.lastUpdatedAt)
        hasher.combine(summary?.lastActivityAt)
        hasher.combine(summary?.lastMessageAt)
        hasher.combine(summary?.assistantPreview)
        hasher.combine(summary?.goal)
    }

    private func combineDetailIdentity(_ detail: SessionDetail?, into hasher: inout Hasher) {
        hasher.combine(detail?.id)
        hasher.combine(detail?.title)
        hasher.combine(detail?.status)
        hasher.combine(detail?.effectiveMode)
        hasher.combine(detail?.lastUpdatedAt)
        hasher.combine(detail?.lastActivityAt)
        hasher.combine(detail?.lastMessageAt)
        hasher.combine(detail?.assistantPreview)
        hasher.combine(detail?.latestAssistantMessage)
        hasher.combine(detail?.goal)
    }

    private func isStreamingAssistantReply(_ presentation: SessionDetailPresentation) -> Bool {
        streamingReplyBuffer.isStreaming ||
            (model.realtimeStreamIsLive && lastReplyStreamActivityAt != nil)
    }

    private func refreshPromptSuggestions(_ presentation: SessionDetailPresentation) async {
        contextualPromptSuggestions = fallbackPromptSuggestions(presentation)
        guard let detail = presentation.detail else {
            return
        }

        let suggestions = await LooperSessionContextEngine().suggestions(for: detail)
        guard !Task.isCancelled else {
            return
        }
        contextualPromptSuggestions = suggestions.isEmpty ? fallbackPromptSuggestions(presentation) : suggestions
    }

    private func usePromptSuggestion(_ suggestion: String) {
        draftPrompt = suggestion
        focusedInput = .prompt
    }

    private func selectDraftMode(_ mode: SessionMode?) {
        draftMode = mode
        hasDraftModeSelection = true
        // Picking a mode applies it immediately; the draft only tracks the selection
        // for prompt composition. Without this, the checkmark moved but nothing was
        // ever sent unless the user also composed a prompt in the same visit.
        Task { @MainActor in
            await model.applyMode(mode, to: sessionID)
        }
    }

    private func resetDraftMode(_ currentMode: SessionMode?) {
        draftMode = currentMode
        hasDraftModeSelection = false
    }

    private func syncDraftModeFromCurrentModeIfNeeded(_ currentMode: SessionMode?) {
        guard !hasDraftModeSelection else {
            return
        }

        draftMode = currentMode
    }

    private func sendPrompt(_ presentation: SessionDetailPresentation) {
        guard canSendPrompt(presentation) else {
            return
        }

        let prompt = trimmedPrompt
        let promptIntent = draftPromptIntent
        let modeToApply = draftMode
        let shouldApplyDraftMode = needsDraftModeApplyBeforePrompt(presentation)
        let didSelectDraftMode = hasDraftModeSelection
        draftPrompt = ""
        focusedInput = nil
        let sendTask = Task { @MainActor in
            if shouldApplyDraftMode {
                guard await model.beginApplyMode(modeToApply, to: sessionID).value else {
                    return false
                }
            }

            return await model.sendSessionPrompt(prompt, intent: promptIntent, to: sessionID)
        }
        isSendingPrompt = true
        Task {
            let didSend = await sendTask.value
            await MainActor.run {
                if !didSend {
                    draftPrompt = prompt
                    focusedInput = .prompt
                } else {
                    if didSelectDraftMode {
                        hasDraftModeSelection = false
                        draftMode = modeToApply
                    }
                }
                isSendingPrompt = false
            }
        }
    }

    private func setSiriDefaultSession() {
        Task {
            await model.setSiriDefaultSession(
                sessionID,
                assistantSurface: route.assistantSurface
            )
        }
    }

    private func markCurrentSiriSessionIfNeeded() async {
        await model.markCurrentSiriSession(
            sessionID,
            assistantSurface: route.assistantSurface
        )
    }

    private func runOpenedSessionLifecycleIfNeeded() {
        guard openedLifecycleSessionID != route.id else {
            return
        }
        openedLifecycleSessionID = route.id
        model.refreshSessionDetail(
            id: sessionID,
            assistantSurface: route.assistantSurface
        )

        openedLifecycleTask?.cancel()
        let openedSessionID = sessionID
        let openedSurface = route.assistantSurface
        openedLifecycleTask = Task { @MainActor in
            await Task.yield()
            guard !Task.isCancelled else {
                return
            }
            await model.markCurrentSiriSession(
                openedSessionID,
                assistantSurface: openedSurface
            )
            guard !Task.isCancelled else {
                return
            }
            if let openedSession = model.viewState.session(
                withID: openedSessionID,
                assistantSurface: openedSurface
            ) {
                await model.donateOpenedSiriSession(openedSession)
            }
        }
    }

}

private enum SessionDetailInput {
    case prompt
}

private struct SessionDetailPresentationRefreshKey: Equatable {
    let routeID: String
    let detailRevision: Int
    let snapshotIdentity: Int
}

private struct SessionGoalStatusDetail: View {
    let goal: SessionGoalSummary

    var body: some View {
        VStack(alignment: .trailing, spacing: 4) {
            GoalStatusIcon(
                tint: SessionGoalStatusVisuals.tint(for: goal),
                size: SessionGoalStatusDetailMetrics.iconSize
            )
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

private enum SessionGoalStatusDetailMetrics {
    static let iconSize: CGFloat = 24
}

private enum SessionDetailScrollAnchor {
    static let latestReply = "session-detail.latest-reply-bottom"
}

private enum SessionDetailScrollMetrics {
    /// How close to the bottom the list must be for a new chunk to keep
    /// autoscrolling; scrolling up past this disengages the pin.
    static let bottomPinTolerance: CGFloat = 40
    static let autoscrollDuration: TimeInterval = 0.2
}

private enum SessionDetailStreamingIndicatorMetrics {
    static let spacing: CGFloat = 6
    static let systemImage = "waveform"
    /// How long the indicator stays on after the last reply change; long
    /// enough to bridge the gap between consecutive text chunks.
    static let linger: Duration = .seconds(3)
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

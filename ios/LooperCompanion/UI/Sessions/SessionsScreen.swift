import SwiftUI

private enum SessionListDisplay {
    static let defaultSectionLimit = 40
}

private enum AssistantSurfaceControlMetrics {
    static let verticalSpacing: CGFloat = 10
    static let controlTopPadding: CGFloat = 6
}

struct SessionsScreen: View {
    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator
    let openSettings: () -> Void

    @State private var isDeviceHubPresented = false
    @State private var navigationPath = NavigationPath()
    @State private var showsAllNeedsAttentionSessions = false
    @State private var showsAllRunningSessions = false
    @State private var showsAllArchivedSessions = false
    @State private var selectedAssistantSurface = CompanionAssistantSurface.defaultSurface
    @State private var pendingAssistantSurface: CompanionAssistantSurface?
    @State private var isSavingAssistantSurface = false

    private var hasVisibleSessions: Bool {
        !model.needsAttentionSessions.isEmpty ||
            !model.runningSessions.isEmpty ||
            !model.stoppedSessions.isEmpty ||
            !model.archivedSessions.isEmpty
    }

    var body: some View {
        NavigationStack(path: $navigationPath) {
            List {
                connectionSection

                if !model.needsAttentionSessions.isEmpty {
                    sessionSection(
                        title: "Needs Attention",
                        sessions: model.needsAttentionSessions,
                        isExpanded: $showsAllNeedsAttentionSessions
                    )
                }

                if !model.runningSessions.isEmpty {
                    sessionSection(
                        title: "Active",
                        sessions: model.runningSessions,
                        isExpanded: $showsAllRunningSessions
                    )
                }

                if !model.stoppedSessions.isEmpty {
                    sessionSection(
                        title: "Recent",
                        sessions: model.stoppedSessions,
                        isExpanded: .constant(false),
                        showsCount: false,
                        allowsExpansion: false,
                        footerText: recentSectionFooter
                    )
                }

                if !model.archivedSessions.isEmpty {
                    sessionSection(
                        title: "Archived",
                        sessions: model.archivedSessions,
                        isExpanded: $showsAllArchivedSessions
                    )
                }
            }
            .listStyle(.insetGrouped)
            .contentMargins(.top, 0, for: .scrollContent)
            .companionListSurface()
            .navigationTitle("Sessions")
            .navigationDestination(for: SessionSummary.self) { session in
                SessionDetailScreen(model: model, session: session)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        isDeviceHubPresented = true
                    } label: {
                        SessionsToolbarOrbButton()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Open device hub")
                }
            }
            .refreshable {
                await model.refresh()
            }
            .onChange(of: model.pendingOpenSessionID) {
                openPendingSessionIfNeeded()
            }
            .onChange(of: snapshotSessionIDs) {
                openPendingSessionIfNeeded()
            }
            .task(id: model.pendingOpenSessionID) {
                openPendingSessionIfNeeded()
            }
            .task(id: model.snapshot?.globalSettings.assistantSurface) {
                syncAssistantSurfaceFromSnapshot()
            }
            .overlay {
                overlayState
            }
        }
        .sheet(isPresented: $isDeviceHubPresented) {
            SessionsDeviceHubSheet(model: model)
                .deviceHubSheetPresentation()
        }
    }

    @ViewBuilder
    private var overlayState: some View {
        if model.isLoading && model.snapshot == nil {
            ProgressView("Loading Looper")
        } else if !hasVisibleSessions {
            ContentUnavailableView(
                unavailableStateTitle,
                systemImage: model.connectionState.symbolName,
                description: Text(emptyStateDescription)
            )
        }
    }

    private var connectionSection: some View {
        Section {
            SessionConnectionRow(
                title: model.connectivityHeadline,
                subtitle: connectionSubtitle,
                statusText: model.connectionState.label,
                statusTint: CompanionTint.tint(for: model.connectionState),
                openSettings: openSettings,
                assistantPicker: {
                    assistantPicker
                }
            )
            .companionCardRowSurface()

            if model.connectionState == .locked {
                Button {
                    Task {
                        await recoverLockedConnection()
                    }
                } label: {
                    Label(lockedConnectionActionTitle, systemImage: "faceid")
                }
                .disabled(authenticator.isAuthenticating)
                .companionCardRowSurface()
            }
        } footer: {
            if let errorMessage = model.errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .foregroundStyle(.red)
            }
        }
    }

    private var assistantPicker: some View {
        AssistantSurfacePicker(
            selection: Binding(
                get: { selectedAssistantSurface },
                set: { updateAssistantSurface($0) }
            ),
            isDisabled: model.connectionState != .connected
        )
        .padding(.top, AssistantSurfaceControlMetrics.controlTopPadding)
    }

    private var connectionSubtitle: String {
        guard let host = model.snapshot?.host else {
            return model.connectivitySummary
        }

        var subtitle = "Last synced \(ModelFormatting.relativeTimestamp(host.lastSyncedAt))"

        if selectedAssistantSurface == .grokBuild,
           let grokBuild = model.snapshot?.grokBuild
        {
            subtitle += " · Grok hooks \(grokBuild.hooksHealthTitle.lowercased())"
            subtitle += " · \(grokBuild.activeSessionCount) active / \(grokBuild.sessionCount) total"
        }

        return subtitle
    }

    private func sessionSection(
        title: String,
        sessions: [SessionSummary],
        isExpanded: Binding<Bool>,
        showsCount: Bool = true,
        allowsExpansion: Bool = true,
        footerText: String? = nil
    ) -> some View {
        let visibleSessions = visibleSessions(
            from: sessions,
            isExpanded: allowsExpansion && isExpanded.wrappedValue
        )
        return Section {
            ForEach(visibleSessions) { session in
                NavigationLink(value: session) {
                    SessionRow(session: session)
                }
                .companionCardRowSurface()
            }

            if allowsExpansion && sessions.count > SessionListDisplay.defaultSectionLimit {
                Button {
                    isExpanded.wrappedValue.toggle()
                } label: {
                    Label(
                        isExpanded.wrappedValue
                            ? "Show Recent"
                            : "Show All \(sessions.count.formatted())",
                        systemImage: isExpanded.wrappedValue
                            ? "rectangle.compress.vertical"
                            : "rectangle.expand.vertical"
                    )
                }
                .companionCardRowSurface()
            }
        } header: {
            Text(sectionTitle(title, count: sessions.count, showsCount: showsCount))
        } footer: {
            if let footerText {
                Text(footerText)
            }
        }
    }

    private func visibleSessions(
        from sessions: [SessionSummary],
        isExpanded: Bool
    ) -> ArraySlice<SessionSummary> {
        let limit = isExpanded ? sessions.count : SessionListDisplay.defaultSectionLimit
        return sessions.prefix(limit)
    }

    private var snapshotSessionIDs: [String] {
        model.snapshot?.sessions.map(\.id) ?? []
    }

    private var recentSectionFooter: String {
        guard model.stoppedSessions.count > SessionListDisplay.defaultSectionLimit else {
            return "Stopped sessions stay here until archived."
        }

        return "Showing the latest \(SessionListDisplay.defaultSectionLimit.formatted()) stopped sessions. Use Search for older sessions."
    }

    private func sectionTitle(_ title: String, count: Int, showsCount: Bool) -> String {
        guard showsCount else {
            return title
        }

        return "\(title) (\(count.formatted()))"
    }

    private var lockedConnectionActionTitle: String {
        authenticator.isFaceIDUnlockEnabled ? "Unlock with Face ID" : "Enable Face ID Unlock"
    }

    private func recoverLockedConnection() async {
        if authenticator.isFaceIDUnlockEnabled {
            await authenticator.unlock()
        } else {
            await authenticator.setFaceIDUnlockEnabled(true)
        }

        guard authenticator.isUnlocked else {
            return
        }

        await model.refresh()
    }

    private func openPendingSessionIfNeeded() {
        guard let sessionID = model.pendingOpenSessionID,
              let session = model.snapshot?.sessions.first(where: { $0.id == sessionID })
        else {
            return
        }

        _ = model.consumePendingOpenSessionID()
        navigationPath = NavigationPath()
        navigationPath.append(session)
    }

    private func updateAssistantSurface(_ surface: CompanionAssistantSurface) {
        guard selectedAssistantSurface != surface else {
            return
        }

        selectedAssistantSurface = surface
        pendingAssistantSurface = surface
        startAssistantSurfaceSaveIfNeeded()
    }

    private func syncAssistantSurfaceFromSnapshot() {
        guard !isSavingAssistantSurface, pendingAssistantSurface == nil else {
            return
        }

        selectedAssistantSurface = committedAssistantSurface
    }

    @MainActor
    private func startAssistantSurfaceSaveIfNeeded() {
        guard !isSavingAssistantSurface else {
            return
        }

        isSavingAssistantSurface = true
        Task { @MainActor in
            await savePendingAssistantSurfaces()
        }
    }

    @MainActor
    private func savePendingAssistantSurfaces() async {
        var shouldRevertToCommittedSurface = false

        while let nextAssistantSurface = pendingAssistantSurface {
            pendingAssistantSurface = nil
            let didSave = await model.saveAssistantSurface(nextAssistantSurface)
            if !didSave, pendingAssistantSurface == nil {
                shouldRevertToCommittedSurface = true
                break
            }
        }

        isSavingAssistantSurface = false
        selectedAssistantSurface = shouldRevertToCommittedSurface
            ? committedAssistantSurface
            : model.snapshot?.globalSettings.assistantSurface ?? selectedAssistantSurface
    }

    private var committedAssistantSurface: CompanionAssistantSurface {
        model.snapshot?.globalSettings.assistantSurface ?? .defaultSurface
    }

    private var emptyStateDescription: String {
        guard model.connectionState == .connected else {
            return model.connectivitySummary
        }

        switch selectedAssistantSurface {
        case .grokBuild:
            return "Start a Grok Build session on your Mac or install the Grok CLI. Looper reads sessions from ~/.grok/sessions/ and hooks at ~/.grok/hooks/looper.json."
        case .devin:
            return "Devin Desktop sessions appear here when Devin is running on your Mac."
        case .codex:
            return model.connectivitySummary
        }
    }

    private var unavailableStateTitle: String {
        switch model.connectionState {
        case .connected:
            switch selectedAssistantSurface {
            case .grokBuild:
                return "No Grok Build Sessions"
            case .devin:
                return "No Devin Sessions"
            case .codex:
                return "No Sessions"
            }
        case .connecting:
            return "Connecting to Your Mac"
        case .offline:
            return "Mac Offline"
        case .unauthorized:
            return "Connection Needs Approval"
        case .locked:
            return "Unlock Required"
        case .unpaired:
            return "Set Up Your Mac Link"
        }
    }
}

extension View {
    func deviceHubSheetPresentation() -> some View {
        presentationDetents([.fraction(0.75), .large])
            .presentationDragIndicator(.visible)
    }
}

private struct SessionConnectionRow<AssistantPicker: View>: View {
    let title: String
    let subtitle: String
    let statusText: String
    let statusTint: Color
    let openSettings: () -> Void
    @ViewBuilder let assistantPicker: () -> AssistantPicker

    var body: some View {
        VStack(alignment: .leading, spacing: AssistantSurfaceControlMetrics.verticalSpacing) {
            Button {
                openSettings()
            } label: {
                HStack(alignment: .top, spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(title)
                            .font(.headline)
                            .foregroundStyle(.primary)

                        Text(subtitle)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.leading)
                    }

                    Spacer(minLength: 12)

                    StatusPill(text: statusText, tint: statusTint)
                }
            }
            .buttonStyle(.plain)

            assistantPicker()
        }
        .padding(.vertical, 4)
    }
}

#Preview {
    let model = CompanionAppModel(environment: CompanionEnvironment(service: MockCompanionService()))
    model.snapshot = PreviewFixtures.snapshot

    return SessionsScreen(model: model, authenticator: CompanionAppAuthenticator(), openSettings: {})
}

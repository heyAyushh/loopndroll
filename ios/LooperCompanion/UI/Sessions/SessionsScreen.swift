import LooperCompanionCore
import SwiftUI

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
                routePresentation: model.connectionRoutePresentation,
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
                get: { model.selectedAssistantSurface },
                set: { updateAssistantSurface($0) }
            ),
            isDisabled: !model.canSwitchAssistantSurface
        )
        .padding(.top, AssistantSurfaceControlMetrics.controlTopPadding)
    }

    private var connectionSubtitle: String {
        if model.selectedAssistantSurface == .grokBuild, let grokBuild = model.snapshot?.grokBuild {
            return "Grok hooks \(grokBuild.hooksHealthTitle.lowercased()) · \(grokBuild.activeSessionCount) active / \(grokBuild.sessionCount) total"
        }

        return model.connectivitySummary
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
                    SessionRow(
                        session: session,
                        assistantSurface: model.selectedAssistantSurface
                    )
                }
                .companionCardRowSurface()
            }

            if allowsExpansion && sessions.count > SessionDisplayPolicy.collapsedSectionLimit {
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
        let limit = isExpanded ? sessions.count : SessionDisplayPolicy.collapsedSectionLimit
        return sessions.prefix(limit)
    }

    private var snapshotSessionIDs: [String] {
        model.snapshot?.sessionsAcrossSurfaces.map(\.id).sorted() ?? []
    }

    private var recentSectionFooter: String {
        guard model.stoppedSessions.count > SessionDisplayPolicy.collapsedSectionLimit else {
            return "Stopped sessions stay here until archived."
        }

        return "Showing the latest \(SessionDisplayPolicy.collapsedSectionLimit.formatted()) stopped sessions. Use Search for older sessions."
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
              let session = model.snapshot?.session(withID: sessionID)
        else {
            return
        }

        _ = model.consumePendingOpenSessionID()
        navigationPath = NavigationPath()
        navigationPath.append(session)
    }

    private func updateAssistantSurface(_ surface: CompanionAssistantSurface) {
        model.selectAssistantSurface(surface)
    }

    private var emptyStateDescription: String {
        guard model.connectionState == .connected else {
            return model.connectivitySummary
        }

        switch model.selectedAssistantSurface {
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
            switch model.selectedAssistantSurface {
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
    let routePresentation: CompanionConnectionRoutePresentation?
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

                    VStack(alignment: .trailing, spacing: 6) {
                        StatusPill(text: statusText, tint: statusTint)

                        if let routePresentation {
                            ConnectionRouteBadge(presentation: routePresentation)
                        }
                    }
                }
            }
            .buttonStyle(.plain)

            assistantPicker()
        }
        .padding(.vertical, 4)
    }
}

private enum ConnectionRouteBadgeMetrics {
    static let iconSize: CGFloat = 15
    static let horizontalSpacing: CGFloat = 5
    static let horizontalPadding: CGFloat = 8
    static let verticalPadding: CGFloat = 5
    static let backgroundOpacity = 0.12
    static let minimumTextScale = 0.75
}

private struct ConnectionRouteBadge: View {
    let presentation: CompanionConnectionRoutePresentation

    var body: some View {
        HStack(spacing: ConnectionRouteBadgeMetrics.horizontalSpacing) {
            if presentation.usesTailscaleLogo {
                TailscaleLogoMark(color: routeTint)
                    .frame(
                        width: ConnectionRouteBadgeMetrics.iconSize,
                        height: ConnectionRouteBadgeMetrics.iconSize
                    )
            } else {
                Image(systemName: presentation.systemImageName)
                    .font(.caption.weight(.semibold))
            }

            Text(presentation.title)
                .lineLimit(1)
                .minimumScaleFactor(ConnectionRouteBadgeMetrics.minimumTextScale)
        }
        .font(.caption.weight(.semibold))
        .foregroundStyle(routeTint)
        .padding(.horizontal, ConnectionRouteBadgeMetrics.horizontalPadding)
        .padding(.vertical, ConnectionRouteBadgeMetrics.verticalPadding)
        .background(routeTint.opacity(ConnectionRouteBadgeMetrics.backgroundOpacity), in: Capsule())
        .accessibilityLabel("\(presentation.title) route")
        .accessibilityValue(presentation.detail)
    }

    private var routeTint: Color {
        switch presentation.route {
        case .tailscale:
            return .blue
        case .lan:
            return .green
        case .remote:
            return .purple
        case .loopback, .unsupported:
            return .secondary
        }
    }
}

private enum TailscaleLogoMarkMetrics {
    static let rowCount = 3
    static let columnCount = 3
    static let dotDiameter: CGFloat = 3.4
    static let dotSpacing: CGFloat = 2.2
}

private struct TailscaleLogoMark: View {
    let color: Color

    var body: some View {
        VStack(spacing: TailscaleLogoMarkMetrics.dotSpacing) {
            ForEach(0..<TailscaleLogoMarkMetrics.rowCount, id: \.self) { _ in
                HStack(spacing: TailscaleLogoMarkMetrics.dotSpacing) {
                    ForEach(0..<TailscaleLogoMarkMetrics.columnCount, id: \.self) { _ in
                        Circle()
                            .fill(color)
                            .frame(
                                width: TailscaleLogoMarkMetrics.dotDiameter,
                                height: TailscaleLogoMarkMetrics.dotDiameter
                            )
                    }
                }
            }
        }
    }
}

#Preview {
    let model = CompanionAppModel(environment: CompanionEnvironment(service: MockCompanionService()))
    model.snapshot = PreviewFixtures.snapshot
    model.serverHealth = CompanionServerHealth(
        ok: true,
        baseURL: "http://100.95.2.4:8765",
        baseURLs: ["http://100.95.2.4:8765"],
        serverTime: Date().ISO8601Format(),
        tailscale: CompanionTailscaleStatus(
            available: true,
            running: true,
            dnsName: "ayushs-macbook-pro.tail62d9a8.ts.net",
            baseURL: "http://100.95.2.4:8765"
        )
    )
    model.reachedBaseURL = URL(string: "http://100.95.2.4:8765")

    return SessionsScreen(model: model, authenticator: CompanionAppAuthenticator(), openSettings: {})
}

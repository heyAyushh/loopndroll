import LooperCompanionCore
import SwiftUI

private enum AssistantSurfaceControlMetrics {
    static let verticalSpacing: CGFloat = 10
    static let controlTopPadding: CGFloat = 6
}

/// Mail-style swipe-between-categories: a horizontal swipe on the session list moves to the
/// adjacent assistant surface. Thresholds keep vertical scrolling and row taps unaffected.
private enum AssistantSurfaceSwipeMetrics {
    static let minimumDragDistance: CGFloat = 24
    static let minimumHorizontalTranslation: CGFloat = 60
    /// Horizontal movement must dominate vertical movement by this factor to count as a swipe.
    static let horizontalDominanceRatio: CGFloat = 1.5
}

private enum SessionConnectionRowMetrics {
    static let horizontalSpacing: CGFloat = 12
    static let statusSpacing: CGFloat = 6
    static let titleSpacing: CGFloat = 4
    static let settingsIconSize: CGFloat = 28
    static let minimumTrailingSpacing: CGFloat = 12
    static let verticalPadding: CGFloat = 4
}

private enum SessionsSkeletonMetrics {
    /// Enough rows to fill a typical phone screen without scrolling.
    static let rowCount = 6
    static let rowSpacing: CGFloat = 12
    static let horizontalPadding: CGFloat = 16
    static let topPadding: CGFloat = 8
}

struct SessionsScreen: View {
    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator
    let openSettings: () -> Void

    @State private var isDeviceHubPresented = false
    @State private var navigationPath = NavigationPath()
    @State private var showsAllNeedsAttentionSessions = false
    @State private var showsAllRunningSessions = false
    @State private var showsAllTodaySessions = false
    @State private var showsAllYesterdaySessions = false
    @State private var showsAllPreviousWeekSessions = false
    @State private var showsAllArchivedSessions = false

    private var hasVisibleSessions: Bool {
        !model.viewState.needsAttentionSessions.isEmpty ||
            !model.viewState.runningSessions.isEmpty ||
            !model.viewState.stoppedSessions.isEmpty ||
            !model.viewState.archivedSessions.isEmpty
    }

    var body: some View {
        NavigationStack(path: $navigationPath) {
            List {
                connectionSection

                if !model.viewState.needsAttentionSessions.isEmpty {
                    sessionSection(
                        title: "Needs Attention",
                        sessions: model.viewState.needsAttentionSessions,
                        isExpanded: $showsAllNeedsAttentionSessions
                    )
                }

                if !model.viewState.runningSessions.isEmpty {
                    sessionSection(
                        title: "Active",
                        sessions: model.viewState.runningSessions,
                        isExpanded: $showsAllRunningSessions
                    )
                }

                let history = SessionHistoryBuckets(
                    stoppedSessions: model.viewState.stoppedSessions
                )
                if !history.today.isEmpty {
                    sessionSection(
                        title: "Today",
                        sessions: history.today,
                        isExpanded: $showsAllTodaySessions
                    )
                }
                if !history.yesterday.isEmpty {
                    sessionSection(
                        title: "Yesterday",
                        sessions: history.yesterday,
                        isExpanded: $showsAllYesterdaySessions
                    )
                }
                if !history.previousWeek.isEmpty {
                    sessionSection(
                        title: "Previous 7 Days",
                        sessions: history.previousWeek,
                        isExpanded: $showsAllPreviousWeekSessions
                    )
                }
                if !history.earlier.isEmpty {
                    sessionSection(
                        title: "Earlier",
                        sessions: history.earlier,
                        isExpanded: .constant(false),
                        showsCount: false,
                        allowsExpansion: false,
                        footerText: earlierSectionFooter(count: history.earlier.count)
                    )
                }

                if !model.viewState.archivedSessions.isEmpty {
                    sessionSection(
                        title: "Archived",
                        sessions: model.viewState.archivedSessions,
                        isExpanded: $showsAllArchivedSessions
                    )
                }
            }
            .listStyle(.insetGrouped)
            .contentMargins(.top, 0, for: .scrollContent)
            .companionListSurface()
            .navigationTitle("Sessions")
            .navigationDestination(for: SessionDetailRoute.self) { route in
                SessionDetailScreen(model: model, route: route)
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
                    .accessibilityIdentifier("sessions.open-device-hub")
                }
            }
            .refreshable {
                model.connection.requestRefresh(.pullGesture)
            }
            .simultaneousGesture(
                DragGesture(minimumDistance: AssistantSurfaceSwipeMetrics.minimumDragDistance)
                    .onEnded(handleAssistantSurfaceSwipe)
            )
            .onChange(of: model.pendingOpenSessionID) {
                openPendingSessionIfNeeded()
            }
            .onChange(of: model.viewState.sessionIndexIdentity) {
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
        if model.isLoading && !model.viewState.hasSnapshot {
            skeletonSessionsList
        } else if !hasVisibleSessions {
            ContentUnavailableView(
                model.viewState.sessionsUnavailableTitle,
                systemImage: model.viewState.sessionsUnavailableSystemImage,
                description: Text(model.viewState.sessionsEmptyDescription)
            )
        }
    }

    /// First-launch loading state: a shimmering stand-in for the sessions
    /// list rather than a spinner, so the screen reads as "about to fill in"
    /// instead of "stuck" while the initial snapshot loads.
    private var skeletonSessionsList: some View {
        ScrollView {
            VStack(spacing: SessionsSkeletonMetrics.rowSpacing) {
                ForEach(0..<SessionsSkeletonMetrics.rowCount, id: \.self) { _ in
                    CompanionSkeletonSessionRow()
                        .companionCardRowSurface()
                }
            }
            .padding(.horizontal, SessionsSkeletonMetrics.horizontalPadding)
            .padding(.top, SessionsSkeletonMetrics.topPadding)
        }
        .scrollDisabled(true)
        .companionListSurface()
        .background(.background)
        .accessibilityLabel("Loading sessions")
    }

    private var connectionSection: some View {
        Section {
            TimelineView(.periodic(
                from: .now,
                by: CompanionConnectionFreshnessPolicy.presentationRefreshCadence
            )) { context in
                let statusPresentation = model.viewState.connectionStatusPresentation(
                    now: context.date
                )
                SessionConnectionRow(
                    title: model.viewState.connectivityHeadline,
                    subtitle: connectionSubtitle,
                    statusText: statusPresentation.label,
                    statusTint: CompanionTint.tint(for: statusPresentation.status),
                    statusIsActive: statusPresentation.isActive,
                    statusIsSyncing: Self.isSyncingStatus(statusPresentation.status),
                    routePresentation: model.viewState.connectionRoutePresentation,
                    openSettings: openSettings,
                    assistantPicker: {
                        assistantPicker
                    }
                )
            }
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

    /// Statuses where the connection is actively working towards `.live`
    /// rather than settled (up) or stalled (down) — these get the shimmer
    /// treatment so "syncing" reads as progress, not as stuck.
    private static func isSyncingStatus(_ status: CompanionConnectionPresentationStatus) -> Bool {
        status == .reconnecting || status == .connecting
    }

    private var assistantPicker: some View {
        AssistantSurfacePicker(
            selection: Binding(
                get: { model.viewState.selectedAssistantSurface },
                set: { updateAssistantSurface($0) }
            ),
            isDisabled: !model.viewState.canSwitchAssistantSurface
        )
        .padding(.top, AssistantSurfaceControlMetrics.controlTopPadding)
    }

    private var connectionSubtitle: String {
        if model.viewState.isShowingUsableLocalState {
            return model.viewState.connectivitySummary
        }

        if let assistantSurfaceSummary = model.viewState.assistantSurfaceConnectionSummary {
            return assistantSurfaceSummary
        }

        return model.viewState.connectivitySummary
    }

    private func sessionSection(
        title: String,
        sessions: [SessionSummary],
        isExpanded: Binding<Bool>,
        showsCount: Bool = true,
        allowsExpansion: Bool = true,
        footerText: String? = nil
    ) -> some View {
        let visibleItems = visibleSessionItems(
            from: sessions,
            isExpanded: allowsExpansion && isExpanded.wrappedValue
        )
        return Section {
            ForEach(visibleItems) { item in
                NavigationLink(value: item.detailRoute) {
                    SessionRow(
                        session: item.session,
                        assistantSurface: item.assistantSurface,
                        pendingPromptDelivery: item.pendingPromptDelivery
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

    private func visibleSessionItems(
        from sessions: [SessionSummary],
        isExpanded: Bool
    ) -> [SessionRowDisplayItem] {
        visibleSessions(from: sessions, isExpanded: isExpanded).map { session in
            SessionRowDisplayItem(
                session: session,
                assistantSurface: model.viewState.assistantSurface(for: session.id),
                pendingPromptDelivery: model.viewState.pendingPromptDeliveryPresentation(for: session.id)
            )
        }
    }

    private func earlierSectionFooter(count: Int) -> String {
        guard count > SessionDisplayPolicy.collapsedSectionLimit else {
            return "Older sessions stay here until archived."
        }

        return "Showing the latest \(SessionDisplayPolicy.collapsedSectionLimit.formatted()) older sessions. Use Search for the rest."
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

        model.handleUnlock()
    }

    private func openPendingSessionIfNeeded() {
        guard let sessionID = model.pendingOpenSessionID,
              let session = model.viewState.session(withID: sessionID)
        else {
            return
        }

        _ = model.consumePendingOpenSessionID()
        var path = NavigationPath()
        path.append(
            SessionDetailRoute(
                sessionID: sessionID,
                assistantSurface: model.viewState.assistantSurface(for: session.id)
            )
        )
        navigationPath = path
    }

    private func handleAssistantSurfaceSwipe(_ value: DragGesture.Value) {
        let horizontal = value.translation.width
        let vertical = value.translation.height
        guard abs(horizontal) >= AssistantSurfaceSwipeMetrics.minimumHorizontalTranslation,
              abs(horizontal) >= abs(vertical) * AssistantSurfaceSwipeMetrics.horizontalDominanceRatio,
              model.viewState.canSwitchAssistantSurface
        else {
            return
        }

        let surfaces = CompanionAssistantSurface.allCases
        guard let currentIndex = surfaces.firstIndex(of: model.viewState.selectedAssistantSurface) else {
            return
        }

        // Swiping the content left reveals the next surface on the right, and vice versa.
        let targetIndex = horizontal < 0 ? currentIndex + 1 : currentIndex - 1
        guard surfaces.indices.contains(targetIndex) else {
            return
        }

        Haptics.selectionChanged()
        updateAssistantSurface(surfaces[targetIndex])
    }

    private func updateAssistantSurface(_ surface: CompanionAssistantSurface) {
        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        _ = withTransaction(transaction) {
            model.selectAssistantSurface(surface)
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
    let statusIsActive: Bool
    let statusIsSyncing: Bool
    let routePresentation: CompanionConnectionRoutePresentation?
    let openSettings: () -> Void
    @ViewBuilder let assistantPicker: () -> AssistantPicker

    var body: some View {
        VStack(alignment: .leading, spacing: AssistantSurfaceControlMetrics.verticalSpacing) {
            Button {
                openSettings()
            } label: {
                HStack(alignment: .top, spacing: SessionConnectionRowMetrics.horizontalSpacing) {
                    Image(systemName: "antenna.radiowaves.left.and.right")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(statusTint)
                        .frame(
                            width: SessionConnectionRowMetrics.settingsIconSize,
                            height: SessionConnectionRowMetrics.settingsIconSize
                        )

                    VStack(alignment: .leading, spacing: SessionConnectionRowMetrics.titleSpacing) {
                        Text(title)
                            .font(.headline)
                            .foregroundStyle(.primary)

                        Text(subtitle)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.leading)
                    }

                    Spacer(minLength: SessionConnectionRowMetrics.minimumTrailingSpacing)

                    VStack(alignment: .trailing, spacing: SessionConnectionRowMetrics.statusSpacing) {
                        StatusPill(
                            text: statusText,
                            tint: statusTint,
                            systemImage: nil,
                            isActive: statusIsActive,
                            isShimmering: statusIsSyncing
                        )

                        if let routePresentation {
                            ConnectionRouteBadge(presentation: routePresentation)
                        }
                    }
                }
            }
            .buttonStyle(.plain)
            .accessibilityHint("Opens connection settings")

            assistantPicker()
        }
        .padding(.vertical, SessionConnectionRowMetrics.verticalPadding)
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
            ConnectionRouteIcon(
                presentation: presentation,
                size: ConnectionRouteBadgeMetrics.iconSize
            )

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
        ConnectionRouteVisuals.tint(for: presentation.route)
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

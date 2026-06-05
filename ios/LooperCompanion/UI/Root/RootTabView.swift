import SwiftUI

private enum RootLaunchArgument {
    static let openOrbScannerOnLaunch = "--open-orb-scanner-on-launch"
    static let openSearchTabOnLaunch = "--open-search-tab"
    static let openSearchOnLaunch = "--open-search"
    static let searchQueryPrefix = "--search-query="
}

private enum RootTab: Hashable {
    case sessions
    case settings
    /// Trailing tab; `TabRole.search` lets the system render search as the bottom-right search affordance.
    case search
}

private enum AppUnlockLayout {
    static let symbolSize: CGFloat = 56
    static let stackSpacing: CGFloat = 18
    static let buttonTopPadding: CGFloat = 10
    static let contentHorizontalPadding: CGFloat = 32
}

struct RootTabView: View {
    @Environment(\.scenePhase) private var scenePhase

    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator

    @AppStorage(OnboardingState.completionStorageKey) private var hasCompletedOnboarding = false
    @AppStorage("pinballGameEnabled") private var isPinballGameEnabled = false
    @AppStorage("pinballDebugOverlayEnabled") private var isPinballDebugOverlayEnabled = false
    @State private var hasCheckedLaunchOrbScanner = false
    @State private var isLaunchOrbScannerPresented = false
    @State private var selectedTab: RootTab = .sessions
    @State private var pinballSurfaces: [PinballSurface] = []
    @State private var searchText = ""
    @State private var searchScope: SessionSearchScope = .all
    @StateObject private var spotlightSearchService = SpotlightSearchService()

    var body: some View {
        ZStack {
            if shouldShowOnboarding {
                OnboardingScreen(
                    model: model,
                    authenticator: authenticator,
                    onComplete: {
                        hasCompletedOnboarding = true
                    }
                )
            } else {
                mainTabs

                if isPinballGameEnabled {
                    PinballGameView(
                        isEnabled: isPinballGameEnabled,
                        showsDebugOverlay: isPinballDebugOverlayEnabled,
                        surfaces: pinballSurfaces
                    )
                    .ignoresSafeArea()
                }
            }

            if !shouldShowOnboarding, !authenticator.isUnlocked {
                AppUnlockScreen(authenticator: authenticator)
                    .transition(.opacity)
            }
        }
        .task(id: refreshTaskID) {
            await refreshForActiveSceneIfNeeded()
        }
        .onChange(of: scenePhase) { _, nextPhase in
            guard nextPhase != .active else {
                return
            }

            authenticator.lockIfNeeded()
        }
        .onChange(of: selectedTab) { _, newTab in
            if newTab == .search {
                Task {
                    await spotlightSearchService.prepareForSearch()
                }
            } else {
                searchText = ""
            }

            pinballSurfaces = []
        }
        .onChange(of: isPinballGameEnabled) { _, isEnabled in
            if !isEnabled {
                pinballSurfaces = []
            }
        }
        .onChange(of: model.pendingOpenSessionID) { _, sessionID in
            if sessionID != nil {
                selectedTab = .sessions
            }
        }
        .onChange(of: model.connectionState) { _, connectionState in
            if connectionState == .locked, !authenticator.isUnlocked {
                authenticator.lockIfNeeded()
            }
        }
        .onAppear {
            guard !hasCheckedLaunchOrbScanner else {
                return
            }

            hasCheckedLaunchOrbScanner = true
            #if DEBUG
            isLaunchOrbScannerPresented = ProcessInfo.processInfo.arguments.contains(
                RootLaunchArgument.openOrbScannerOnLaunch
            )
            #else
            isLaunchOrbScannerPresented = false
            #endif

            if shouldOpenSearchOnLaunch {
                selectedTab = .search
            }

            applyLaunchSearchQueryIfNeeded()
        }
        .fullScreenCover(isPresented: $isLaunchOrbScannerPresented) {
            OrbScannerScreen()
        }
    }

    private var mainTabs: some View {
        TabView(selection: $selectedTab) {
            Tab("Sessions", systemImage: "message.badge.waveform", value: RootTab.sessions) {
                SessionsScreen(
                    model: model,
                    authenticator: authenticator,
                    openSettings: {
                        select(.settings)
                    }
                )
                .pinballSurfaceCollectionEnabled(shouldCollectPinballSurfaces(for: .sessions))
            }

            if selectedTab != .search {
                Tab("Settings", systemImage: "gearshape", value: RootTab.settings) {
                    SettingsScreen(model: model, authenticator: authenticator)
                        .pinballSurfaceCollectionEnabled(shouldCollectPinballSurfaces(for: .settings))
                }
            }

            Tab("Search", systemImage: "magnifyingglass", value: RootTab.search, role: .search) {
                SessionSearchScreen(
                    model: model,
                    authenticator: authenticator,
                    searchText: $searchText,
                    selectedScope: $searchScope,
                    searchService: spotlightSearchService
                )
                .pinballSurfaceCollectionEnabled(shouldCollectPinballSurfaces(for: .search))
            }
        }
        .modifier(TabBarMinimizeWhenAvailable())
        .modifier(TabViewSearchActivationWhenAvailable())
        .collectPinballSurfaces($pinballSurfaces)
    }

    private func select(_ tab: RootTab) {
        guard selectedTab != tab else {
            return
        }
        selectedTab = tab
    }

    private var shouldShowOnboarding: Bool {
        !hasCompletedOnboarding ||
            !CompanionConfiguration.hasAuthenticatedConnection() ||
            model.connectionState == .unpaired ||
            model.connectionState == .unauthorized
    }

    private func shouldCollectPinballSurfaces(for tab: RootTab) -> Bool {
        isPinballGameEnabled && selectedTab == tab
    }

    private var shouldOpenSearchOnLaunch: Bool {
        ProcessInfo.processInfo.arguments.contains(RootLaunchArgument.openSearchOnLaunch) ||
            ProcessInfo.processInfo.arguments.contains(RootLaunchArgument.openSearchTabOnLaunch)
    }

    private func applyLaunchSearchQueryIfNeeded() {
        guard searchText.isEmpty else {
            return
        }

        guard
            let argument = ProcessInfo.processInfo.arguments.first(where: { argument in
                argument.hasPrefix(RootLaunchArgument.searchQueryPrefix)
            })
        else {
            return
        }

        searchText = String(argument.dropFirst(RootLaunchArgument.searchQueryPrefix.count))
    }

    private var refreshTaskID: String {
        "\(authenticator.isUnlocked)"
    }

    private func refreshForActiveSceneIfNeeded() async {
        guard authenticator.isUnlocked else {
            CompanionDiagnostics.lifecycle.info("Root refresh skipped because app is locked")
            CompanionDiagnostics.record("root:refresh-skip locked")
            return
        }

        CompanionDiagnostics.lifecycle.info(
            "Root refresh starting scenePhase=\(String(describing: scenePhase), privacy: .public) onboarding=\(shouldShowOnboarding, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "root:refresh-start scenePhase=\(String(describing: scenePhase)) onboarding=\(shouldShowOnboarding)"
        )
        await model.prepareForActiveState()
        await model.sendLaunchVerificationAlertIfRequested()

        while !Task.isCancelled {
            try? await Task.sleep(for: CompanionMetrics.autoRefreshInterval)

            guard !Task.isCancelled else {
                return
            }

            guard authenticator.isUnlocked else {
                CompanionDiagnostics.lifecycle.info("Root refresh loop stopped because app locked")
                return
            }

            guard scenePhase == .active else {
                CompanionDiagnostics.lifecycle.info(
                    "Root refresh loop waiting for active scenePhase=\(String(describing: scenePhase), privacy: .public)"
                )
                continue
            }

            await model.refresh()
        }
    }
}

private struct AppUnlockScreen: View {
    let authenticator: CompanionAppAuthenticator

    var body: some View {
        VStack(spacing: AppUnlockLayout.stackSpacing) {
            Image(systemName: "faceid")
                .font(.system(size: AppUnlockLayout.symbolSize, weight: .regular))
                .symbolRenderingMode(.hierarchical)
                .foregroundStyle(.primary)

            VStack(spacing: AppUnlockLayout.buttonTopPadding) {
                Text("Unlock looper")
                    .font(.title2.bold())

                Text("Face ID is required before sessions are shown.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }

            Button {
                Task {
                    await authenticator.unlock()
                }
            } label: {
                Label("Unlock with Face ID", systemImage: "faceid")
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(authenticator.isAuthenticating)
            .padding(.top, AppUnlockLayout.buttonTopPadding)

            Button(role: .destructive) {
                Task {
                    await authenticator.setFaceIDUnlockEnabled(false)
                }
            } label: {
                Label("Turn Off Face ID Unlock", systemImage: "lock.open")
            }
            .buttonStyle(.bordered)
            .disabled(authenticator.isAuthenticating)

            if authenticator.isAuthenticating {
                ProgressView()
            }

            if let errorMessage = authenticator.errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .multilineTextAlignment(.center)
            }
        }
        .padding(.horizontal, AppUnlockLayout.contentHorizontalPadding)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(.regularMaterial)
    }
}

private struct TabBarMinimizeWhenAvailable: ViewModifier {
    func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content.tabBarMinimizeBehavior(.onScrollDown)
        } else {
            content
        }
    }
}

private struct TabViewSearchActivationWhenAvailable: ViewModifier {
    func body(content: Content) -> some View {
        if #available(iOS 26.0, *) {
            content.tabViewSearchActivation(.searchTabSelection)
        } else {
            content
        }
    }
}

#Preview {
    RootTabView(
        model: CompanionAppModel(
            environment: CompanionEnvironment(service: MockCompanionService())
        ),
        authenticator: CompanionAppAuthenticator()
    )
}

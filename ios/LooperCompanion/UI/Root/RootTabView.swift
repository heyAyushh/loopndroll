import SwiftUI
#if DEBUG
import LooperCompanionCore
#endif

private enum RootLaunchArgument {
    static let openOrbScannerOnLaunch = "--open-orb-scanner-on-launch"
    static let openSearchTabOnLaunch = "--open-search-tab"
    static let openSearchOnLaunch = "--open-search"
    static let searchQueryPrefix = "--search-query="
    /// DEBUG-only repro seam for the route-switch main-thread-hang
    /// investigation: cycles the connection route preference a few seconds
    /// after launch so the blocking chain can be reproduced headlessly and
    /// its diagnostics spans pulled from the simulator's Caches directory.
    static let cycleRouteOnLaunch = "--cycle-route-on-launch"
}

#if DEBUG
private enum RouteCycleDebugTiming {
    static let initialDelay: Duration = .seconds(3)
    static let stepDelay: Duration = .seconds(3)
    static let sequence: [CompanionConnectionRoutePreference] = [.tailscale, .lan, .remote]
}
#endif

enum LooperKeyboardShortcut {
    static let searchKey: KeyEquivalent = "l"
    static let searchModifiers: EventModifiers = .command
}

@MainActor
final class LooperRootCommandCenter: ObservableObject {
    @Published private(set) var searchRequestID = 0

    func requestSearch() {
        searchRequestID += 1
    }
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
    @ObservedObject var commandCenter: LooperRootCommandCenter

    @AppStorage(OnboardingState.completionStorageKey) private var hasCompletedOnboarding = false
    @AppStorage(PinballSettingsKeys.isGameEnabled) private var isPinballGameEnabled = false
    @AppStorage(PinballSettingsKeys.isDebugOverlayEnabled) private var isPinballDebugOverlayEnabled = false
    @State private var hasCheckedLaunchOrbScanner = false
    @State private var isLaunchOrbScannerPresented = false
    #if DEBUG
    @State private var hasStartedRouteCycleOnLaunch = false
    #endif
    @State private var selectedTab: RootTab = .sessions
    @State private var pinballSurfaces: [PinballSurface] = []
    @State private var searchText = ""
    @State private var searchScope: SessionSearchScope = .all
    @State private var searchFocusRequestID = 0
    @State private var settingsTarget: SettingsSearchTarget?
    @State private var settingsTargetRevision = 0
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
        .onChange(of: model.pendingSettingsTarget) { _, target in
            openPendingSettingsTarget(target)
        }
        .onChange(of: model.connectionState) { _, connectionState in
            if connectionState == .locked, !authenticator.isUnlocked {
                authenticator.lockIfNeeded()
            }
        }
        .onChange(of: commandCenter.searchRequestID) { _, _ in
            openSearchFromKeyboard()
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
                openSearchFromKeyboard()
            }

            applyLaunchSearchQueryIfNeeded()

            #if DEBUG
            startRouteCycleOnLaunchIfRequested()
            #endif
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

            Tab("Settings", systemImage: "gearshape", value: RootTab.settings) {
                SettingsScreen(
                    model: model,
                    authenticator: authenticator,
                    initialSearchTarget: settingsTarget,
                    initialSearchRequestID: settingsTargetRevision
                )
                    .pinballSurfaceCollectionEnabled(shouldCollectPinballSurfaces(for: .settings))
            }

            Tab("Search", systemImage: "magnifyingglass", value: RootTab.search, role: .search) {
                SessionSearchScreen(
                    model: model,
                    authenticator: authenticator,
                    searchText: $searchText,
                    selectedScope: $searchScope,
                    searchFocusRequestID: searchFocusRequestID,
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

    private func openSearchFromKeyboard() {
        guard !shouldShowOnboarding, authenticator.isUnlocked else {
            return
        }

        selectedTab = .search
        searchFocusRequestID += 1
    }

    private func openPendingSettingsTarget(_ target: SettingsSearchTarget?) {
        guard let target else {
            return
        }

        settingsTarget = target
        settingsTargetRevision += 1
        selectedTab = .settings
        _ = model.consumePendingSettingsTarget()
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

    #if DEBUG
    /// Headless repro seam for the route-switch main-thread-hang
    /// investigation: a few seconds after launch, cycles the connection
    /// route preference the same way the Settings picker does
    /// (`model.setConnectionRoutePreference`), so `sample`/diagnostics can
    /// be captured without touching the picker by hand.
    private func startRouteCycleOnLaunchIfRequested() {
        guard !hasStartedRouteCycleOnLaunch else {
            return
        }
        guard ProcessInfo.processInfo.arguments.contains(RootLaunchArgument.cycleRouteOnLaunch) else {
            return
        }

        hasStartedRouteCycleOnLaunch = true
        Task {
            try? await Task.sleep(for: RouteCycleDebugTiming.initialDelay)
            for preference in RouteCycleDebugTiming.sequence {
                CompanionDiagnostics.record(
                    "route-switch:debug-cycle-begin preference=\(preference.rawValue)"
                )
                await model.setConnectionRoutePreference(preference)
                try? await Task.sleep(for: RouteCycleDebugTiming.stepDelay)
            }
            CompanionDiagnostics.record("route-switch:debug-cycle-complete")
        }
    }
    #endif

    private var refreshTaskID: String {
        "\(authenticator.isUnlocked)-\(scenePhase == .active)"
    }

    private func refreshForActiveSceneIfNeeded() async {
        guard authenticator.isUnlocked else {
            CompanionDiagnostics.lifecycle.info("Root refresh skipped because app is locked")
            CompanionDiagnostics.record("root:refresh-skip locked")
            return
        }

        guard scenePhase == .active else {
            CompanionDiagnostics.lifecycle.info(
                "Root refresh skipped because scenePhase=\(String(describing: scenePhase), privacy: .public)"
            )
            CompanionDiagnostics.record(
                "root:refresh-skip scenePhase=\(String(describing: scenePhase))"
            )
            return
        }

        CompanionDiagnostics.lifecycle.info(
            "Root active-state prepare starting scenePhase=\(String(describing: scenePhase), privacy: .public) onboarding=\(shouldShowOnboarding, privacy: .public)"
        )
        CompanionDiagnostics.record(
            "root:active-prepare-start scenePhase=\(String(describing: scenePhase)) onboarding=\(shouldShowOnboarding)"
        )
        await model.prepareForActiveState()
        model.connection.requestRefresh(.foreground)
        await model.sendLaunchVerificationAlertIfRequested()

        // The periodic freshness tick lives in the connection runtime now;
        // this task only switches it on while unlocked-and-active.
        // `refreshTaskID` includes `scenePhase == .active`, so cancellation
        // (scene change or lock) switches it back off.
        model.connection.setAppActive(true)
        await withTaskCancellationHandler {
            // Park until cancelled; the watchdog does the ticking.
            while !Task.isCancelled {
                try? await Task.sleep(for: CompanionMetrics.autoRefreshInterval)
            }
        } onCancel: {
            Task { @MainActor in
                model.connection.setAppActive(false)
            }
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
        authenticator: CompanionAppAuthenticator(),
        commandCenter: LooperRootCommandCenter()
    )
}

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

struct RootTabView: View {
    @Environment(\.scenePhase) private var scenePhase

    let model: CompanionAppModel

    @AppStorage("pinballGameEnabled") private var isPinballGameEnabled = true
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
            TabView(selection: $selectedTab) {
                Tab("Sessions", systemImage: "message.badge.waveform", value: RootTab.sessions) {
                    SessionsScreen(
                        model: model,
                        openSettings: {
                            select(.settings)
                        }
                    )
                    .pinballSurfaceCollectionEnabled(selectedTab == .sessions)
                }

                if selectedTab != .search {
                    Tab("Settings", systemImage: "gearshape", value: RootTab.settings) {
                        SettingsScreen(model: model)
                            .pinballSurfaceCollectionEnabled(selectedTab == .settings)
                    }
                }

                Tab("Search", systemImage: "magnifyingglass", value: RootTab.search, role: .search) {
                    SessionSearchScreen(
                        model: model,
                        searchText: $searchText,
                        selectedScope: $searchScope,
                        searchService: spotlightSearchService
                    )
                    .pinballSurfaceCollectionEnabled(selectedTab == .search)
                }
            }
            .modifier(TabBarMinimizeWhenAvailable())
            .modifier(TabViewSearchActivationWhenAvailable())
            .collectPinballSurfaces($pinballSurfaces)

            PinballGameView(
                isEnabled: isPinballGameEnabled,
                showsDebugOverlay: isPinballDebugOverlayEnabled,
                surfaces: pinballSurfaces
            )
            .ignoresSafeArea()
        }
        .task(id: scenePhase) {
            await refreshForActiveSceneIfNeeded()
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

    private func select(_ tab: RootTab) {
        guard selectedTab != tab else {
            return
        }
        selectedTab = tab
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

    private func refreshForActiveSceneIfNeeded() async {
        guard scenePhase == .active else {
            return
        }

        await model.prepareForActiveState()
        await model.sendLaunchVerificationAlertIfRequested()

        while !Task.isCancelled {
            try? await Task.sleep(for: CompanionMetrics.autoRefreshInterval)

            guard !Task.isCancelled else {
                return
            }

            await model.refresh()
        }
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
        )
    )
}

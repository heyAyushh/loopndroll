import SwiftUI

private enum RootLaunchArgument {
    static let openOrbScannerOnLaunch = "--open-orb-scanner-on-launch"
    static let openSearchTabOnLaunch = "--open-search-tab"
    /// Xcode / CLI launch argument to pre-fill the TabView-attached search field.
    static let searchQueryPrefix = "--search-query="
}

private enum RootTab: Hashable {
    case sessions
    case settings
    /// Trailing tab; use `TabRole.search` so the tab bar follows system search-tab layout (HIG tab bars).
    case search
}

struct RootTabView: View {
    @Environment(\.scenePhase) private var scenePhase

    let model: CompanionAppModel

    @State private var hasCheckedLaunchOrbScanner = false
    @State private var isLaunchOrbScannerPresented = false
    @State private var selectedTab: RootTab = .sessions
    @State private var globalSearchText = ""
    @State private var globalSearchScope: SessionSearchScope = .all

    var body: some View {
        TabView(selection: $selectedTab) {
            Tab("Sessions", systemImage: "message.badge.waveform", value: RootTab.sessions) {
                SessionsScreen(
                    model: model,
                    openSettings: {
                        select(.settings)
                    }
                )
            }

            Tab("Settings", systemImage: "gearshape", value: RootTab.settings) {
                SettingsScreen(model: model)
            }

            Tab("Search", systemImage: "magnifyingglass", value: RootTab.search, role: .search) {
                SessionSearchScreen(
                    model: model,
                    searchText: $globalSearchText,
                    selectedScope: $globalSearchScope
                )
            }
        }
        .companionTabViewSearchChrome(
            model: model,
            searchText: $globalSearchText,
            selectedScope: $globalSearchScope
        )
        .modifier(TabViewSearchActivationWhenAvailable())
        .task(id: scenePhase) {
            await refreshForActiveSceneIfNeeded()
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

            if ProcessInfo.processInfo.arguments.contains(RootLaunchArgument.openSearchTabOnLaunch) {
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

    private func applyLaunchSearchQueryIfNeeded() {
        guard globalSearchText.isEmpty else {
            return
        }

        guard
            let argument = ProcessInfo.processInfo.arguments.first(where: { argument in
                argument.hasPrefix(RootLaunchArgument.searchQueryPrefix)
            })
        else {
            return
        }

        globalSearchText = String(argument.dropFirst(RootLaunchArgument.searchQueryPrefix.count))
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

/// Links search-tab selection to search activation per Human Interface Guidelines / TabView search behavior.
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

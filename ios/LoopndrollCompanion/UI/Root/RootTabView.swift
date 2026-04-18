import SwiftUI

private enum RootLaunchArgument {
    static let openOrbScannerOnLaunch = "--open-orb-scanner-on-launch"
    static let openSearchTabOnLaunch = "--open-search-tab"
}

private enum RootTab: Hashable {
    case sessions
    case search
    case settings
}

struct RootTabView: View {
    @Environment(\.scenePhase) private var scenePhase

    let model: CompanionAppModel

    @State private var hasCheckedLaunchOrbScanner = false
    @State private var isLaunchOrbScannerPresented = false
    @State private var selectedTab: RootTab = .sessions

    var body: some View {
        TabView(selection: $selectedTab) {
            SessionsScreen(
                model: model,
                openSettings: {
                    select(.settings)
                }
            )
            .tabItem {
                Label("Sessions", systemImage: "message.badge.waveform")
            }
            .tag(RootTab.sessions)

            SessionSearchScreen(model: model)
                .tabItem {
                    Label("Search", systemImage: "magnifyingglass")
                }
                .tag(RootTab.search)

            SettingsScreen(model: model)
                .tabItem {
                    Label("Settings", systemImage: "gearshape")
                }
                .tag(RootTab.settings)
        }
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

#Preview {
    RootTabView(
        model: CompanionAppModel(
            environment: CompanionEnvironment(service: MockCompanionService())
        )
    )
}

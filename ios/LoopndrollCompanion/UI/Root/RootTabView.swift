import SwiftUI

struct RootTabView: View {
    let model: CompanionAppModel

    var body: some View {
        TabView {
            SessionsScreen(model: model)
                .tabItem {
                    Label("Sessions", systemImage: "message.badge.waveform")
                }

            SettingsScreen(model: model)
                .tabItem {
                    Label("Settings", systemImage: "gearshape")
                }
        }
        .task {
            if model.snapshot == nil {
                await model.loadSnapshot()
            }
        }
    }
}

#Preview {
    RootTabView(model: CompanionAppModel(environment: CompanionEnvironment(service: MockCompanionService())))
}

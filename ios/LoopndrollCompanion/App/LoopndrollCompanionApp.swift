import SwiftUI

@main
struct LoopndrollCompanionApp: App {
    @State private var model: CompanionAppModel

    init() {
        _model = State(initialValue: CompanionAppModel(environment: .live()))
    }

    var body: some Scene {
        WindowGroup {
            RootTabView(model: model)
        }
    }
}

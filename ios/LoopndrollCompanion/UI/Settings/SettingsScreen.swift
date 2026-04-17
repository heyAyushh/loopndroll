import SwiftUI

struct SettingsScreen: View {
    let model: CompanionAppModel

    @AppStorage("stopQuickActions") private var storedQuickActions = "open-session,continue"
    @State private var draftPrompt = ""

    private var selectedQuickActions: Set<String> {
        Set(storedQuickActions.split(separator: ",").map(String.init))
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Continue Prompt") {
                    TextEditor(text: $draftPrompt)
                        .frame(minHeight: 120)
                    Button("Save Prompt") {
                        Task {
                            await model.saveDefaultPrompt(draftPrompt)
                            Haptics.success()
                        }
                    }
                }

                Section("Push Quick Actions") {
                    ForEach(QuickActionOption.allCases) { action in
                        Toggle(
                            action.label,
                            isOn: Binding(
                                get: { selectedQuickActions.contains(action.rawValue) },
                                set: { isEnabled in
                                    var next = selectedQuickActions
                                    if isEnabled {
                                        next.insert(action.rawValue)
                                    } else {
                                        next.remove(action.rawValue)
                                    }
                                    storedQuickActions = next.sorted().joined(separator: ",")
                                    Haptics.impact()
                                }
                            )
                        )
                    }
                }

                if let host = model.snapshot?.host {
                    Section("Paired Mac") {
                        LabeledContent("Host", value: host.name)
                        LabeledContent("Address", value: host.address)
                        LabeledContent("Last Sync", value: ModelFormatting.relativeTimestamp(host.lastSyncedAt))
                    }
                }

                if let notifications = model.snapshot?.notifications, !notifications.isEmpty {
                    Section("Notifications") {
                        ForEach(notifications) { notification in
                            LabeledContent(notification.label, value: notification.channel.capitalized)
                        }
                    }
                }

                if let completionChecks = model.snapshot?.completionChecks, !completionChecks.isEmpty {
                    Section("Completion Checks") {
                        ForEach(completionChecks) { completionCheck in
                            LabeledContent(completionCheck.label, value: "\(completionCheck.commandCount)")
                        }
                    }
                }
            }
            .navigationTitle("Settings")
            .task(id: model.snapshot?.globalSettings.defaultPrompt) {
                draftPrompt = model.snapshot?.globalSettings.defaultPrompt ?? draftPrompt
            }
        }
    }
}

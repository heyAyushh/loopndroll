import SwiftUI

struct SettingsScreen: View {
    let model: CompanionAppModel
    var initialSearchTarget: SettingsSearchTarget? = nil
    var embedsInNavigationStack = true

    @State private var connectionCodeErrorMessage: String?
    @State private var draftConnectionCode = ""
    @AppStorage("stopQuickActions") private var storedQuickActions = "open-session,continue"
    @AppStorage("pinballGameEnabled") private var isPinballGameEnabled = true
    @AppStorage("pinballDebugOverlayEnabled") private var isPinballDebugOverlayEnabled = false
    @State private var draftPrompt = ""
    @State private var scrollTarget: SettingsSearchTarget?

    private var selectedQuickActions: Set<String> {
        Set(storedQuickActions.split(separator: ",").map(String.init))
    }

    private var release: CompanionRelease {
        CompanionConfiguration.currentRelease()
    }

    private var isPromptDirty: Bool {
        draftPrompt != (model.snapshot?.globalSettings.defaultPrompt ?? "")
    }

    var body: some View {
        Group {
            if embedsInNavigationStack {
                NavigationStack {
                    settingsContent
                }
            } else {
                settingsContent
            }
        }
    }

    private var settingsContent: some View {
        Form {
            connectionSection
                .id(SettingsSearchTarget.connection)
            continuePromptSection
                .id(SettingsSearchTarget.continuePrompt)
            quickActionsSection
                .id(SettingsSearchTarget.stopQuickActions)
            pinballSection
            advancedConfigurationSection
            versionSection
        }
        .navigationTitle("Settings")
        .scrollPosition(id: $scrollTarget)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Save") {
                    savePrompt()
                }
                .disabled(!isPromptDirty)
            }
        }
        .task(id: model.snapshot?.globalSettings.defaultPrompt) {
            if let defaultPrompt = model.snapshot?.globalSettings.defaultPrompt {
                draftPrompt = defaultPrompt
            }
        }
        .task(id: initialSearchTarget) {
            scrollToSearchTarget(initialSearchTarget)
        }
        .refreshable {
            await model.prepareForActiveState()
        }
    }

    private var connectionSection: some View {
        Section {
            LabeledContent("Status", value: model.connectionState.label)
            LabeledContent("Linked Mac", value: model.snapshot?.host.name ?? "Not Connected")

            TextField("Enter device code", text: $draftConnectionCode)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.asciiCapable)
                .submitLabel(.done)
                .font(.body.monospaced())
                .onSubmit {
                    connectUsingDeviceCode()
                }

            Button("Connect with Device Code") {
                connectUsingDeviceCode()
            }
            .disabled(draftConnectionCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

            if let connectionCodeErrorMessage {
                Text(connectionCodeErrorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        } header: {
            Text("Connection")
        } footer: {
            Text("Use Scan Orb from the device hub, or enter the device code manually here. The raw Mac URL is hidden from this screen.")
        }
    }

    private var continuePromptSection: some View {
        Section {
            TextEditor(text: $draftPrompt)
                .font(.body)
                .frame(minHeight: CompanionMetrics.editorMinHeight)
        } header: {
            Text("Continue Prompt")
        } footer: {
            Text("This is sent back when you continue a stopped chat.")
        }
    }

    private var quickActionsSection: some View {
        Section {
            ForEach(QuickActionOption.allCases) { action in
                Toggle(
                    action.label,
                    isOn: Binding(
                        get: { selectedQuickActions.contains(action.rawValue) },
                        set: { isEnabled in
                            updateQuickActions(action: action, isEnabled: isEnabled)
                        }
                    )
                )
            }
        } header: {
            Text("Stop Quick Actions")
        } footer: {
            Text("These actions appear when a stop alert expands.")
        }
    }

    private var pinballSection: some View {
        Section {
            Toggle("Pinball", isOn: $isPinballGameEnabled)
            Toggle("Physics Debug Overlay", isOn: $isPinballDebugOverlayEnabled)
                .disabled(!isPinballGameEnabled)
        } header: {
            Text("Pinball")
        } footer: {
            Text("The overlay uses SpriteKit physics, Core Motion tilt, and Core Haptics collision feedback.")
        }
    }

    private var advancedConfigurationSection: some View {
        Section("Advanced Mac Configuration") {
            NavigationLink("Notification Routes") {
                SettingsRoutesScreen(model: model)
            }

            NavigationLink("Completion Checks") {
                SettingsCompletionChecksScreen(model: model)
            }
        }
    }

    private var versionSection: some View {
        Section {
            Text(release.footerLabel)
                .font(.footnote.monospaced())
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.leading)
        }
    }

    private func updateQuickActions(action: QuickActionOption, isEnabled: Bool) {
        var next = selectedQuickActions

        if isEnabled {
            next.insert(action.rawValue)
        } else {
            next.remove(action.rawValue)
        }

        storedQuickActions = next.sorted().joined(separator: ",")
    }

    private func connectUsingDeviceCode() {
        let trimmedConnectionCode = draftConnectionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnectionCode.isEmpty else {
            connectionCodeErrorMessage = "Enter the device code shown on your Mac."
            return
        }

        do {
            let resolvedBaseURL = try CompanionConfiguration.resolveBaseURLString(
                fromConnectionCode: trimmedConnectionCode
            )
            Task {
                await saveResolvedConnectionBaseURL(resolvedBaseURL)
            }
        } catch {
            connectionCodeErrorMessage = error.localizedDescription
        }
    }

    private func saveResolvedConnectionBaseURL(_ baseURL: String) async {
        await MainActor.run {
            connectionCodeErrorMessage = nil
        }

        await model.saveConnectionBaseURL(baseURL)

        await MainActor.run {
            draftConnectionCode = ""

            if model.connectionState == .connected {
                Haptics.success()
            }
        }
    }

    private func scrollToSearchTarget(_ target: SettingsSearchTarget?) {
        guard target == .connection || target == .continuePrompt || target == .stopQuickActions else {
            scrollTarget = nil
            return
        }

        withAnimation(.easeInOut(duration: 0.2)) {
            scrollTarget = target
        }
    }

    private func savePrompt() {
        Task {
            await model.saveDefaultPrompt(draftPrompt)
        }
    }
}

struct SettingsRoutesScreen: View {
    let model: CompanionAppModel

    var body: some View {
        List {
            if let notifications = model.snapshot?.notifications, !notifications.isEmpty {
                ForEach(notifications) { notification in
                    LabeledContent(notification.label, value: notification.channel.capitalized)
                }
            } else {
                ContentUnavailableView(
                    "No Routes Configured",
                    systemImage: "bell.slash",
                    description: Text("Create routes on the Mac and they will appear here automatically.")
                )
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Notification Routes")
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct SettingsCompletionChecksScreen: View {
    let model: CompanionAppModel

    var body: some View {
        List {
            if let completionChecks = model.snapshot?.completionChecks, !completionChecks.isEmpty {
                ForEach(completionChecks) { completionCheck in
                    LabeledContent(completionCheck.label) {
                        Text(commandCountLabel(completionCheck.commandCount))
                    }
                }
            } else {
                ContentUnavailableView(
                    "No Completion Checks",
                    systemImage: "checkmark.circle",
                    description: Text("Once you define checks on the Mac, they will be listed here.")
                )
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("Completion Checks")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func commandCountLabel(_ count: Int) -> String {
        count == 1 ? "1 command" : "\(count) commands"
    }
}

import SwiftUI
import LooperCompanionCore

struct SettingsScreen: View {
    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator
    var initialSearchTarget: SettingsSearchTarget? = nil
    var embedsInNavigationStack = true

    @Environment(\.openURL) private var openURL
    @State private var connectionCodeErrorMessage: String?
    @State private var draftConnectionCode = ""
    @AppStorage(QuickActionSettings.storageKey) private var storedQuickActions =
        QuickActionSettings.defaultStorageValue
    @AppStorage("appearanceMode") private var appearanceModeRawValue = CompanionAppearanceMode.system.rawValue
    @AppStorage(CompanionConfiguration.connectionRoutePreferenceKey) private var connectionRoutePreferenceRawValue =
        CompanionConnectionRoutePreference.defaultPreference.rawValue
    @AppStorage(OnboardingState.completionStorageKey) private var hasCompletedOnboarding = false
    @AppStorage("pinballGameEnabled") private var isPinballGameEnabled = false
    @AppStorage("pinballDebugOverlayEnabled") private var isPinballDebugOverlayEnabled = false
    @State private var draftPrompt = ""
    @State private var isConnecting = false
    @State private var isOrbScannerPresented = false
    @State private var isUpdatingFaceIDUnlock = false
    @State private var localNetworkAccess = LocalNetworkAccessMonitor()
    @State private var scrollTarget: SettingsSearchTarget?
    @FocusState private var focusedInput: SettingsInput?

    private var selectedQuickActions: Set<QuickActionOption> {
        QuickActionSettings.actions(from: storedQuickActions)
    }

    private var appearanceMode: Binding<CompanionAppearanceMode> {
        Binding(
            get: {
                CompanionAppearanceMode(rawValue: appearanceModeRawValue) ?? .system
            },
            set: { nextMode in
                appearanceModeRawValue = nextMode.rawValue
            }
        )
    }

    private var release: CompanionRelease {
        CompanionConfiguration.currentRelease()
    }

    private var connectionRoutePreference: Binding<CompanionConnectionRoutePreference> {
        Binding(
            get: {
                CompanionConnectionRoutePreference(rawValue: connectionRoutePreferenceRawValue) ??
                    .defaultPreference
            },
            set: { nextPreference in
                connectionRoutePreferenceRawValue = nextPreference.rawValue
                Task {
                    await model.setConnectionRoutePreference(nextPreference)
                }
            }
        )
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
            securitySection
                .id(SettingsSearchTarget.security)
            appearanceSection
            pinballSection
            advancedConfigurationSection
            versionSection
        }
        .navigationTitle("Settings")
        .scrollPosition(id: $scrollTarget)
        .scrollDismissesKeyboard(.interactively)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("Save") {
                    savePrompt()
                }
                .disabled(!isPromptDirty || model.isSavingDefaultPrompt)
            }

            ToolbarItemGroup(placement: .keyboard) {
                Spacer()

                Button("Done") {
                    focusedInput = nil
                }
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

            Picker("Route", selection: connectionRoutePreference) {
                ForEach(CompanionConnectionRoutePreference.allCases) { preference in
                    Text(preference.settingsLabel)
                        .tag(preference)
                }
            }
            .pickerStyle(.segmented)

            Toggle(
                isOn: Binding(
                    get: { localNetworkAccess.status.isToggleOn },
                    set: { isEnabled in
                        updateLocalNetworkAccess(isEnabled)
                    }
                )
            ) {
                Label("Local Network Access", systemImage: localNetworkAccess.status.symbolName)
            }
            .disabled(localNetworkAccess.isChecking)

            Text(localNetworkAccess.status.summary)
                .font(.footnote)
                .foregroundStyle(.secondary)

            if localNetworkAccess.isChecking {
                ProgressView("Checking Local Network")
            }

            if localNetworkAccess.status.canOpenAppSettings {
                Button("Open iOS Settings") {
                    localNetworkAccess.openAppSettings()
                }
            }

            tailscaleRows

            TextField("Enter device code", text: $draftConnectionCode)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.asciiCapable)
                .submitLabel(.done)
                .font(.body.monospaced())
                .focused($focusedInput, equals: .connectionCode)
                .onSubmit {
                    connectUsingDeviceCode()
                    focusedInput = nil
                }

            Button("Connect with Device Code") {
                connectUsingDeviceCode()
            }
            .disabled(isConnecting || draftConnectionCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

            Button {
                isOrbScannerPresented = true
            } label: {
                Label("Scan Mac Orb", systemImage: "viewfinder.circle")
            }
            .disabled(isConnecting)

            Button("Run Setup Again") {
                hasCompletedOnboarding = false
            }

            if isConnecting {
                ProgressView("Connecting")
            }

            if let connectionCodeErrorMessage {
                Text(connectionCodeErrorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        } header: {
            Text("Connection")
        } footer: {
            Text("Scan the Mac orb or enter the device code manually. The raw Mac URL is hidden from this screen.")
        }
        .fullScreenCover(isPresented: $isOrbScannerPresented) {
            OrbScannerScreen { orbID in
                try await connectUsingOrbID(orbID)
            }
        }
    }

    @ViewBuilder
    private var tailscaleRows: some View {
        let tailscale = model.serverHealth?.tailscale

        LabeledContent("Tailscale", value: tailscale?.statusLabel ?? "Unknown")

        if let detailLabel = tailscale?.detailLabel, !detailLabel.isEmpty {
            Text(detailLabel)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }

        if let magicDNSSuffix = tailscale?.magicDNSSuffix {
            LabeledContent("Tailnet", value: magicDNSSuffix)
        }

        if let baseURL = tailscale?.baseURL {
            LabeledContent("Tailnet URL", value: baseURL)
        }

        Button {
            openTailscaleDownload()
        } label: {
            Label("Open Tailscale", systemImage: "arrow.up.forward.app")
        }
    }

    private var continuePromptSection: some View {
        Section {
            TextEditor(text: $draftPrompt)
                .font(.body)
                .frame(minHeight: CompanionMetrics.editorMinHeight)
                .focused($focusedInput, equals: .continuePrompt)
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
                        get: { selectedQuickActions.contains(action) },
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

    private var securitySection: some View {
        Section {
            Toggle(
                isOn: Binding(
                    get: { authenticator.isFaceIDUnlockEnabled },
                    set: { isEnabled in
                        updateFaceIDUnlock(isEnabled)
                    }
                )
            ) {
                Label("Face ID Unlock", systemImage: "faceid")
            }
            .disabled(
                !CompanionConfiguration.hasAuthenticatedConnection() ||
                    isUpdatingFaceIDUnlock ||
                    authenticator.isAuthenticating
            )

            if !CompanionConfiguration.hasAuthenticatedConnection() {
                Text("Login with a Mac device code before enabling Face ID Unlock.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            Text(authenticator.faceIDStatusMessage)
                .font(.footnote)
                .foregroundStyle(.secondary)

            if authenticator.isFaceIDUnlockEnabled {
                Button("Lock Now") {
                    authenticator.lockIfNeeded()
                }
                .disabled(!authenticator.isUnlocked)
            }

            if authenticator.isAuthenticating {
                ProgressView("Waiting for Face ID")
            }

            if let errorMessage = authenticator.errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        } header: {
            Text("App Security")
        } footer: {
            Text("The private key stays in Secure Enclave. Your Mac stores only the public key and verifies each unlock challenge.")
        }
    }

    private var appearanceSection: some View {
        Section("Appearance") {
            Picker("Appearance", selection: appearanceMode) {
                ForEach(CompanionAppearanceMode.allCases) { mode in
                    Text(mode.label)
                        .tag(mode)
                }
            }
            .pickerStyle(.segmented)
        }
    }

    private var pinballSection: some View {
        Section {
            Toggle("Pinball", isOn: $isPinballGameEnabled)
            Toggle("Physics Debug Overlay", isOn: $isPinballDebugOverlayEnabled)
                .disabled(!isPinballGameEnabled)
            NavigationLink("Maze") {
                SettingsMazeScreen()
            }
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
            next.insert(action)
        } else {
            next.remove(action)
        }

        storedQuickActions = QuickActionSettings.storageValue(for: next)
        model.configureStopQuickActions()
    }

    private func updateFaceIDUnlock(_ isEnabled: Bool) {
        guard !isUpdatingFaceIDUnlock else {
            return
        }

        isUpdatingFaceIDUnlock = true
        Task {
            await authenticator.setFaceIDUnlockEnabled(isEnabled)
            await MainActor.run {
                isUpdatingFaceIDUnlock = false
            }
        }
    }

    private func updateLocalNetworkAccess(_ isEnabled: Bool) {
        Task {
            await localNetworkAccess.setAccessRequested(isEnabled)
        }
    }

    private func openTailscaleDownload() {
        guard let appStoreURL = URL(string: TailscaleAppLink.appStoreURLString) else {
            return
        }

        openURL(appStoreURL)
    }

    private func connectUsingDeviceCode() {
        guard !isConnecting else {
            return
        }

        let trimmedConnectionCode = draftConnectionCode.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedConnectionCode.isEmpty else {
            connectionCodeErrorMessage = "Enter the device code shown on your Mac."
            return
        }

        isConnecting = true
        Task {
            defer {
                isConnecting = false
            }

            do {
                try await model.saveConnectionCode(trimmedConnectionCode)
                connectionCodeErrorMessage = nil
                draftConnectionCode = ""

                if model.connectionState == .connected {
                    Haptics.success()
                } else {
                    connectionCodeErrorMessage = model.errorMessage ??
                        "Device code saved, but looper is not reachable yet."
                    Haptics.warning()
                }
            } catch {
                connectionCodeErrorMessage = error.localizedDescription
                Haptics.error()
            }
        }
    }

    private func connectUsingOrbID(_ orbID: String) async throws {
        guard !isConnecting else {
            return
        }

        isConnecting = true
        defer {
            isConnecting = false
        }

        do {
            try await model.saveConnectionOrbID(orbID)
            connectionCodeErrorMessage = nil
            draftConnectionCode = ""
        } catch {
            connectionCodeErrorMessage = error.localizedDescription
            throw error
        }
    }

    private func scrollToSearchTarget(_ target: SettingsSearchTarget?) {
        switch target {
        case .connection, .continuePrompt, .stopQuickActions, .security:
            break
        case .notificationRoutes, .completionChecks, nil:
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

private enum SettingsInput: Hashable {
    case connectionCode
    case continuePrompt
}

private enum TailscaleAppLink {
    // Tailscale reserves its app scheme for Tailnet Lock signing links, not generic launch.
    static let appStoreURLString = "https://apps.apple.com/app/tailscale/id1470499037"
}

private extension CompanionConnectionRoutePreference {
    var settingsLabel: String {
        switch self {
        case .remote:
            return "Remote"
        case .tailscale:
            return "Tailscale"
        case .lan:
            return "LAN"
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

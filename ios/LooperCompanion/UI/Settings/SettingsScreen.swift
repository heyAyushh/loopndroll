import SwiftUI
import LooperCompanionCore

struct SettingsScreen: View {
    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator
    var initialSearchTarget: SettingsSearchTarget? = nil
    var initialSearchRequestID = 0
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
    @AppStorage(PinballSettingsKeys.isGameEnabled) private var isPinballGameEnabled = false
    @AppStorage(PinballSettingsKeys.isDebugOverlayEnabled) private var isPinballDebugOverlayEnabled = false
    @State private var draftPrompt = ""
    @State private var isConnecting = false
    @State private var isOrbScannerPresented = false
    @State private var isUpdatingFaceIDUnlock = false
    @State private var localNetworkAccess = LocalNetworkAccessMonitor()
    @State private var scrollTarget: SettingsSearchTarget?
    @State private var settingsPath = NavigationPath()
    @State private var lastSyncedDefaultPrompt: String?
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
                guard
                    let preference = CompanionConnectionRoutePreference(
                        rawValue: connectionRoutePreferenceRawValue
                    ),
                    CompanionConnectionRoutePreference.allCases.contains(preference)
                else {
                    return .defaultPreference
                }

                return preference
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
        draftPrompt != model.viewState.defaultPrompt
    }

    var body: some View {
        Group {
            if embedsInNavigationStack {
                NavigationStack(path: $settingsPath) {
                    settingsContentWithDestinations
                }
            } else {
                settingsContentWithDestinations
            }
        }
    }

    private var settingsContentWithDestinations: some View {
        settingsContent
            .navigationDestination(for: SettingsSearchTarget.self) { target in
                settingsDestination(for: target)
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
                .accessibilityIdentifier("settings.save")
            }

            ToolbarItemGroup(placement: .keyboard) {
                Spacer()

                Button("Done") {
                    focusedInput = nil
                }
                .accessibilityIdentifier("settings.keyboard-done")
            }
        }
        .task(id: model.viewState.defaultPrompt) {
            syncDraftPromptFromModelIfSafe()
        }
        .task(id: initialSearchTaskID) {
            openSettingsTarget(initialSearchTarget)
        }
        .refreshable {
            await model.reconcileLocalSessionState(reason: .manualRefresh)
        }
    }

    private var connectionSection: some View {
        Section {
            LabeledContent("Status", value: model.viewState.connectivityStatusLabel)
            LabeledContent("Linked Mac", value: model.viewState.hostName ?? "Not Connected")

            Picker("Route", selection: connectionRoutePreference) {
                ForEach(CompanionConnectionRoutePreference.allCases) { preference in
                    Label(preference.settingsLabel, systemImage: preference.settingsSystemImageName)
                        .tag(preference)
                }
            }
            .pickerStyle(.segmented)

            if let routePresentation = model.viewState.connectionRoutePresentation {
                ConnectionRouteSummaryRow(title: "Current Route", presentation: routePresentation)
            } else {
                LabeledContent {
                    Text("Waiting")
                        .foregroundStyle(.secondary)
                } label: {
                    Label("Current Route", systemImage: "network")
                }
            }

            Text(connectionRoutePreference.wrappedValue.settingsDetail)
                .font(.footnote)
                .foregroundStyle(.secondary)

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
            .accessibilityIdentifier("settings.local-network-access")

            Text(localNetworkAccess.status.summary)
                .font(.footnote)
                .foregroundStyle(.secondary)

            if localNetworkAccess.isChecking {
                ProgressView("Checking Local Network")
            }

            if localNetworkAccess.status.canOpenAppSettings {
                Button {
                    localNetworkAccess.openAppSettings()
                } label: {
                    Label("Open iOS Settings", systemImage: "gear")
                }
                .accessibilityIdentifier("settings.open-ios-settings")
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
                .accessibilityIdentifier("settings.connection-code")

            Button {
                connectUsingDeviceCode()
            } label: {
                Label("Login with Device Code", systemImage: "link.badge.plus")
            }
            .disabled(isConnecting || draftConnectionCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .accessibilityIdentifier("settings.login-device-code")

            Button {
                isOrbScannerPresented = true
            } label: {
                Label("Scan Mac Orb", systemImage: "viewfinder.circle")
            }
            .disabled(isConnecting)
            .accessibilityIdentifier("settings.scan-mac-orb")

            Button {
                hasCompletedOnboarding = false
            } label: {
                Label("Run Setup Again", systemImage: "arrow.clockwise")
            }
            .accessibilityIdentifier("settings.run-setup-again")

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

        LabeledContent {
            Text(tailscale?.statusLabel ?? "Unknown")
        } label: {
            Label {
                Text("Tailscale")
            } icon: {
                TailscaleLogoMark(color: .blue)
                    .frame(
                        width: ConnectionRouteVisuals.defaultIconSize,
                        height: ConnectionRouteVisuals.defaultIconSize
                    )
            }
        }

        if let detailLabel = tailscale?.detailLabel, !detailLabel.isEmpty {
            Text(detailLabel)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
        }

        if let magicDNSSuffix = tailscale?.magicDNSSuffix {
            LabeledContent("Tailnet", value: magicDNSSuffix)
        }

        if tailscale?.running == true, let baseURL = tailscale?.baseURL {
            LabeledContent("Tailnet URL", value: baseURL)
        }

        Button {
            openTailscaleDownload()
        } label: {
            Label("Open Tailscale in App Store", systemImage: "arrow.up.forward.app")
        }
        .accessibilityIdentifier("settings.open-tailscale")
    }

    private var continuePromptSection: some View {
        Section {
            TextEditor(text: $draftPrompt)
                .font(.body)
                .frame(minHeight: CompanionMetrics.editorMinHeight)
                .scrollDisabled(focusedInput != .continuePrompt)
                .focused($focusedInput, equals: .continuePrompt)
                .accessibilityIdentifier("settings.default-prompt-editor")
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
                .accessibilityIdentifier("settings.quick-action.\(action.rawValue)")
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
            .accessibilityIdentifier("settings.face-id-unlock")

            if !CompanionConfiguration.hasAuthenticatedConnection() {
                Text("Login with a Mac device code before enabling Face ID Unlock.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }

            Text(authenticator.faceIDStatusMessage)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("settings.face-id-status")

            if authenticator.isFaceIDUnlockEnabled {
                Button("Lock Now") {
                    authenticator.lockIfNeeded()
                }
                .disabled(!authenticator.isUnlocked)
                .accessibilityIdentifier("settings.lock-now")
            }

            if authenticator.isAuthenticating {
                ProgressView("Waiting for Face ID")
            }

            if let errorMessage = authenticator.errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("settings.face-id-error")
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
                .accessibilityIdentifier("settings.pinball.enabled")
            Toggle("Physics Debug Overlay", isOn: $isPinballDebugOverlayEnabled)
                .disabled(!isPinballGameEnabled)
                .accessibilityIdentifier("settings.pinball.debug-overlay")
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
            .accessibilityIdentifier("settings.notification-routes")

            NavigationLink("Completion Checks") {
                SettingsCompletionChecksScreen(model: model)
            }
            .accessibilityIdentifier("settings.completion-checks")
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

    @ViewBuilder
    private func settingsDestination(for target: SettingsSearchTarget) -> some View {
        switch target {
        case .notificationRoutes:
            SettingsRoutesScreen(model: model)
        case .completionChecks:
            SettingsCompletionChecksScreen(model: model)
        case .connection, .continuePrompt, .stopQuickActions, .security:
            SettingsScreen(
                model: model,
                authenticator: authenticator,
                initialSearchTarget: target,
                initialSearchRequestID: initialSearchRequestID,
                embedsInNavigationStack: false
            )
        }
    }

    private var initialSearchTaskID: String {
        [
            initialSearchTarget?.rawValue ?? "none",
            String(initialSearchRequestID),
        ].joined(separator: ":")
    }

    private func openSettingsTarget(_ target: SettingsSearchTarget?) {
        switch target {
        case .notificationRoutes, .completionChecks:
            guard embedsInNavigationStack, let target else {
                scrollTarget = nil
                return
            }
            var path = NavigationPath()
            path.append(target)
            settingsPath = path
            scrollTarget = nil
        case .connection, .continuePrompt, .stopQuickActions, .security:
            settingsPath = NavigationPath()
            scrollToSearchTarget(target)
        case nil:
            scrollTarget = nil
        }
    }

    private func savePrompt() {
        Task {
            await model.saveDefaultPrompt(draftPrompt)
        }
    }

    private func syncDraftPromptFromModelIfSafe() {
        let nextDefaultPrompt = model.viewState.defaultPrompt
        if draftPrompt == nextDefaultPrompt {
            lastSyncedDefaultPrompt = nextDefaultPrompt
            return
        }

        guard !hasUnsavedPromptEdits(relativeTo: nextDefaultPrompt) else {
            return
        }

        draftPrompt = nextDefaultPrompt
        lastSyncedDefaultPrompt = nextDefaultPrompt
    }

    private func hasUnsavedPromptEdits(relativeTo nextDefaultPrompt: String) -> Bool {
        guard let lastSyncedDefaultPrompt else {
            return !draftPrompt.isEmpty && draftPrompt != nextDefaultPrompt
        }
        if focusedInput == .continuePrompt {
            return draftPrompt != lastSyncedDefaultPrompt ||
                nextDefaultPrompt != lastSyncedDefaultPrompt
        }
        return draftPrompt != lastSyncedDefaultPrompt &&
            draftPrompt != nextDefaultPrompt
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
            return "Tailscale"
        case .tailscale:
            return "Tailscale"
        case .lan:
            return "LAN"
        }
    }

    var settingsSystemImageName: String {
        switch self {
        case .remote:
            return "circle.grid.3x3.fill"
        case .tailscale:
            return "circle.grid.3x3.fill"
        case .lan:
            return "wifi.router"
        }
    }

    var settingsDetail: String {
        switch self {
        case .remote:
            return "Tailscale is tried first, then LAN when the tailnet route is unavailable."
        case .tailscale:
            return "Tailscale is tried first, then LAN when the tailnet route is unavailable."
        case .lan:
            return "LAN is tried first, then Tailscale when local network access is unavailable."
        }
    }
}

struct SettingsRoutesScreen: View {
    let model: CompanionAppModel

    var body: some View {
        List {
            let notifications = model.viewState.availableNotifications
            if !notifications.isEmpty {
                ForEach(notifications) { notification in
                    NotificationDestinationRow(destination: notification)
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
            let completionChecks = model.viewState.availableCompletionChecks
            if !completionChecks.isEmpty {
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

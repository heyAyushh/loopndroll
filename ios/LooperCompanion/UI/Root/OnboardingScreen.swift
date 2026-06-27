import SwiftUI

enum OnboardingState {
    static let completionStorageKey = "looper.hasCompletedOnboarding.v1"
}

struct OnboardingScreen: View {
    let model: CompanionAppModel
    let authenticator: CompanionAppAuthenticator
    let onComplete: () -> Void

    @State private var connectionCodeErrorMessage: String?
    @State private var draftConnectionCode = ""
    @State private var isConnecting = false
    @State private var isOrbScannerPresented = false
    @State private var isUpdatingFaceIDUnlock = false
    @State private var localNetworkAccess = LocalNetworkAccessMonitor()
    @FocusState private var focusedField: OnboardingField?

    private var hasAuthenticatedMacLink: Bool {
        CompanionConfiguration.hasAuthenticatedConnection()
    }

    private var canFinishSetup: Bool {
        hasAuthenticatedMacLink && model.connectionState.allowsOnboardingCompletion
    }

    var body: some View {
        NavigationStack {
            Form {
                macLoginSection
                localNetworkSection
                notificationsSection
                faceIDSection
                completionSection
            }
            .navigationTitle("Set Up looper")
            .scrollDismissesKeyboard(.interactively)
            .task {
                await model.prepareForActiveState()
                await localNetworkAccess.checkAccess()
            }
            .toolbar {
                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()

                    Button("Done") {
                        focusedField = nil
                    }
                    .accessibilityIdentifier("onboarding.keyboard-done")
                }
            }
            .fullScreenCover(isPresented: $isOrbScannerPresented) {
                OrbScannerScreen { orbID in
                    try await connectUsingOrbID(orbID)
                }
            }
        }
    }

    private var macLoginSection: some View {
        Section {
            LabeledContent("Status", value: model.viewState.connectivityStatusLabel)
            LabeledContent("Linked Mac", value: model.viewState.hostName ?? "Not Connected")

            TextField("Enter device code", text: $draftConnectionCode)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.asciiCapable)
                .submitLabel(.done)
                .font(.body.monospaced())
                .focused($focusedField, equals: .connectionCode)
                .onSubmit {
                    connectUsingDeviceCode()
                    focusedField = nil
                }

            Button {
                connectUsingDeviceCode()
            } label: {
                Label("Login with Device Code", systemImage: "link.badge.plus")
            }
            .disabled(isConnecting || draftConnectionCode.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)

            if isConnecting {
                ProgressView("Connecting")
            }

            Button {
                isOrbScannerPresented = true
            } label: {
                Label("Scan Mac Orb", systemImage: "viewfinder.circle")
            }
            .disabled(isConnecting)

            if let connectionCodeErrorMessage {
                Text(connectionCodeErrorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }

            if let errorMessage = model.errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        } header: {
            Text("Mac Login")
        }
    }

    private var localNetworkSection: some View {
        Section {
            LabeledContent("Status", value: localNetworkAccess.status.label)

            Button {
                Task {
                    await localNetworkAccess.setAccessRequested(true)
                }
            } label: {
                Label("Enable Local Network", systemImage: localNetworkAccess.status.symbolName)
            }
            .disabled(localNetworkAccess.isChecking)
            .accessibilityIdentifier("onboarding.local-network")

            if localNetworkAccess.isChecking {
                ProgressView("Checking Local Network")
            }

            if localNetworkAccess.status.canOpenAppSettings {
                Button("Open iOS Settings") {
                    localNetworkAccess.openAppSettings()
                }
                .accessibilityIdentifier("onboarding.open-ios-settings")
            }

            Text(localNetworkAccess.status.summary)
                .font(.footnote)
                .foregroundStyle(.secondary)
        } header: {
            Text("Network")
        }
    }

    private var notificationsSection: some View {
        Section {
            LabeledContent("Status", value: model.viewState.localNotificationStatusLabel)

            Button {
                Task {
                    await model.enableLocalNotifications()
                }
            } label: {
                Label("Enable Notifications", systemImage: "bell.badge")
            }

            Text(model.viewState.remotePushDetailMessage)
                .font(.footnote)
                .foregroundStyle(.secondary)
        } header: {
            Text("Notifications")
        }
    }

    private var faceIDSection: some View {
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
            .disabled(!hasAuthenticatedMacLink || isUpdatingFaceIDUnlock || authenticator.isAuthenticating)
            .accessibilityIdentifier("onboarding.face-id-unlock")

            if authenticator.isAuthenticating {
                ProgressView("Waiting for Face ID")
            }

            if let errorMessage = authenticator.errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }

            Text(authenticator.faceIDStatusMessage)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("onboarding.face-id-status")
        } header: {
            Text("Security")
        }
    }

    private var completionSection: some View {
        Section {
            Button {
                finishSetup()
            } label: {
                Label("Start Using looper", systemImage: "arrow.right.circle.fill")
                    .frame(maxWidth: .infinity, alignment: .center)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(!canFinishSetup)
            .accessibilityLabel("Start Using looper")
            .accessibilityIdentifier("onboarding.start")
        } header: {
            Text("Finish")
        }
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
                if model.connectionState == .connected {
                    connectionCodeErrorMessage = nil
                    draftConnectionCode = ""
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

    private func updateFaceIDUnlock(_ isEnabled: Bool) {
        guard !isUpdatingFaceIDUnlock else {
            return
        }

        isUpdatingFaceIDUnlock = true
        Task {
            await authenticator.setFaceIDUnlockEnabled(isEnabled)
            isUpdatingFaceIDUnlock = false
        }
    }

    private func finishSetup() {
        guard hasAuthenticatedMacLink else {
            connectionCodeErrorMessage = "Login with the Mac device code first."
            return
        }

        guard canFinishSetup else {
            connectionCodeErrorMessage = "Complete the live Mac connection before continuing."
            return
        }

        onComplete()
    }
}

private enum OnboardingField: Hashable {
    case connectionCode
}

private extension ConnectivityState {
    var allowsOnboardingCompletion: Bool {
        switch self {
        case .connecting, .connected, .offline, .locked:
            return true
        case .unauthorized, .unpaired:
            return false
        }
    }
}

#Preview {
    OnboardingScreen(
        model: CompanionAppModel(environment: CompanionEnvironment(service: MockCompanionService())),
        authenticator: CompanionAppAuthenticator(),
        onComplete: {}
    )
}

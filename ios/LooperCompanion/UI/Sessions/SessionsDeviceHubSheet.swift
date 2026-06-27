import SwiftUI

struct SessionsToolbarOrbButton: View {
    private let orbButtonSize: CGFloat = 32
    private let hitTargetSize: CGFloat = 44

    var body: some View {
        toolbarImage
            .scaledToFill()
            .frame(width: orbButtonSize, height: orbButtonSize)
            .clipShape(Circle())
            .frame(width: hitTargetSize, height: hitTargetSize)
            .contentShape(Circle())
            .pinballSurface(cornerRadius: hitTargetSize / 2, material: .metal)
    }

    @ViewBuilder
    private var toolbarImage: some View {
        CompanionCachedImage(
            asset: .notificationOrb,
            fallbackSystemImage: "circle"
        )
    }
}

struct SessionsDeviceHubSheet: View {
    let model: CompanionAppModel

    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var isOrbScannerPresented = false

    private var alertActionTitle: String {
        if model.viewState.areLocalNotificationsDenied {
            return "Open Notification Settings"
        }

        return model.viewState.canSendLocalNotifications ? "Send Test Alert" : "Enable Notifications"
    }

    private var alertActionSubtitle: String {
        if model.viewState.areLocalNotificationsDenied {
            return "Notifications are off for this iPhone."
        }

        return model.viewState.canSendLocalNotifications
            ? "Confirm delivery on this iPhone."
            : "Allow alerts before you rely on stop notifications."
    }

    private var alertActionSystemImageName: String {
        if model.viewState.areLocalNotificationsDenied {
            return "gear"
        }

        return model.viewState.canSendLocalNotifications ? "bell.badge" : "bell"
    }

    private var personalDeviceName: String {
        UIDevice.current.name
    }

    private var deviceSoftwareLabel: String {
        "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)"
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Device") {
                    LabeledContent("This iPhone", value: personalDeviceName)
                    LabeledContent("Software", value: deviceSoftwareLabel)
                    LabeledContent("Mac", value: model.viewState.hostName ?? "No Mac Connected")
                    LabeledContent("API", value: model.viewState.deviceHubAPIStatusLabel)
                    if let routePresentation = model.viewState.connectionRoutePresentation {
                        ConnectionRouteSummaryRow(title: "Current Route", presentation: routePresentation)
                    } else if let baseURL = model.viewState.activeConnectionRouteBaseURLString {
                        LabeledContent("Current Route", value: baseURL)
                    }
                    LabeledContent("Access", value: model.viewState.deviceHubAccessStatusLabel)
                    LabeledContent("Connection", value: model.viewState.deviceHubConnectionStatusLabel)
                }
                .listRowBackground(Color.clear)

                Section("Actions") {
                    Button {
                        isOrbScannerPresented = true
                    } label: {
                        Label("Scan Mac Orb", systemImage: "viewfinder.circle")
                    }
                    .accessibilityIdentifier("device-hub.scan-orb")
                }
                .listRowBackground(Color.clear)

                Section("Alerts") {
                    LabeledContent("Notifications", value: model.viewState.localNotificationStatusLabel)
                    LabeledContent("Remote Push", value: model.viewState.remotePushStatusLabel)

                    Button {
                        Task {
                            await runAlertAction()
                        }
                    } label: {
                        Label {
                            VStack(alignment: .leading, spacing: 4) {
                                Text(alertActionTitle)
                                Text(alertActionSubtitle)
                                    .font(.footnote)
                                    .foregroundStyle(.secondary)
                            }
                        } icon: {
                            Image(systemName: alertActionSystemImageName)
                        }
                    }
                    .accessibilityIdentifier("device-hub.alert-action")
                }
                .listRowBackground(Color.clear)
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Color.clear)
            .navigationTitle("This iPhone")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .accessibilityIdentifier("device-hub.done")
                }
            }
        }
        .fullScreenCover(isPresented: $isOrbScannerPresented) {
            OrbScannerScreen { orbID in
                try await model.saveConnectionOrbID(orbID)
            }
        }
    }

    private func runAlertAction() async {
        if model.viewState.areLocalNotificationsDenied {
            guard let settingsURL = URL(string: UIApplication.openSettingsURLString) else {
                return
            }

            openURL(settingsURL)
            return
        }

        if model.viewState.canSendLocalNotifications {
            await model.sendTestAlert()
            return
        }

        await model.enableLocalNotifications()
    }
}

import SwiftUI
import UIKit

private enum ToolbarOrbAsset {
    static let resourceName = "notification-orb"
    static let fileExtension = "png"

    static var image: UIImage? {
        if let resourceURL = Bundle.main.url(
            forResource: resourceName,
            withExtension: fileExtension
        ) {
            return UIImage(contentsOfFile: resourceURL.path)
        }

        return UIImage(named: resourceName)
    }
}

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
        if let image = ToolbarOrbAsset.image {
            Image(uiImage: image)
                .resizable()
        } else {
            Image(systemName: "circle")
                .resizable()
                .foregroundStyle(.secondary)
        }
    }
}

struct SessionsDeviceHubSheet: View {
    let model: CompanionAppModel

    @Environment(\.openURL) private var openURL
    @State private var isOrbScannerPresented = false

    private var syncLabel: String {
        guard let host = model.snapshot?.host else {
            return "Waiting for first sync"
        }

        return ModelFormatting.relativeTimestamp(host.lastSyncedAt)
    }

    private var alertActionTitle: String {
        if model.areLocalNotificationsDenied {
            return "Open Notification Settings"
        }

        return model.canSendLocalNotifications ? "Send Test Alert" : "Enable Notifications"
    }

    private var alertActionSubtitle: String {
        if model.areLocalNotificationsDenied {
            return "Notifications are off for this iPhone."
        }

        return model.canSendLocalNotifications
            ? "Confirm delivery on this iPhone."
            : "Allow alerts before you rely on stop notifications."
    }

    private var personalDeviceName: String {
        UIDevice.current.name
    }

    private var deviceSoftwareLabel: String {
        "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)"
    }

    private var loginStatusLabel: String {
        switch model.connectionState {
        case .connected:
            return "Approved"
        case .connecting:
            return "Checking"
        case .offline:
            return "Offline"
        case .unauthorized:
            return "Needs Approval"
        case .locked:
            return "Locked"
        case .unpaired:
            return "Not Linked"
        }
    }

    private var serverStatusLabel: String {
        guard let serverHealth = model.serverHealth else {
            return model.connectionState == .connected ? "Unknown" : "Offline"
        }

        return serverHealth.ok ? "Running" : "Unavailable"
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Device") {
                    LabeledContent("This iPhone", value: personalDeviceName)
                    LabeledContent("Software", value: deviceSoftwareLabel)
                    LabeledContent("Mac", value: model.snapshot?.host.name ?? "No Mac Connected")
                    LabeledContent("API", value: serverStatusLabel)
                    if let baseURL = model.activeConnectionRouteBaseURLString {
                        LabeledContent("API Route", value: baseURL)
                    }
                    LabeledContent("Access", value: loginStatusLabel)
                    LabeledContent("Last Sync", value: syncLabel)
                }
                .listRowBackground(Color.clear)

                Section("Actions") {
                    Button {
                        isOrbScannerPresented = true
                    } label: {
                        Label("Scan Orb", systemImage: "viewfinder.circle")
                    }
                }
                .listRowBackground(Color.clear)

                Section("Alerts") {
                    LabeledContent("Notifications", value: model.localNotificationStatusLabel)
                    LabeledContent("Remote Push", value: model.remotePushStatusLabel)

                    Button {
                        Task {
                            await runAlertAction()
                        }
                    } label: {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(alertActionTitle)
                            Text(alertActionSubtitle)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .listRowBackground(Color.clear)
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(Color.clear)
            .navigationTitle("This iPhone")
            .navigationBarTitleDisplayMode(.inline)
        }
        .fullScreenCover(isPresented: $isOrbScannerPresented) {
            OrbScannerScreen { orbID in
                try await model.saveConnectionOrbID(orbID)
            }
        }
    }

    private func runAlertAction() async {
        if model.areLocalNotificationsDenied {
            guard let settingsURL = URL(string: UIApplication.openSettingsURLString) else {
                return
            }

            openURL(settingsURL)
            return
        }

        if model.canSendLocalNotifications {
            await model.sendTestAlert()
            return
        }

        await model.enableLocalNotifications()
    }
}

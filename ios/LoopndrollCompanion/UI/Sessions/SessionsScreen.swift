import SwiftUI

struct SessionsScreen: View {
    let model: CompanionAppModel
    let openSettings: () -> Void

    @State private var isDeviceHubPresented = false

    private var hasVisibleSessions: Bool {
        !model.needsAttentionSessions.isEmpty ||
            !model.runningSessions.isEmpty ||
            !model.archivedSessions.isEmpty
    }

    var body: some View {
        NavigationStack {
            List {
                connectionSection

                if !model.needsAttentionSessions.isEmpty {
                    sessionSection(title: "Needs Attention", sessions: model.needsAttentionSessions)
                }

                if !model.runningSessions.isEmpty {
                    sessionSection(title: "Active", sessions: model.runningSessions)
                }

                if !model.archivedSessions.isEmpty {
                    sessionSection(title: "Archived", sessions: model.archivedSessions)
                }
            }
            .listStyle(.insetGrouped)
            .contentMargins(.top, 0, for: .scrollContent)
            .companionListSurface()
            .navigationTitle("Sessions")
            .navigationDestination(for: SessionSummary.self) { session in
                SessionDetailScreen(model: model, session: session)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        isDeviceHubPresented = true
                    } label: {
                        SessionsToolbarOrbButton()
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Open device hub")
                }
            }
            .refreshable {
                await model.refresh()
            }
            .overlay {
                overlayState
            }
        }
        .sheet(isPresented: $isDeviceHubPresented) {
            SessionsDeviceHubSheet(model: model)
                .deviceHubSheetPresentation()
        }
    }

    @ViewBuilder
    private var overlayState: some View {
        if model.isLoading && model.snapshot == nil {
            ProgressView("Loading Looper")
        } else if !hasVisibleSessions {
            ContentUnavailableView(
                unavailableStateTitle,
                systemImage: model.connectionState.symbolName,
                description: Text(model.connectivitySummary)
            )
        }
    }

    private var connectionSection: some View {
        Section {
            Button {
                openSettings()
            } label: {
                SessionConnectionRow(
                    title: model.connectivityHeadline,
                    subtitle: connectionSubtitle,
                    statusText: model.connectionState.label,
                    statusTint: CompanionTint.tint(for: model.connectionState)
                )
            }
            .buttonStyle(.plain)
            .companionCardRowSurface()
        } footer: {
            if let errorMessage = model.errorMessage, !errorMessage.isEmpty {
                Text(errorMessage)
                    .foregroundStyle(.red)
            }
        }
    }

    private var connectionSubtitle: String {
        guard let host = model.snapshot?.host else {
            return model.connectivitySummary
        }

        return "Last synced \(ModelFormatting.relativeTimestamp(host.lastSyncedAt))"
    }

    private func sessionSection(title: String, sessions: [SessionSummary]) -> some View {
        Section(title) {
            ForEach(sessions) { session in
                NavigationLink(value: session) {
                    SessionRow(session: session)
                }
                .companionCardRowSurface()
            }
        }
    }

    private var unavailableStateTitle: String {
        switch model.connectionState {
        case .connected:
            return "No Sessions"
        case .connecting:
            return "Connecting to Your Mac"
        case .offline:
            return "Mac Offline"
        case .unauthorized:
            return "Connection Needs Approval"
        case .unpaired:
            return "Set Up Your Mac Link"
        }
    }
}

extension View {
    func deviceHubSheetPresentation() -> some View {
        presentationDetents([.fraction(0.75), .large])
            .presentationDragIndicator(.visible)
    }
}

private struct SessionConnectionRow: View {
    let title: String
    let subtitle: String
    let statusText: String
    let statusTint: Color

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.headline)
                    .foregroundStyle(.primary)

                Text(subtitle)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)
            }

            Spacer(minLength: 12)

            StatusPill(text: statusText, tint: statusTint)
        }
        .padding(.vertical, 4)
    }
}

#Preview {
    let model = CompanionAppModel(environment: CompanionEnvironment(service: MockCompanionService()))
    model.snapshot = PreviewFixtures.snapshot

    return SessionsScreen(model: model, openSettings: {})
}

import SwiftUI

private enum SessionFilter: String, CaseIterable, Identifiable {
    case active = "Active"
    case archived = "Archived"

    var id: String { rawValue }
}

struct SessionsScreen: View {
    let model: CompanionAppModel
    @State private var filter: SessionFilter = .active

    private var sessions: [SessionSummary] {
        switch filter {
        case .active:
            return model.activeSessions
        case .archived:
            return model.archivedSessions
        }
    }

    private var connectionTint: Color {
        switch model.connectionState {
        case .connected:
            return .green
        case .connecting:
            return .orange
        case .offline, .unauthorized, .unpaired:
            return .red
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if model.isLoading && model.snapshot == nil {
                    ProgressView("Loading Loopndroll")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 20) {
                            connectionCard

                            Picker("Session Filter", selection: $filter) {
                                ForEach(SessionFilter.allCases) { option in
                                    Text(option.rawValue).tag(option)
                                }
                            }
                            .pickerStyle(.segmented)

                            if sessions.isEmpty {
                                ContentUnavailableView(
                                    filter == .active ? "No active sessions" : "No archived sessions",
                                    systemImage: "message",
                                    description: Text("Loopndroll sessions will appear here once the Mac app is managing chats.")
                                )
                                .padding(.top, 24)
                            } else {
                                LazyVStack(spacing: 12) {
                                    ForEach(sessions) { session in
                                        NavigationLink(value: session) {
                                            SessionRow(session: session)
                                        }
                                        .buttonStyle(.plain)
                                    }
                                }
                            }
                        }
                        .padding(20)
                    }
                    .refreshable {
                        await model.refresh()
                    }
                }
            }
            .navigationTitle("Sessions")
            .navigationDestination(for: SessionSummary.self) { session in
                SessionDetailScreen(model: model, session: session)
            }
        }
    }

    private var connectionCard: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text(model.snapshot?.host.name ?? "Loopndroll Host")
                        .font(.title3.weight(.semibold))
                    Text(model.snapshot?.host.address ?? "Waiting for a paired Mac")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                StatusPill(text: model.connectionState.label, tint: connectionTint)
            }

            if let globalSettings = model.snapshot?.globalSettings {
                HStack(alignment: .top, spacing: 16) {
                    infoItem(label: "Mode", value: ModelFormatting.friendlyMode(globalSettings.globalMode))
                    infoItem(label: "Notification", value: globalSettings.notificationLabel ?? "None")
                    infoItem(label: "Check", value: globalSettings.completionCheckLabel ?? "None")
                }
            }

            if let errorMessage = model.errorMessage {
                Text(errorMessage)
                    .font(.footnote)
                    .foregroundStyle(.red)
            }
        }
        .padding(18)
        .background(
            LinearGradient(
                colors: [Color.blue.opacity(0.18), Color.cyan.opacity(0.12)],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            ),
            in: RoundedRectangle(cornerRadius: 22, style: .continuous)
        )
    }

    private func infoItem(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            Text(value)
                .font(.subheadline)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

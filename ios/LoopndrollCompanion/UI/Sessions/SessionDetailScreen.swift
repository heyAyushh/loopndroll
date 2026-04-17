import SwiftUI

struct SessionDetailScreen: View {
    let model: CompanionAppModel
    let session: SessionSummary

    @State private var showingDeleteConfirmation = false

    private var detail: SessionDetail? {
        model.detail(for: session.id)
    }

    var body: some View {
        List {
            headerSection
            modeSection
            attachmentsSection
            destructiveSection
        }
        .navigationTitle(session.ref)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await model.loadSessionDetail(id: session.id)
        }
        .confirmationDialog(
            "Delete Session",
            isPresented: $showingDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete Session", role: .destructive) {
                Haptics.warning()
                Task {
                    await model.deleteSession(session.id)
                }
            }
        } message: {
            Text("This removes the session from the Loopndroll list on Mac and iPhone.")
        }
    }

    private var headerSection: some View {
        Section {
            VStack(alignment: .leading, spacing: 12) {
                Text(session.title)
                    .font(.title3.weight(.semibold))
                HStack {
                    StatusPill(text: detail?.status.label ?? session.status.label, tint: tint(for: detail?.status ?? session.status))
                    StatusPill(text: ModelFormatting.friendlyMode(detail?.effectiveMode ?? session.effectiveMode), tint: .blue)
                }
                if let message = detail?.latestAssistantMessage ?? session.assistantPreview {
                    Text(message)
                        .font(.body)
                        .foregroundStyle(.secondary)
                }
                Text("Updated \(ModelFormatting.relativeTimestamp(detail?.lastUpdatedAt ?? session.lastUpdatedAt))")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            .padding(.vertical, 8)
        }
    }

    private var modeSection: some View {
        Section("Mode") {
            ForEach(SessionMode.allCases, id: \.rawValue) { mode in
                Button {
                    Task {
                        await model.applyMode(mode, to: session.id)
                    }
                } label: {
                    HStack {
                        Text(mode.label)
                        Spacer()
                        if (detail?.effectiveMode ?? session.effectiveMode) == mode {
                            Image(systemName: "checkmark.circle.fill")
                                .foregroundStyle(.blue)
                        }
                    }
                }
            }

            Button("Turn Off Mode") {
                Task {
                    await model.applyMode(nil, to: session.id)
                }
            }
        }
    }

    private var attachmentsSection: some View {
        Section("Attachments") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Notifications")
                    .font(.subheadline.weight(.semibold))
                ForEach(detail?.availableNotifications ?? model.snapshot?.notifications ?? [], id: \.id) { notification in
                    HStack {
                        Text(notification.label)
                        Spacer()
                        if detail?.notificationIds.contains(notification.id) == true {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.green)
                        }
                    }
                }
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("Completion Check")
                    .font(.subheadline.weight(.semibold))
                Text(
                    detail?.availableCompletionChecks.first(where: { $0.id == detail?.completionCheckID })?.label
                        ?? "No completion check attached"
                )
                .foregroundStyle(.secondary)
                if detail?.completionCheckWaitForReply == true {
                    Label("Waits for reply after checks", systemImage: "bubble.left.and.bubble.right")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    private var destructiveSection: some View {
        Section("Manage Session") {
            Button((detail?.isArchived ?? session.isArchived) ? "Unarchive Session" : "Archive Session") {
                Haptics.warning()
                Task {
                    await model.setSessionArchived(!(detail?.isArchived ?? session.isArchived), sessionID: session.id)
                }
            }
            .foregroundStyle(.orange)

            Button("Delete Session", role: .destructive) {
                showingDeleteConfirmation = true
            }
        }
    }

    private func tint(for status: SessionStatus) -> Color {
        switch status {
        case .active:
            return .green
        case .waiting:
            return .orange
        case .stopped:
            return .blue
        case .archived:
            return .gray
        }
    }
}

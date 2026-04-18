import SwiftUI

struct SessionDetailScreen: View {
    let model: CompanionAppModel
    let session: SessionSummary

    @Environment(\.dismiss) private var dismiss
    @State private var showingDeleteConfirmation = false

    private var detail: SessionDetail? {
        model.detail(for: session.id)
    }

    private var currentStatus: SessionStatus {
        detail?.status ?? session.status
    }

    private var currentMode: SessionMode? {
        detail?.effectiveMode ?? session.effectiveMode
    }

    private var currentMessage: String? {
        detail?.latestAssistantMessage ?? session.assistantPreview
    }

    private var availableNotifications: [NotificationDestination] {
        detail?.availableNotifications ?? model.snapshot?.notifications ?? []
    }

    private var selectedCompletionCheck: CompletionCheckSummary? {
        detail?.availableCompletionChecks.first(where: { $0.id == detail?.completionCheckID })
    }

    var body: some View {
        Form {
            summarySection
            modeSection
            notificationsSection
            completionCheckSection
            manageSection
        }
        .navigationTitle(session.ref)
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await model.refreshSessionDetail(id: session.id)
        }
        .refreshable {
            await model.refreshSessionDetail(id: session.id)
        }
        .confirmationDialog(
            "Delete Session",
            isPresented: $showingDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete Session", role: .destructive) {
                Task {
                    await model.deleteSession(session.id)
                    dismiss()
                }
            }
        } message: {
            Text("This removes the session from the looper list on Mac and iPhone.")
        }
    }

    private var summarySection: some View {
        Section("Summary") {
            LabeledContent("Title", value: session.title)
            LabeledContent("Status", value: currentStatus.label)
            LabeledContent("Updated") {
                Text(ModelFormatting.relativeTimestamp(detail?.lastUpdatedAt ?? session.lastUpdatedAt))
            }
            LabeledContent("Mode", value: ModelFormatting.friendlyMode(currentMode))

            if let currentMessage, !currentMessage.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Latest Assistant Reply")
                        .font(.footnote)
                        .foregroundStyle(.secondary)

                    Text(currentMessage)
                        .font(.body)
                        .foregroundStyle(.primary)
                }
                .padding(.vertical, 4)
            }
        }
    }

    private var modeSection: some View {
        Section {
            ForEach(SessionMode.allCases, id: \.rawValue) { mode in
                Button {
                    Task {
                        await model.applyMode(mode, to: session.id)
                    }
                } label: {
                    HStack(spacing: 12) {
                        Label(mode.label, systemImage: mode.symbolName)
                            .foregroundStyle(.primary)

                        Spacer()

                        if currentMode == mode {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                        }
                    }
                }
            }

            Button {
                Task {
                    await model.applyMode(nil, to: session.id)
                }
            } label: {
                HStack {
                    Label("Use Global Default", systemImage: "dial.low")
                    Spacer()
                    if currentMode == nil {
                        Image(systemName: "checkmark")
                            .foregroundStyle(.tint)
                    }
                }
            }
        } header: {
            Text("Mode")
        } footer: {
            Text(currentMode?.summary ?? "This session follows the global Looper default.")
        }
    }

    private var notificationsSection: some View {
        Section("Notification Routes") {
            if availableNotifications.isEmpty {
                Text("No notification routes configured on the Mac.")
                    .foregroundStyle(.secondary)
            } else {
                ForEach(availableNotifications) { notification in
                    HStack(spacing: 12) {
                        Label(notification.label, systemImage: channelSymbolName(notification.channel))
                            .foregroundStyle(.primary)

                        Spacer()

                        if detail?.notificationIds.contains(notification.id) == true {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.tint)
                        } else {
                            Text(notification.channel.capitalized)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }

    private var completionCheckSection: some View {
        Section("Completion Check") {
            if let selectedCompletionCheck {
                LabeledContent("Rule", value: selectedCompletionCheck.label)
                LabeledContent(
                    "Reply Requirement",
                    value: detail?.completionCheckWaitForReply == true
                        ? "Waits for reply"
                        : "Ready immediately"
                )
            } else {
                Text("No completion check attached to this session.")
                    .foregroundStyle(.secondary)
            }
        }
    }

    private var manageSection: some View {
        Section {
            Button {
                Task {
                    await model.setSessionArchived(
                        !(detail?.isArchived ?? session.isArchived),
                        sessionID: session.id
                    )
                }
            } label: {
                Label(
                    (detail?.isArchived ?? session.isArchived)
                        ? "Unarchive Session"
                        : "Archive Session",
                    systemImage: (detail?.isArchived ?? session.isArchived)
                        ? "tray.and.arrow.up"
                        : "archivebox"
                )
            }

            Button("Delete Session", role: .destructive) {
                showingDeleteConfirmation = true
            }
        } header: {
            Text("Manage")
        } footer: {
            Text("Archive keeps the history. Delete removes the session from the Mac and this iPhone.")
        }
    }

    private func channelSymbolName(_ channel: String) -> String {
        switch channel.lowercased() {
        case "telegram":
            return "paperplane"
        case "slack":
            return "bubble.left.and.bubble.right"
        default:
            return "bell"
        }
    }
}

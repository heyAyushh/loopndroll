import AppIntents
import SwiftUI

struct SessionDetailRoute: Hashable, Identifiable {
    let sessionID: String
    let assistantSurface: CompanionAssistantSurface

    var id: String {
        "\(assistantSurface.rawValue):\(sessionID)"
    }
}

struct SessionRowDisplayItem: Identifiable, Hashable {
    let session: SessionSummary
    let assistantSurface: CompanionAssistantSurface
    let pendingPromptDelivery: PendingPromptDeliveryPresentation?

    init(
        session: SessionSummary,
        assistantSurface: CompanionAssistantSurface,
        pendingPromptDelivery: PendingPromptDeliveryPresentation? = nil
    ) {
        self.session = session
        self.assistantSurface = assistantSurface
        self.pendingPromptDelivery = pendingPromptDelivery
    }

    var id: String {
        "\(assistantSurface.rawValue):\(session.id)"
    }

    var detailRoute: SessionDetailRoute {
        SessionDetailRoute(
            sessionID: session.id,
            assistantSurface: assistantSurface
        )
    }
}

struct SessionRow: View {
    let session: SessionSummary
    let assistantSurface: CompanionAssistantSurface
    let pendingPromptDelivery: PendingPromptDeliveryPresentation?

    init(
        session: SessionSummary,
        assistantSurface: CompanionAssistantSurface,
        pendingPromptDelivery: PendingPromptDeliveryPresentation? = nil
    ) {
        self.session = session
        self.assistantSurface = assistantSurface
        self.pendingPromptDelivery = pendingPromptDelivery
    }

    private var tint: Color {
        CompanionTint.tint(for: session.status)
    }

    private var pendingPromptTint: Color {
        switch pendingPromptDelivery?.status {
        case .some(.sending):
            return .accentColor
        case .some(.notDeliveredRetry):
            return .orange
        case .none:
            return .secondary
        }
    }

    private var appEntityIdentifier: EntityIdentifier? {
        LooperContinuationActivity.appEntityIdentifier(
            sessionID: session.id,
            assistantSurface: assistantSurface
        )
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 12) {
                VStack(spacing: 4) {
                    AssistantSurfaceLogoMark(surface: assistantSurface)
                        .frame(width: 28, height: 28)
                        .accessibilityLabel(assistantSurface.displayTitle)

                    Text(session.ref)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text(session.title)
                        .font(.headline)
                        .foregroundStyle(.primary)
                        .lineLimit(2)

                    Label(
                        session.metadata.displayTitle,
                        systemImage: session.metadata.kind.symbolName
                    )
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                }

                Spacer(minLength: 12)

                HStack(spacing: 6) {
                    if let goal = session.goal, goal.cardStatusLabel != nil {
                        GoalStatusBadge(goal: goal)
                            .accessibilityIdentifier("session-row.goal-status")
                    }

                    StatusPill(text: session.status.label, tint: tint)
                        .accessibilityIdentifier("session-row.session-status")
                }
            }

            if let assistantPreview = session.assistantPreview, !assistantPreview.isEmpty {
                Text(assistantPreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
                    .contentTransition(.opacity)
                    .animation(.easeInOut(duration: 0.2), value: assistantPreview)
            }

            HStack(spacing: 12) {
                Label(
                    ModelFormatting.friendlyMode(session.effectiveMode),
                    systemImage: session.effectiveMode?.symbolName ?? "dial.low"
                )

                Label(
                    ModelFormatting.sessionFreshness(session),
                    systemImage: "clock"
                )

                if !session.metadata.installedPlugins.isEmpty {
                    Label(
                        "\(session.metadata.installedPlugins.count)",
                        systemImage: "puzzlepiece.extension"
                    )
                }

                if session.metadata.taskKind != .unknown {
                    Label(session.metadata.taskKind.label, systemImage: "tag")
                }

                if let pendingPromptDelivery {
                    StatusPill(text: pendingPromptDelivery.label, tint: pendingPromptTint)
                        .accessibilityIdentifier("session-row.pending-prompt")
                }
            }
            .font(.footnote)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
        .padding(.vertical, 4)
        .looperAppEntityIdentifier(appEntityIdentifier)
    }
}

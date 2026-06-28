import AppIntents
import SwiftUI

struct SessionRowDisplayItem: Identifiable, Hashable {
    let session: SessionSummary
    let assistantSurface: CompanionAssistantSurface

    var id: String {
        "\(assistantSurface.rawValue):\(session.id)"
    }
}

struct SessionRow: View {
    let session: SessionSummary
    let assistantSurface: CompanionAssistantSurface

    private var tint: Color {
        CompanionTint.tint(for: session.status)
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
                AssistantSurfaceLogoMark(surface: assistantSurface)
                    .frame(width: 28, height: 28)
                    .accessibilityLabel(assistantSurface.displayTitle)

                VStack(alignment: .leading, spacing: 4) {
                    Text(session.ref)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)

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

                VStack(alignment: .trailing, spacing: 6) {
                    StatusPill(text: session.status.label, tint: tint)
                        .accessibilityIdentifier("session-row.session-status")

                    if session.hasBlockedGoal,
                       let goal = session.goal,
                       let workStatusLabel = session.workStatusLabel
                    {
                        StatusPill(
                            text: workStatusLabel,
                            tint: SessionGoalStatusVisuals.tint(for: goal)
                        )
                        .accessibilityIdentifier("session-row.goal-status")
                    }
                }
            }

            if let assistantPreview = session.assistantPreview, !assistantPreview.isEmpty {
                Text(assistantPreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
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

                if let workStatusLabel = session.workStatusLabel {
                    Label(workStatusLabel, systemImage: session.workStatusSymbolName)
                        .foregroundStyle(
                            session.goal.map(SessionGoalStatusVisuals.tint(for:)) ?? .secondary
                        )
                        .accessibilityIdentifier("session-row.goal-work-status")
                }

                if session.metadata.taskKind != .unknown {
                    Label(session.metadata.taskKind.label, systemImage: "tag")
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

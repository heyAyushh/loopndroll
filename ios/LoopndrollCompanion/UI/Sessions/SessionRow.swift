import SwiftUI

struct SessionRow: View {
    let session: SessionSummary

    private var tint: Color {
        CompanionTint.tint(for: session.status)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 12) {
                AssistantClientGlyph(client: session.assistantClient)

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

                StatusPill(text: session.status.label, tint: tint)
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
                    ModelFormatting.relativeTimestamp(session.lastUpdatedAt),
                    systemImage: "clock"
                )

                if !session.metadata.installedPlugins.isEmpty {
                    Label(
                        "\(session.metadata.installedPlugins.count)",
                        systemImage: "puzzlepiece.extension"
                    )
                }
            }
            .font(.footnote)
            .foregroundStyle(.secondary)
            .lineLimit(1)
        }
        .padding(.vertical, 4)
    }
}

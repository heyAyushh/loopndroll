import SwiftUI

struct SessionRow: View {
    let session: SessionSummary

    private var tint: Color {
        switch session.status {
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

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(session.ref)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                    Text(session.title)
                        .font(.headline)
                        .foregroundStyle(.primary)
                        .multilineTextAlignment(.leading)
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

            HStack {
                Label(ModelFormatting.friendlyMode(session.effectiveMode), systemImage: "waveform.path.ecg")
                Spacer()
                Text(ModelFormatting.relativeTimestamp(session.lastUpdatedAt))
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(16)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }
}

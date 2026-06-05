import SwiftUI

struct SearchCommandRow: View {
    let title: String
    let subtitle: String
    let systemImage: String
    let categoryLabel: String?

    private var breadcrumb: String {
        categoryLabel ?? subtitle
    }

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: systemImage)
                .font(.footnote.weight(.medium))
                .foregroundStyle(Color.accentColor)
                .frame(width: 28, height: 28)
                .background(Color.accentColor.opacity(0.15), in: RoundedRectangle(cornerRadius: 7))

            VStack(alignment: .leading, spacing: 1) {
                Text(title)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(breadcrumb)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .companionCardRowSurface()
    }
}

struct SearchSessionRow: View {
    let session: SessionSummary

    private var tint: Color {
        CompanionTint.tint(for: session.status)
    }

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            AssistantClientGlyph(
                client: session.assistantClient,
                isWorking: session.status == .active
            )
                .frame(width: 18, height: 18)
                .frame(width: 28, height: 28)
                .background(tint.opacity(0.15), in: RoundedRectangle(cornerRadius: 7))

            VStack(alignment: .leading, spacing: 1) {
                Text(session.title)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text("Sessions -> \(session.status.label)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .companionCardRowSurface()
    }
}

struct RecentSearchRow: View {
    let query: String
    let breadcrumb: String
    let systemImage: String

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: systemImage)
                .font(.footnote.weight(.medium))
                .foregroundStyle(.secondary)
                .frame(width: 28, height: 28)
                .background(Color.secondary.opacity(0.15), in: RoundedRectangle(cornerRadius: 7))

            VStack(alignment: .leading, spacing: 1) {
                Text(query)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(breadcrumb)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .companionCardRowSurface()
    }
}

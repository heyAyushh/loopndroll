import AppIntents
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
        .frame(maxWidth: .infinity, alignment: .leading)
        .contentShape(Rectangle())
        .companionCardRowSurface()
    }
}

struct SearchSessionRow: View {
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
        HStack(alignment: .center, spacing: 12) {
            AssistantSurfaceLogoMark(surface: assistantSurface)
                .frame(width: 18, height: 18)
                .frame(width: 28, height: 28)
                .background(tint.opacity(0.15), in: RoundedRectangle(cornerRadius: 7))
                .accessibilityLabel(assistantSurface.displayTitle)

            VStack(alignment: .leading, spacing: 1) {
                Text(session.title)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(searchBreadcrumb)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: 0)
        }
        .contentShape(Rectangle())
        .companionCardRowSurface()
        .looperAppEntityIdentifier(appEntityIdentifier)
    }

    private var searchBreadcrumb: String {
        let parts = [
            "Sessions",
            session.status.label,
            session.workStatusLabel,
            ModelFormatting.sessionFreshness(session),
        ]
        .compactMap { $0 }

        return parts.joined(separator: " -> ")
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

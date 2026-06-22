import SwiftUI

struct NotificationDestinationPresentation: Equatable, Sendable {
    let title: String
    let channelTitle: String
    let systemImageName: String
    let channelKind: NotificationDestinationChannelKind

    init(destination: NotificationDestination) {
        self.init(label: destination.label, channelIdentifier: destination.channel)
    }

    init(label: String, channelIdentifier: String) {
        let channelKind = NotificationDestinationChannelKind(identifier: channelIdentifier)
        self.channelKind = channelKind
        channelTitle = channelKind.displayTitle(for: channelIdentifier)
        systemImageName = channelKind.systemImageName
        title = label.trimmedNonEmpty ?? channelTitle
    }
}

enum NotificationDestinationChannelKind: Equatable, Sendable {
    case telegram
    case slack
    case macOS
    case iPhone
    case other

    init(identifier: String) {
        switch identifier.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "telegram":
            self = .telegram
        case "slack":
            self = .slack
        case "macos":
            self = .macOS
        case "iphone":
            self = .iPhone
        default:
            self = .other
        }
    }

    var systemImageName: String {
        switch self {
        case .telegram:
            return NotificationDestinationSystemImageName.telegram
        case .slack:
            return NotificationDestinationSystemImageName.slack
        case .macOS:
            return NotificationDestinationSystemImageName.macOS
        case .iPhone:
            return NotificationDestinationSystemImageName.iPhone
        case .other:
            return NotificationDestinationSystemImageName.notification
        }
    }

    func displayTitle(for identifier: String) -> String {
        switch self {
        case .telegram:
            return "Telegram"
        case .slack:
            return "Slack"
        case .macOS:
            return "macOS"
        case .iPhone:
            return "iPhone"
        case .other:
            return identifier.notificationIdentifierDisplayTitle
        }
    }
}

struct NotificationDestinationRow: View {
    let destination: NotificationDestination
    var isSelected = false
    var showsSelection = false

    private var presentation: NotificationDestinationPresentation {
        NotificationDestinationPresentation(destination: destination)
    }

    var body: some View {
        HStack(spacing: NotificationDestinationRowMetrics.contentSpacing) {
            NotificationDestinationGlyph(presentation: presentation)

            VStack(alignment: .leading, spacing: NotificationDestinationRowMetrics.textSpacing) {
                Text(presentation.title)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                Text(presentation.channelTitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer(minLength: NotificationDestinationRowMetrics.trailingSpacing)

            if showsSelection, isSelected {
                Image(systemName: NotificationDestinationSystemImageName.selected)
                    .font(.body.weight(.semibold))
                    .foregroundStyle(.tint)
                    .accessibilityLabel("Selected")
            }
        }
        .accessibilityElement(children: .combine)
    }
}

private struct NotificationDestinationGlyph: View {
    let presentation: NotificationDestinationPresentation

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: NotificationDestinationRowMetrics.glyphCornerRadius)
                .fill(presentation.channelKind.tintColor.opacity(NotificationDestinationPalette.tintOpacity))

            Image(systemName: presentation.systemImageName)
                .font(.system(
                    size: NotificationDestinationRowMetrics.glyphSymbolSize,
                    weight: .semibold
                ))
                .foregroundStyle(presentation.channelKind.tintColor)
        }
        .frame(
            width: NotificationDestinationRowMetrics.glyphSize,
            height: NotificationDestinationRowMetrics.glyphSize
        )
        .accessibilityHidden(true)
    }
}

private enum NotificationDestinationSystemImageName {
    static let iPhone = "iphone"
    static let macOS = "macbook"
    static let notification = "bell.badge"
    static let selected = "checkmark"
    static let slack = "number"
    static let telegram = "paperplane.fill"
}

private enum NotificationDestinationPalette {
    static let fallback = Color.secondary
    static let iPhone = Color.accentColor
    static let macOS = Color.indigo
    static let slack = Color(red: 0.39, green: 0.26, blue: 0.63)
    static let telegram = Color(red: 0.0, green: 0.53, blue: 0.82)
    static let tintOpacity = 0.14
}

private enum NotificationDestinationRowMetrics {
    static let contentSpacing: CGFloat = 12
    static let glyphCornerRadius: CGFloat = 7
    static let glyphSize: CGFloat = 30
    static let glyphSymbolSize: CGFloat = 16
    static let textSpacing: CGFloat = 2
    static let trailingSpacing: CGFloat = 8
}

private extension NotificationDestinationChannelKind {
    var tintColor: Color {
        switch self {
        case .telegram:
            return NotificationDestinationPalette.telegram
        case .slack:
            return NotificationDestinationPalette.slack
        case .macOS:
            return NotificationDestinationPalette.macOS
        case .iPhone:
            return NotificationDestinationPalette.iPhone
        case .other:
            return NotificationDestinationPalette.fallback
        }
    }
}

private extension String {
    var trimmedNonEmpty: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    var notificationIdentifierDisplayTitle: String {
        let words = split { character in
            character == "-" || character == "_" || character == " "
        }
        .map(String.init)
        .map(\.notificationDisplayTitleWord)

        return words.isEmpty ? "Notification" : words.joined(separator: " ")
    }

    var notificationDisplayTitleWord: String {
        switch lowercased() {
        case "api":
            return "API"
        case "ios":
            return "iOS"
        case "macos":
            return "macOS"
        default:
            return prefix(1).uppercased() + dropFirst()
        }
    }
}

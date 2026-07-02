import LooperCompanionCore
import SwiftUI

struct StatusPill: View {
    let text: String
    let tint: Color
    var systemImage: String?
    var isActive = false

    init(
        text: String,
        tint: Color,
        systemImage: String? = nil,
        isActive: Bool = false
    ) {
        self.text = text
        self.tint = tint
        self.systemImage = systemImage
        self.isActive = isActive
    }

    var body: some View {
        HStack(spacing: StatusPillMetrics.iconSpacing) {
            if let systemImage {
                Image(systemName: systemImage)
                    .symbolEffect(.variableColor.iterative, isActive: isActive)
                    .accessibilityHidden(true)
            }

            Text(text)
        }
            .font(.caption.weight(.medium))
            .foregroundStyle(tint)
            .lineLimit(1)
            .padding(.horizontal, StatusPillMetrics.horizontalPadding)
            .padding(.vertical, StatusPillMetrics.verticalPadding)
            .background(Capsule().fill(tint.opacity(StatusPillMetrics.backgroundOpacity)))
            .fixedSize(horizontal: true, vertical: false)
            .pinballSurface(cornerRadius: StatusPillMetrics.cornerRadius, material: .soft)
    }
}

private enum StatusPillMetrics {
    static let iconSpacing: CGFloat = 4
    static let horizontalPadding: CGFloat = 10
    static let verticalPadding: CGFloat = 6
    static let backgroundOpacity = 0.14
    static let cornerRadius: CGFloat = 14
}

enum SessionGoalStatusVisuals {
    static func tint(for goal: SessionGoalSummary) -> Color {
        goal.running ? .green : .secondary
    }
}

enum GoalStatusIconMetrics {
    static let defaultSize: CGFloat = 20
    static let assetName = "GoalMark"
}

/// Bullseye board with a dart landing on its center, drawn from the vector
/// GoalMark asset and tinted by goal state.
struct GoalStatusIcon: View {
    let tint: Color
    var size: CGFloat = GoalStatusIconMetrics.defaultSize

    var body: some View {
        Image(GoalStatusIconMetrics.assetName)
            .resizable()
            .renderingMode(.template)
            .scaledToFit()
            .foregroundStyle(tint)
            .frame(width: size, height: size)
            .accessibilityHidden(true)
    }
}

/// Icon-only replacement for the goal status text pill: the goal state is
/// conveyed by tint alone, the status wording lives in accessibility.
struct GoalStatusBadge: View {
    let goal: SessionGoalSummary

    private var tint: Color {
        SessionGoalStatusVisuals.tint(for: goal)
    }

    var body: some View {
        GoalStatusIcon(tint: tint)
            .padding(GoalStatusBadgeMetrics.padding)
            .background(Circle().fill(tint.opacity(StatusPillMetrics.backgroundOpacity)))
            .pinballSurface(
                cornerRadius: GoalStatusBadgeMetrics.surfaceCornerRadius,
                material: .soft
            )
            .accessibilityLabel(goal.displayStatusLabel)
    }
}

private enum GoalStatusBadgeMetrics {
    static let padding: CGFloat = 6
    static let surfaceCornerRadius: CGFloat = 16
}

enum ConnectionRouteVisuals {
    static let defaultIconSize: CGFloat = 16

    static func tint(for route: CompanionBaseURLRoute) -> Color {
        switch route {
        case .tailscale:
            return .blue
        case .lan:
            return .green
        case .remote:
            return .purple
        case .loopback, .unsupported:
            return .secondary
        }
    }
}

struct ConnectionRouteIcon: View {
    let presentation: CompanionConnectionRoutePresentation
    var size = ConnectionRouteVisuals.defaultIconSize

    var body: some View {
        Group {
            if presentation.usesTailscaleLogo {
                TailscaleLogoMark(color: tint)
            } else {
                Image(systemName: presentation.systemImageName)
                    .font(.caption.weight(.semibold))
            }
        }
        .foregroundStyle(tint)
        .frame(width: size, height: size)
    }

    private var tint: Color {
        ConnectionRouteVisuals.tint(for: presentation.route)
    }
}

struct ConnectionRouteSummaryRow: View {
    let title: String
    let presentation: CompanionConnectionRoutePresentation

    var body: some View {
        HStack(alignment: .top, spacing: ConnectionRouteSummaryMetrics.horizontalSpacing) {
            Label {
                Text(title)
            } icon: {
                ConnectionRouteIcon(presentation: presentation)
            }

            Spacer(minLength: ConnectionRouteSummaryMetrics.minimumSpacerLength)

            VStack(alignment: .trailing, spacing: ConnectionRouteSummaryMetrics.detailSpacing) {
                Text(presentation.title)
                    .foregroundStyle(.primary)

                Text(presentation.detail)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.trailing)
                    .textSelection(.enabled)
            }
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title), \(presentation.title)")
        .accessibilityValue(presentation.detail)
    }
}

private enum ConnectionRouteSummaryMetrics {
    static let horizontalSpacing: CGFloat = 12
    static let minimumSpacerLength: CGFloat = 12
    static let detailSpacing: CGFloat = 2
}

private enum TailscaleLogoMarkMetrics {
    static let rowCount = 3
    static let columnCount = 3
    static let dotDiameter: CGFloat = 3.4
    static let dotSpacing: CGFloat = 2.2
}

struct TailscaleLogoMark: View {
    let color: Color

    var body: some View {
        VStack(spacing: TailscaleLogoMarkMetrics.dotSpacing) {
            ForEach(0..<TailscaleLogoMarkMetrics.rowCount, id: \.self) { _ in
                HStack(spacing: TailscaleLogoMarkMetrics.dotSpacing) {
                    ForEach(0..<TailscaleLogoMarkMetrics.columnCount, id: \.self) { _ in
                        Circle()
                            .fill(color)
                            .frame(
                                width: TailscaleLogoMarkMetrics.dotDiameter,
                                height: TailscaleLogoMarkMetrics.dotDiameter
                            )
                    }
                }
            }
        }
    }
}

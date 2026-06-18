import LooperCompanionCore
import SwiftUI

struct StatusPill: View {
    let text: String
    let tint: Color

    var body: some View {
        Text(text)
            .font(.caption.weight(.medium))
            .foregroundStyle(tint)
            .lineLimit(1)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(Capsule().fill(tint.opacity(0.14)))
            .fixedSize(horizontal: true, vertical: false)
            .pinballSurface(cornerRadius: 14, material: .soft)
    }
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

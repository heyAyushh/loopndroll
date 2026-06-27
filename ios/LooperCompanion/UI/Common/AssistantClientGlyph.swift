import SwiftUI

/// Leading badge for session rows: native SF Symbol in a fixed metric (HIG-aligned list accessory).
struct AssistantClientGlyph: View {
    let client: AssistantClient
    var isWorking = false

    private static let size: CGFloat = 28

    var body: some View {
        if client == .codex {
            AssistantSurfaceLogoMark(surface: .codex)
                .frame(width: Self.size, height: Self.size)
                .accessibilityLabel(client.displayTitle)
        } else if client == .claudeCode {
            ClaudeLogoMark()
                .frame(width: Self.size, height: Self.size)
                .accessibilityLabel(client.displayTitle)
        } else if client == .zed {
            ZedLogoMark()
                .frame(width: Self.size, height: Self.size)
                .accessibilityLabel(client.displayTitle)
        } else if let surface = CompanionAssistantSurface(assistantClient: client) {
            AssistantSurfaceLogoMark(surface: surface)
                .frame(width: Self.size, height: Self.size)
                .accessibilityLabel(client.displayTitle)
        } else {
            Image(systemName: client.systemImageName)
                .font(.body.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: Self.size, height: Self.size)
                .accessibilityLabel(client.displayTitle)
        }
    }
}

private struct CodexLogoMark: View {
    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: CodexLogoMetrics.cornerRadius)
                .fill(CodexLogoPalette.background)
            Text(CodexLogoMetrics.letter)
                .font(.system(
                    size: CodexLogoMetrics.letterSize,
                    weight: .heavy,
                    design: .rounded
                ))
                .foregroundStyle(CodexLogoPalette.foreground)
                .minimumScaleFactor(CodexLogoMetrics.minimumScale)
        }
        .aspectRatio(1, contentMode: .fit)
        .accessibilityHidden(true)
    }
}

struct AssistantSurfacePicker: View {
    @Binding var selection: CompanionAssistantSurface
    var isDisabled = false

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: AssistantSurfacePickerMetrics.itemSpacing) {
                ForEach(CompanionAssistantSurface.allCases) { surface in
                    Button {
                        selection = surface
                    } label: {
                        AssistantSurfacePickerItem(
                            surface: surface,
                            isSelected: selection == surface
                        )
                    }
                    .buttonStyle(.plain)
                    .disabled(isDisabled)
                    .accessibilityLabel(surface.displayTitle)
                    .accessibilityIdentifier("assistant.surface.\(surface.rawValue)")
                    .accessibilityAddTraits(selection == surface ? .isSelected : [])
                }
            }
            .padding(.vertical, AssistantSurfacePickerMetrics.verticalPadding)
        }
        .disabled(isDisabled)
        .accessibilityLabel("Assistant")
        .accessibilityIdentifier("assistant.surface.picker")
    }
}

private struct AssistantSurfacePickerItem: View {
    let surface: CompanionAssistantSurface
    let isSelected: Bool

    var body: some View {
        HStack(spacing: AssistantSurfacePickerMetrics.contentSpacing) {
            AssistantSurfaceLogoMark(surface: surface)
                .frame(
                    width: AssistantSurfacePickerMetrics.iconSize,
                    height: AssistantSurfacePickerMetrics.iconSize
                )

            Text(surface.displayTitle)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
        }
        .padding(.horizontal, AssistantSurfacePickerMetrics.horizontalPadding)
        .frame(height: AssistantSurfacePickerMetrics.height)
        .background(
            Capsule()
                .fill(isSelected ? Color.accentColor.opacity(0.16) : Color.secondary.opacity(0.08))
        )
        .overlay {
            Capsule()
                .strokeBorder(isSelected ? Color.accentColor : Color.secondary.opacity(0.18))
        }
        .foregroundStyle(isSelected ? Color.accentColor : Color.primary)
    }
}

struct AssistantSurfaceLogoMark: View {
    let surface: CompanionAssistantSurface

    var body: some View {
        switch surface {
        case .codex:
            CodexLogoMark()
        case .claudeCode:
            ClaudeLogoMark()
        case .devin:
            DevinLogoMark()
        case .grokBuild:
            GrokLogoMark()
        case .zed:
            ZedLogoMark()
        }
    }
}

private struct ZedLogoMark: View {
    var body: some View {
        CompanionCachedImage(
            asset: .zedLogo,
            fallbackSystemImage: "bolt.square"
        )
            .scaledToFit()
            .aspectRatio(1, contentMode: .fit)
    }
}

private struct GrokLogoMark: View {
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        CompanionCachedImage(
            asset: .grokLogo,
            renderingMode: .template,
            fallbackSystemImage: "sparkle"
        )
            .scaledToFit()
            .foregroundStyle(tintColor)
            .padding(GrokLogoMetrics.symbolInset)
            .aspectRatio(1, contentMode: .fit)
    }

    private var tintColor: Color {
        switch colorScheme {
        case .dark:
            return GrokLogoPalette.darkModeTint
        case .light:
            return GrokLogoPalette.lightModeTint
        @unknown default:
            return GrokLogoPalette.lightModeTint
        }
    }
}

private struct DevinLogoMark: View {
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        CompanionCachedImage(
            asset: .devinLogo,
            renderingMode: .template,
            fallbackSystemImage: "d.square"
        )
            .scaledToFit()
            .foregroundStyle(tintColor)
            .aspectRatio(1, contentMode: .fit)
    }

    private var tintColor: Color {
        switch colorScheme {
        case .dark:
            return DevinLogoPalette.darkModeTint
        case .light:
            return DevinLogoPalette.lightModeTint
        @unknown default:
            return DevinLogoPalette.lightModeTint
        }
    }
}

private struct ClaudeLogoMark: View {
    var body: some View {
        ClaudeLogoShape()
            .fill(ClaudeLogoPalette.mark)
            .aspectRatio(1, contentMode: .fit)
    }
}

private struct ClaudeLogoShape: Shape {
    func path(in rect: CGRect) -> Path {
        let frame = ClaudeLogoGeometry.frame(in: rect)
        var path = Path()

        ClaudeLogoGeometry.arms.forEach { arm in
            path.addPath(arm.path(in: frame))
        }
        path.addEllipse(in: ClaudeLogoGeometry.centerMass(in: frame))

        return path
    }
}

private enum AssistantSurfacePickerMetrics {
    static let itemSpacing: CGFloat = 6
    static let contentSpacing: CGFloat = 5
    static let verticalPadding: CGFloat = 2
    static let horizontalPadding: CGFloat = 9
    static let height: CGFloat = 32
    static let iconSize: CGFloat = 18
}

private enum DevinLogoPalette {
    static let darkModeTint = Color.white
    static let lightModeTint = Color.black
}

private enum ClaudeLogoPalette {
    static let mark = Color(red: 0.85, green: 0.45, blue: 0.32)
}

private enum ClaudeLogoGeometry {
    static let designSize: CGFloat = 1_600

    static let arms: [ClaudeLogoArm] = [
        ClaudeLogoArm(end: CGPoint(x: 20, y: 790), width: 142),
        ClaudeLogoArm(end: CGPoint(x: 150, y: 360), width: 142),
        ClaudeLogoArm(end: CGPoint(x: 430, y: 40), width: 150),
        ClaudeLogoArm(end: CGPoint(x: 950, y: 36), width: 132),
        ClaudeLogoArm(end: CGPoint(x: 1_300, y: 240), width: 152),
        ClaudeLogoArm(end: CGPoint(x: 1_580, y: 690), width: 124),
        ClaudeLogoArm(end: CGPoint(x: 1_535, y: 970), width: 124),
        ClaudeLogoArm(end: CGPoint(x: 1_420, y: 1_325), width: 110),
        ClaudeLogoArm(end: CGPoint(x: 1_180, y: 1_480), width: 126),
        ClaudeLogoArm(end: CGPoint(x: 780, y: 1_580), width: 124),
        ClaudeLogoArm(end: CGPoint(x: 400, y: 1_450), width: 134),
        ClaudeLogoArm(end: CGPoint(x: 180, y: 1_210), width: 126)
    ]

    private static let center = CGPoint(x: 780, y: 850)
    private static let centerMassWidth: CGFloat = 400
    private static let centerMassHeight: CGFloat = 310

    static func frame(in rect: CGRect) -> CGRect {
        let side = min(rect.width, rect.height)
        return CGRect(
            x: rect.midX - side / 2,
            y: rect.midY - side / 2,
            width: side,
            height: side
        )
    }

    static func point(_ point: CGPoint, in frame: CGRect) -> CGPoint {
        CGPoint(
            x: frame.minX + point.x / designSize * frame.width,
            y: frame.minY + point.y / designSize * frame.height
        )
    }

    static func length(_ value: CGFloat, in frame: CGRect) -> CGFloat {
        value / designSize * frame.width
    }

    static func centerPoint(in frame: CGRect) -> CGPoint {
        point(center, in: frame)
    }

    static func centerMass(in frame: CGRect) -> CGRect {
        let centerPoint = centerPoint(in: frame)
        return CGRect(
            x: centerPoint.x - length(centerMassWidth, in: frame) / 2,
            y: centerPoint.y - length(centerMassHeight, in: frame) / 2,
            width: length(centerMassWidth, in: frame),
            height: length(centerMassHeight, in: frame)
        )
    }
}

private struct ClaudeLogoArm {
    let end: CGPoint
    let width: CGFloat

    func path(in frame: CGRect) -> Path {
        let start = ClaudeLogoGeometry.centerPoint(in: frame)
        let end = ClaudeLogoGeometry.point(end, in: frame)
        let halfWidth = ClaudeLogoGeometry.length(width, in: frame) / 2
        let direction = CGVector(dx: end.x - start.x, dy: end.y - start.y)
        let length = hypot(direction.dx, direction.dy)
        guard length > 0 else {
            return Path()
        }

        let normal = CGVector(
            dx: -direction.dy / length * halfWidth,
            dy: direction.dx / length * halfWidth
        )

        var path = Path()
        path.move(to: CGPoint(x: start.x + normal.dx, y: start.y + normal.dy))
        path.addLine(to: CGPoint(x: end.x + normal.dx, y: end.y + normal.dy))
        path.addLine(to: CGPoint(x: end.x - normal.dx, y: end.y - normal.dy))
        path.addLine(to: CGPoint(x: start.x - normal.dx, y: start.y - normal.dy))
        path.closeSubpath()
        return path
    }
}

private enum GrokLogoMetrics {
    static let symbolInset: CGFloat = 2
}

private enum GrokLogoPalette {
    static let darkModeTint = Color(red: 0.47, green: 0.47, blue: 0.49)
    static let lightModeTint = Color(red: 0.30, green: 0.30, blue: 0.32)
}

private enum CodexLogoPalette {
    static let background = Color(red: 0.05, green: 0.31, blue: 0.24)
    static let foreground = Color.white
}

private enum CodexLogoMetrics {
    static let cornerRadius: CGFloat = 7
    static let letter = "C"
    static let letterSize: CGFloat = 18
    static let minimumScale: CGFloat = 0.7
}

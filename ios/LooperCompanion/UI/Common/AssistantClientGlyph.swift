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
    var tintOverride: Color? = nil

    var body: some View {
        CompanionCachedImage(
            asset: .codexLogo,
            renderingMode: tintOverride == nil ? nil : .template,
            fallbackSystemImage: "terminal"
        )
        .scaledToFit()
        .foregroundStyle(tintOverride ?? .primary)
        .accessibilityHidden(true)
    }
}

/// Mail-style category picker (iOS 18.2 Mail): the selected surface is a solid-color pill with
/// a white mark and label that stretches to fill the remaining row width, while unselected
/// surfaces sit as gray icon-only rounded squares. Metrics mirror Mail's category bar.
struct AssistantSurfacePicker: View {
    @Binding var selection: CompanionAssistantSurface
    var isDisabled = false

    /// Drives the pill expand/collapse animation locally so the surrounding session list
    /// (which reloads on `selection` changes with animations disabled) never animates.
    @State private var visualSelection: CompanionAssistantSurface?

    private var highlightedSurface: CompanionAssistantSurface {
        visualSelection ?? selection
    }

    var body: some View {
        GeometryReader { geometry in
            ScrollViewReader { scrollProxy in
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: AssistantSurfacePickerMetrics.itemSpacing) {
                        ForEach(CompanionAssistantSurface.allCases) { surface in
                            Button {
                                select(surface)
                            } label: {
                                AssistantSurfacePickerItem(
                                    surface: surface,
                                    isSelected: highlightedSurface == surface
                                )
                            }
                            .buttonStyle(.plain)
                            .disabled(isDisabled)
                            .id(surface.id)
                            .accessibilityLabel(surface.displayTitle)
                            .accessibilityIdentifier("assistant.surface.\(surface.rawValue)")
                            .accessibilityAddTraits(highlightedSurface == surface ? .isSelected : [])
                        }
                    }
                    .frame(minWidth: geometry.size.width)
                }
                .onChange(of: selection) { _, selectedSurface in
                    withAnimation(AssistantSurfacePickerMetrics.selectionAnimation) {
                        visualSelection = selectedSurface
                        scrollProxy.scrollTo(selectedSurface.id, anchor: .center)
                    }
                }
                .onAppear {
                    scrollProxy.scrollTo(selection.id, anchor: .center)
                }
            }
        }
        .frame(height: AssistantSurfacePickerMetrics.barHeight)
        .animation(AssistantSurfacePickerMetrics.selectionAnimation, value: highlightedSurface)
        .disabled(isDisabled)
        .accessibilityLabel("Assistant")
        .accessibilityIdentifier("assistant.surface.picker")
    }

    private func select(_ surface: CompanionAssistantSurface) {
        guard surface != highlightedSurface else {
            return
        }

        Haptics.selectionChanged()
        withAnimation(AssistantSurfacePickerMetrics.selectionAnimation) {
            visualSelection = surface
        }

        var transaction = Transaction(animation: nil)
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            selection = surface
        }
    }
}

private struct AssistantSurfacePickerItem: View {
    let surface: CompanionAssistantSurface
    let isSelected: Bool

    var body: some View {
        HStack(spacing: AssistantSurfacePickerMetrics.contentSpacing) {
            AssistantSurfaceLogoMark(
                surface: surface,
                tintOverride: isSelected
                    ? .white
                    : AssistantSurfacePickerMetrics.unselectedMarkColor
            )
            .frame(
                width: AssistantSurfacePickerMetrics.iconSize,
                height: AssistantSurfacePickerMetrics.iconSize
            )

            if isSelected {
                Text(surface.compactTitle)
                    .font(.callout)
                    .fontWeight(.semibold)
                    .lineLimit(1)
            }
        }
        .frame(maxHeight: .infinity)
        .frame(maxWidth: isSelected ? .infinity : nil)
        .padding(
            .horizontal,
            isSelected
                ? AssistantSurfacePickerMetrics.selectedHorizontalPadding
                : AssistantSurfacePickerMetrics.unselectedHorizontalPadding
        )
        .foregroundStyle(isSelected ? Color.white : AssistantSurfacePickerMetrics.unselectedMarkColor)
        .background(
            isSelected
                ? AssistantSurfaceTintPalette.tint(for: surface)
                : Color(.tertiarySystemFill)
        )
        .clipShape(.rect(
            cornerRadius: AssistantSurfacePickerMetrics.cornerRadius,
            style: .continuous
        ))
    }
}

/// Per-surface accent colors, in the spirit of Mail's per-category tints.
private enum AssistantSurfaceTintPalette {
    static func tint(for surface: CompanionAssistantSurface) -> Color {
        switch surface {
        case .codex:
            return .teal
        case .claudeCode:
            return ClaudeLogoPalette.mark
        case .devin:
            return .blue
        case .grokBuild:
            return Color(red: 0.35, green: 0.37, blue: 0.41)
        case .zed:
            return .indigo
        }
    }
}

struct AssistantSurfaceLogoMark: View {
    let surface: CompanionAssistantSurface
    /// Renders the mark as a single-color silhouette (Mail-style pill content)
    /// instead of the brand colors.
    var tintOverride: Color? = nil

    var body: some View {
        switch surface {
        case .codex:
            CodexLogoMark(tintOverride: tintOverride)
        case .claudeCode:
            ClaudeLogoMark(tintOverride: tintOverride)
        case .devin:
            DevinLogoMark(tintOverride: tintOverride)
        case .grokBuild:
            GrokLogoMark(tintOverride: tintOverride)
        case .zed:
            ZedLogoMark(tintOverride: tintOverride)
        }
    }
}

private struct ZedLogoMark: View {
    var tintOverride: Color? = nil

    var body: some View {
        CompanionCachedImage(
            asset: tintOverride == nil ? .zedLogo : .zedGlyph,
            renderingMode: tintOverride == nil ? nil : .template,
            fallbackSystemImage: "bolt.square"
        )
            .scaledToFit()
            .foregroundStyle(tintOverride ?? .primary)
            .aspectRatio(1, contentMode: .fit)
    }
}

private struct GrokLogoMark: View {
    var tintOverride: Color? = nil

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
        if let tintOverride {
            return tintOverride
        }

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
    var tintOverride: Color? = nil

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
        if let tintOverride {
            return tintOverride
        }

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
    var tintOverride: Color? = nil

    var body: some View {
        ClaudeLogoShape()
            .fill(tintOverride ?? ClaudeLogoPalette.mark)
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

/// Mirrors the iOS 18.2+ Mail category bar: continuous rounded-rect chips (not capsules),
/// icon-only gray chips for unselected categories, a solid tinted pill that stretches into
/// the leftover row width for the selection, `.bouncy` animation on the whole bar, and a
/// horizontally scrolling row that overflows offscreen exactly like Mail's.
private enum AssistantSurfacePickerMetrics {
    static let barHeight: CGFloat = 40
    static let cornerRadius: CGFloat = 14
    static let itemSpacing: CGFloat = 8
    static let contentSpacing: CGFloat = 6
    static let selectedHorizontalPadding: CGFloat = 14
    static let unselectedHorizontalPadding: CGFloat = 20
    static let iconSize: CGFloat = 20
    static let unselectedMarkColor = Color(.systemGray)
    static let selectionAnimation = Animation.bouncy
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

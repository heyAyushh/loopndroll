import SwiftUI
import UIKit

/// Leading badge for session rows: native SF Symbol in a fixed metric (HIG-aligned list accessory).
struct AssistantClientGlyph: View {
    let client: AssistantClient
    var isWorking = false

    private static let size: CGFloat = 28

    var body: some View {
        if client == .codex {
            CodexLogoMark()
                .frame(width: Self.size, height: Self.size)
                .accessibilityLabel(client.displayTitle)
        } else if client == .claudeCode {
            ClaudeLogoMark()
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
    @Environment(\.colorScheme) private var colorScheme

    private var asset: CodexLogoAsset.Variant {
        CodexLogoAsset.variant(for: colorScheme)
    }

    var body: some View {
        CodexLogoPoster(resourceName: asset.posterName)
        .clipShape(RoundedRectangle(cornerRadius: CodexLogoMetrics.cornerRadius))
        .contentShape(RoundedRectangle(cornerRadius: CodexLogoMetrics.cornerRadius))
    }
}

private struct CodexLogoPoster: View {
    let resourceName: String

    var body: some View {
        if let image = UIImage(named: resourceName) {
            Image(uiImage: image)
                .resizable()
                .scaledToFill()
        } else {
            AssistantSurfaceLogoMark(surface: .codex)
        }
    }
}

struct AssistantSurfacePicker: View {
    @Binding var selection: CompanionAssistantSurface
    var isDisabled = false

    var body: some View {
        Picker("Assistant", selection: $selection) {
            ForEach(CompanionAssistantSurface.allCases) { surface in
                Text(surface.displayTitle)
                    .tag(surface)
            }
        }
        .pickerStyle(.segmented)
        .disabled(isDisabled)
        .accessibilityLabel("Assistant")
    }
}

struct AssistantSurfaceLogoMark: View {
    let surface: CompanionAssistantSurface

    var body: some View {
        switch surface {
        case .codex:
            AssistantMonogramLogoMark(
                monogram: "C",
                gradientColors: AssistantSurfaceLogoPalette.codexGradient
            )
        case .devin:
            DevinLogoMark()
        case .grokBuild:
            GrokLogoMark()
        }
    }
}

private struct AssistantMonogramLogoMark: View {
    let monogram: String
    let gradientColors: [Color]

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: AssistantSurfaceLogoMetrics.cornerRadius)
                .fill(
                    LinearGradient(
                        colors: gradientColors,
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )

            Text(monogram)
                .font(
                    .system(
                        size: AssistantSurfaceLogoMetrics.fontSize,
                        weight: .black,
                        design: .rounded
                    )
                )
                .foregroundStyle(.white)
        }
        .aspectRatio(1, contentMode: .fit)
    }
}

private struct GrokLogoMark: View {
    @Environment(\.colorScheme) private var colorScheme

    var body: some View {
        Image(GrokLogoAsset.name)
            .renderingMode(.template)
            .resizable()
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
        Image(DevinLogoAsset.name)
            .renderingMode(.template)
            .resizable()
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

private enum AssistantSurfaceLogoPalette {
    static let codexGradient = [
        Color(red: 0.05, green: 0.12, blue: 0.10),
        Color(red: 0.11, green: 0.42, blue: 0.32)
    ]
}

private enum AssistantSurfaceLogoMetrics {
    static let cornerRadius: CGFloat = 7
    static let fontSize: CGFloat = 13
}

private enum GrokLogoAsset {
    static let name = "GrokLogo"
}

private enum DevinLogoAsset {
    static let name = "DevinLogo"
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

private enum CodexLogoAsset {
    struct Variant {
        let posterName: String
        let videoName: String
    }

    static let light = Variant(
        posterName: "codex-logo-poster",
        videoName: "codex-logo"
    )
    static let dark = Variant(
        posterName: "codex-logo-dark-poster",
        videoName: "codex-logo-dark"
    )
    static let videoExtension = "mp4"

    static func variant(for colorScheme: ColorScheme) -> Variant {
        switch colorScheme {
        case .dark:
            return dark
        case .light:
            return light
        @unknown default:
            return light
        }
    }
}

private enum CodexLogoMetrics {
    static let cornerRadius: CGFloat = 7
}

private extension CompanionAssistantSurface {
    init?(assistantClient: AssistantClient) {
        switch assistantClient {
        case .codex:
            self = .codex
        case .devin:
            self = .devin
        case .grokBuild:
            self = .grokBuild
        case .unknown, .cursor, .claudeCode, .superEngineering, .openclaw:
            return nil
        }
    }

}

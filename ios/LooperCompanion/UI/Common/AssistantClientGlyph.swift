import AVFoundation
import SwiftUI
import UIKit

/// Leading badge for session rows: native SF Symbol in a fixed metric (HIG-aligned list accessory).
struct AssistantClientGlyph: View {
    let client: AssistantClient
    var isWorking = false

    private static let size: CGFloat = 28

    var body: some View {
        if client == .codex {
            CodexLogoMark(isWorking: isWorking)
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
    let isWorking: Bool

    @Environment(\.colorScheme) private var colorScheme
    @State private var isHovering = false

    private var shouldPlay: Bool {
        isWorking || isHovering
    }

    private var asset: CodexLogoAsset.Variant {
        CodexLogoAsset.variant(for: colorScheme)
    }

    var body: some View {
        ZStack {
            CodexLogoPoster(resourceName: asset.posterName)

            if shouldPlay {
                LoopingResourceVideoView(
                    resourceName: asset.videoName,
                    resourceExtension: CodexLogoAsset.videoExtension,
                    isPlaying: shouldPlay
                )
                .mask(CodexLogoPoster(resourceName: asset.posterName))
                .transition(.opacity)
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: CodexLogoMetrics.cornerRadius))
        .contentShape(RoundedRectangle(cornerRadius: CodexLogoMetrics.cornerRadius))
        .onHover { isHovering = $0 }
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

private struct LoopingResourceVideoView: UIViewRepresentable {
    let resourceName: String
    let resourceExtension: String
    let isPlaying: Bool

    func makeUIView(context _: Context) -> LoopingVideoUIView {
        let view = LoopingVideoUIView()
        view.configure(url: Bundle.main.url(forResource: resourceName, withExtension: resourceExtension))
        view.setPlaying(isPlaying)
        return view
    }

    func updateUIView(_ uiView: LoopingVideoUIView, context _: Context) {
        uiView.setPlaying(isPlaying)
    }
}

private final class LoopingVideoUIView: UIView {
    private var queuePlayer: AVQueuePlayer?
    private var playerLooper: AVPlayerLooper?
    private var configuredURL: URL?
    private var isCurrentlyPlaying = false

    override static var layerClass: AnyClass {
        AVPlayerLayer.self
    }

    private var playerLayer: AVPlayerLayer {
        layer as! AVPlayerLayer
    }

    func configure(url: URL?) {
        guard configuredURL != url else {
            return
        }

        queuePlayer?.pause()
        queuePlayer = nil
        playerLooper = nil
        playerLayer.player = nil
        configuredURL = url
        isCurrentlyPlaying = false

        guard let url else {
            return
        }

        let playerItem = AVPlayerItem(url: url)
        let player = AVQueuePlayer()
        player.isMuted = true
        player.actionAtItemEnd = .none
        queuePlayer = player
        playerLooper = AVPlayerLooper(player: player, templateItem: playerItem)
        playerLayer.player = player
        playerLayer.videoGravity = .resizeAspectFill
    }

    func setPlaying(_ shouldPlay: Bool) {
        guard let queuePlayer else {
            return
        }

        guard shouldPlay != isCurrentlyPlaying else {
            return
        }

        isCurrentlyPlaying = shouldPlay
        if shouldPlay {
            queuePlayer.seek(to: .zero)
            queuePlayer.play()
        } else {
            queuePlayer.pause()
            queuePlayer.seek(to: .zero)
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
        ZStack {
            RoundedRectangle(cornerRadius: AssistantSurfaceLogoMetrics.cornerRadius)
                .fill(
                    LinearGradient(
                        colors: gradientColors,
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )

            Text(surface.logoMonogram)
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

    private var gradientColors: [Color] {
        switch surface {
        case .codex:
            return [
                Color(red: 0.05, green: 0.12, blue: 0.10),
                Color(red: 0.11, green: 0.42, blue: 0.32)
            ]
        case .devin:
            return [
                Color(red: 0.08, green: 0.16, blue: 0.36),
                Color(red: 0.35, green: 0.28, blue: 0.72)
            ]
        }
    }
}

private enum AssistantSurfaceLogoMetrics {
    static let cornerRadius: CGFloat = 7
    static let fontSize: CGFloat = 13
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
        case .unknown, .cursor, .claudeCode, .superEngineering, .openclaw:
            return nil
        }
    }

    var logoMonogram: String {
        switch self {
        case .codex:
            return "C"
        case .devin:
            return "D"
        }
    }

}

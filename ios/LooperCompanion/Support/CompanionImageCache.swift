import SwiftUI
import UIKit

enum CompanionBundledImage: String, CaseIterable {
    case codexLogo = "CodexLogo"
    case devinLogo = "DevinLogo"
    case grokLogo = "GrokLogo"
    case zedLogo = "ZedLogo"
    /// Alpha-only Z mark (unlike `zedLogo`, which is a full app-icon tile)
    /// so template rendering produces a silhouette instead of a solid square.
    case zedGlyph = "ZedGlyph"
    case notificationOrb = "notification-orb"

    var fileExtension: String? {
        switch self {
        case .notificationOrb:
            return "png"
        case .codexLogo,
             .devinLogo,
             .grokLogo,
             .zedLogo,
             .zedGlyph:
            return nil
        }
    }
}

final class CompanionImageCache: @unchecked Sendable {
    static let shared = CompanionImageCache()

    private let lock = NSLock()
    private var images: [CompanionBundledImage: UIImage] = [:]

    private init() {}

    func prewarm() {
        for asset in CompanionBundledImage.allCases {
            _ = image(for: asset)
        }
    }

    func image(for asset: CompanionBundledImage) -> UIImage? {
        lock.withLock {
            images[asset]
        } ?? loadAndStoreImage(for: asset)
    }

    private func loadAndStoreImage(for asset: CompanionBundledImage) -> UIImage? {
        let image = loadImage(for: asset)
        if let image {
            lock.withLock {
                images[asset] = image
            }
        }
        return image
    }

    private func loadImage(for asset: CompanionBundledImage) -> UIImage? {
        if let image = UIImage(named: asset.rawValue) {
            return image
        }
        guard let fileExtension = asset.fileExtension,
              let resourceURL = Bundle.main.url(
                forResource: asset.rawValue,
                withExtension: fileExtension
              )
        else {
            return nil
        }
        return UIImage(contentsOfFile: resourceURL.path)
    }
}

struct CompanionCachedImage: View {
    let asset: CompanionBundledImage
    var renderingMode: Image.TemplateRenderingMode? = nil
    var fallbackSystemImage: String

    var body: some View {
        if let uiImage = CompanionImageCache.shared.image(for: asset) {
            imageView(uiImage)
                .resizable()
        } else {
            Image(systemName: fallbackSystemImage)
                .resizable()
                .scaledToFit()
                .foregroundStyle(.secondary)
        }
    }

    private func imageView(_ uiImage: UIImage) -> Image {
        let image = Image(uiImage: uiImage)
        if let renderingMode {
            return image.renderingMode(renderingMode)
        }
        return image.renderingMode(.original)
    }
}

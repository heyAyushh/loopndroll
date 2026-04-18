import Foundation

#if canImport(UIKit)
import CoreImage
import CoreGraphics
import UIKit

private enum OrbCameraFrameScannerMetrics {
    static let maximumScanImageDimension: CGFloat = 1280
    static let centerCropFraction: CGFloat = 0.8
    static let minimumCandidateEdge: CGFloat = 240
    static let sharpenContrastBoost: CGFloat = 1.2
    static let sharpenUnsharpRadius: CGFloat = 1.4
    static let sharpenUnsharpIntensity: CGFloat = 0.9
}

public extension OrbCodeKit {
    static func scanOrbID(fromCameraFrameData frameData: Data) throws -> String {
        guard let image = UIImage(data: frameData) else {
            throw OrbCodeError.imageNormalizationFailed
        }

        return try scanOrbID(fromCameraFrame: image)
    }

    // Lightweight fallback path used by the photo-upload flow. The live camera pipeline
    // now crops upstream via Vision, so this path only needs to cover the few cases where
    // an uploaded image needs a center-crop or a moderate sharpen pass.
    static func scanOrbID(fromCameraFrame image: UIImage) throws -> String {
        let normalizedImage = try normalizedImage(from: image)
        let preparedImage = resizeCameraFrameIfNeeded(normalizedImage)
        var lastScanError: Error?

        for candidate in fallbackCandidates(from: preparedImage) {
            do {
                return try scanOrbID(fromImage: candidate)
            } catch {
                lastScanError = error
            }
        }

        throw lastScanError ?? OrbCodeError.imageNormalizationFailed
    }

    private static func fallbackCandidates(from image: UIImage) -> [UIImage] {
        var baseCandidates = [image]
        let imageContext = CIContext()

        if let centerCrop = centerSquareCrop(
            from: image,
            fraction: OrbCameraFrameScannerMetrics.centerCropFraction
        ) {
            baseCandidates.append(centerCrop)
        }

        var candidates = baseCandidates
        candidates.append(contentsOf: baseCandidates.compactMap { image in
            sharpenedCandidate(from: image, imageContext: imageContext)
        })
        return candidates
    }

    private static func resizeCameraFrameIfNeeded(_ image: UIImage) -> UIImage {
        guard let cgImage = image.cgImage else {
            return image
        }

        let width = CGFloat(cgImage.width)
        let height = CGFloat(cgImage.height)
        let longestEdge = max(width, height)
        guard longestEdge > OrbCameraFrameScannerMetrics.maximumScanImageDimension else {
            return image
        }

        let scale = OrbCameraFrameScannerMetrics.maximumScanImageDimension / longestEdge
        let targetSize = CGSize(
            width: floor(width * scale),
            height: floor(height * scale)
        )
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = false

        return UIGraphicsImageRenderer(size: targetSize, format: format).image { context in
            context.cgContext.interpolationQuality = .high
            image.draw(in: CGRect(origin: .zero, size: targetSize))
        }
    }

    private static func centerSquareCrop(from image: UIImage, fraction: CGFloat) -> UIImage? {
        guard let cgImage = image.cgImage else {
            return nil
        }

        let width = CGFloat(cgImage.width)
        let height = CGFloat(cgImage.height)
        let shortestEdge = min(width, height)
        let candidateEdge = floor(shortestEdge * fraction)

        guard candidateEdge >= OrbCameraFrameScannerMetrics.minimumCandidateEdge else {
            return nil
        }

        let originX = floor((width - candidateEdge) * 0.5)
        let originY = floor((height - candidateEdge) * 0.5)
        let cropRect = CGRect(
            x: originX,
            y: originY,
            width: candidateEdge,
            height: candidateEdge
        )

        guard let croppedImage = cgImage.cropping(to: cropRect) else {
            return nil
        }

        return UIImage(cgImage: croppedImage, scale: image.scale, orientation: .up)
    }

    private static func sharpenedCandidate(
        from image: UIImage,
        imageContext: CIContext
    ) -> UIImage? {
        guard let inputImage = CIImage(image: image) else {
            return nil
        }

        let contrastAdjusted = inputImage.applyingFilter(
            "CIColorControls",
            parameters: [
                kCIInputSaturationKey: 0,
                kCIInputContrastKey: OrbCameraFrameScannerMetrics.sharpenContrastBoost,
            ]
        )
        let sharpened = contrastAdjusted.applyingFilter(
            "CIUnsharpMask",
            parameters: [
                kCIInputRadiusKey: OrbCameraFrameScannerMetrics.sharpenUnsharpRadius,
                kCIInputIntensityKey: OrbCameraFrameScannerMetrics.sharpenUnsharpIntensity,
            ]
        )

        guard let cgImage = imageContext.createCGImage(sharpened, from: sharpened.extent) else {
            return nil
        }

        return UIImage(cgImage: cgImage, scale: image.scale, orientation: .up)
    }
}
#endif

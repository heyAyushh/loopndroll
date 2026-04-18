import Foundation
import Testing
@testable import OrbCodeKit
#if canImport(UIKit)
import CoreImage
import UIKit
#endif

@Test
func orbCodeRoundTrip() throws {
    let orb = try OrbCodeKit.generateOrb(fromData: "https://example.com")
    let scannedOrbID = try OrbCodeKit.scanOrbID(fromPNG: orb.pngData)
    let derivedOrbID = try OrbCodeKit.deriveOrbID(from: "https://example.com")

    #expect(!orb.orbID.isEmpty)
    #expect(scannedOrbID == orb.orbID)
    #expect(derivedOrbID == orb.orbID)
    #expect(try OrbCodeKit.verifyOrb(fromPNG: orb.pngData))
}

#if canImport(UIKit)
@Test
func scanOrbFromLuma8Buffer() throws {
    // Default 1024px: the decoder needs enough resolution to lock the sync pattern,
    // and 512px sits right at the edge where detection becomes unreliable.
    let orb = try OrbCodeKit.generateOrb(fromData: "example")
    let image = try #require(UIImage(data: orb.pngData))
    let cgImage = try #require(image.cgImage)

    let width = cgImage.width
    let height = cgImage.height
    let bytesPerRow = width

    var lumaBytes = [UInt8](repeating: 0, count: width * height)
    // Rendering into a Device Gray context with 8 bpp and bytesPerRow==width produces
    // a tightly packed single-channel luma buffer — the exact shape scan_luma8 expects.
    // CGContext uses a bottom-left origin, so draw with a vertical flip to land the
    // pixels in the same top-left order the decoder expects.
    let colorSpace = CGColorSpaceCreateDeviceGray()
    try lumaBytes.withUnsafeMutableBufferPointer { buffer in
        guard let context = CGContext(
            data: buffer.baseAddress,
            width: width,
            height: height,
            bitsPerComponent: 8,
            bytesPerRow: bytesPerRow,
            space: colorSpace,
            bitmapInfo: CGImageAlphaInfo.none.rawValue
        ) else {
            throw OrbCodeError.imageNormalizationFailed
        }
        context.draw(cgImage, in: CGRect(x: 0, y: 0, width: width, height: height))
    }

    let lumaData = Data(lumaBytes)
    let scannedOrbID = try OrbCodeKit.scanOrbID(
        fromLuma8: lumaData,
        width: UInt32(width),
        height: UInt32(height)
    )

    #expect(scannedOrbID == orb.orbID)
}

@Test
func orbCodeRoundTripFromUIImage() throws {
    let orb = try OrbCodeKit.generateOrb(fromData: "https://example.com")
    let image = try #require(UIImage(data: orb.pngData))

    let scannedOrbID = try OrbCodeKit.scanOrbID(fromImage: image)
    let isValid = try OrbCodeKit.verifyOrb(fromImage: image)

    #expect(scannedOrbID == orb.orbID)
    #expect(isValid)
}

// The lean fallback path used by the photo-upload flow tries the normalized image, then
// a single 80% center crop, then a sharpened variant of each. The frame here mimics a
// screenshot where the orb fills most of the image but is wrapped in chrome that the
// crop step needs to peel away.
@Test
func orbCodeCameraFrameScannerRecoversCenteredOrbInWideFrame() throws {
    let orb = try OrbCodeKit.generateOrb(fromData: "https://example.com")
    let orbImage = try #require(UIImage(data: orb.pngData))
    let framedImage = makeScreenFrameImage(
        orbImage: orbImage,
        frameSize: CGSize(width: 1280, height: 1280),
        orbFrame: CGRect(x: 140, y: 140, width: 1000, height: 1000),
        backgroundColor: UIColor(white: 0.08, alpha: 1)
    )

    #expect((try? OrbCodeKit.scanOrbID(fromImage: framedImage)) == nil)

    let scannedOrbID = try OrbCodeKit.scanOrbID(fromCameraFrame: framedImage)

    #expect(scannedOrbID == orb.orbID)
}

// Exercises the sharpen-variant branch of the lean fallback path.
@Test
func orbCodeCameraFrameScannerHandlesSoftScreenCapture() throws {
    let orb = try OrbCodeKit.generateOrb(fromData: "camera-soft")
    let orbImage = try #require(UIImage(data: orb.pngData))
    let framedImage = makeScreenFrameImage(
        orbImage: orbImage,
        frameSize: CGSize(width: 1280, height: 1280),
        orbFrame: CGRect(x: 160, y: 160, width: 960, height: 960),
        backgroundColor: UIColor(white: 0.18, alpha: 1)
    )
    let softenedImage = try blur(image: framedImage, radius: 1.2)

    let scannedOrbID = try OrbCodeKit.scanOrbID(fromCameraFrame: softenedImage)

    #expect(scannedOrbID == orb.orbID)
}

private func makeScreenFrameImage(
    orbImage: UIImage,
    frameSize: CGSize,
    orbFrame: CGRect,
    backgroundColor: UIColor
) -> UIImage {
    let format = UIGraphicsImageRendererFormat.default()
    format.scale = 1
    format.opaque = true

    return UIGraphicsImageRenderer(size: frameSize, format: format).image { context in
        backgroundColor.setFill()
        context.fill(CGRect(origin: .zero, size: frameSize))
        orbImage.draw(in: orbFrame)
    }
}

private func blur(image: UIImage, radius: Double) throws -> UIImage {
    let ciImage = try #require(CIImage(image: image))
    let filter = try #require(CIFilter(name: "CIGaussianBlur"))
    filter.setValue(ciImage, forKey: kCIInputImageKey)
    filter.setValue(radius, forKey: kCIInputRadiusKey)

    let context = CIContext()
    let outputImage = try #require(filter.outputImage?.cropped(to: ciImage.extent))
    let cgImage = try #require(context.createCGImage(outputImage, from: outputImage.extent))
    return UIImage(cgImage: cgImage)
}
#endif

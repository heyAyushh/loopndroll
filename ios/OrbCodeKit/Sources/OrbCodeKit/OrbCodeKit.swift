import Foundation
import OrbCodeFFI
#if canImport(UIKit)
import UIKit
#endif

public enum OrbCodeError: Error, LocalizedError {
    case ffi(String)
    case invalidUtf8
    case imageNormalizationFailed

    public var errorDescription: String? {
        switch self {
        case let .ffi(message):
            return message
        case .invalidUtf8:
            return "Rust returned invalid UTF-8."
        case .imageNormalizationFailed:
            return "The image could not be converted into a scannable PNG."
        }
    }
}

public struct OrbGeneration: Sendable, Equatable {
    public let orbID: String
    public let pngData: Data

    public init(orbID: String, pngData: Data) {
        self.orbID = orbID
        self.pngData = pngData
    }
}

public enum OrbCodeKit {
    public static func deriveOrbID(from input: String) throws -> String {
        let value = try input.withCString { inputPointer in
            let result = orb_code_derive_id_from_data(inputPointer)
            guard let result else {
                throw lastError()
            }
            defer { orb_code_string_free(result) }
            guard let string = String(validatingCString: result) else {
                throw OrbCodeError.invalidUtf8
            }
            return string
        }
        return value
    }

    public static func generateOrb(fromData input: String, size: UInt32 = 1024) throws -> OrbGeneration {
        let orbID = try deriveOrbID(from: input)
        let pngData = try generatePNG(fromData: input, size: size)
        return OrbGeneration(orbID: orbID, pngData: pngData)
    }

    public static func generatePNG(fromOrbID orbID: String, size: UInt32 = 1024) throws -> Data {
        let buffer = try orbID.withCString { orbIDPointer in
            let buffer = orb_code_generate_png_from_id(orbIDPointer, size)
            if buffer.data == nil || buffer.len == 0 {
                throw lastError()
            }
            return buffer
        }
        defer { orb_code_buffer_free(buffer) }
        return Data(bytes: buffer.data, count: buffer.len)
    }

    public static func generatePNG(fromData input: String, size: UInt32 = 1024) throws -> Data {
        let buffer = try input.withCString { inputPointer in
            let buffer = orb_code_generate_png_from_data(inputPointer, size)
            if buffer.data == nil || buffer.len == 0 {
                throw lastError()
            }
            return buffer
        }
        defer { orb_code_buffer_free(buffer) }
        return Data(bytes: buffer.data, count: buffer.len)
    }

    public static func scanOrbID(fromImageData imageData: Data) throws -> String {
        let value = try imageData.withUnsafeBytes { rawBuffer in
            guard let baseAddress = rawBuffer.bindMemory(to: UInt8.self).baseAddress else {
                throw lastError()
            }
            let result = orb_code_scan_png(baseAddress, imageData.count)
            guard let result else {
                throw lastError()
            }
            defer { orb_code_string_free(result) }
            guard let string = String(validatingCString: result) else {
                throw OrbCodeError.invalidUtf8
            }
            return string
        }
        return value
    }

    public static func scanOrbID(fromPNG pngData: Data) throws -> String {
        try scanOrbID(fromImageData: pngData)
    }

    public static func verifyOrb(fromImageData imageData: Data) throws -> Bool {
        try imageData.withUnsafeBytes { rawBuffer in
            guard let baseAddress = rawBuffer.bindMemory(to: UInt8.self).baseAddress else {
                throw lastError()
            }
            let isValid = orb_code_verify_png(baseAddress, imageData.count)
            if !isValid {
                let errorMessage = readLastErrorMessage()
                if let errorMessage {
                    throw OrbCodeError.ffi(errorMessage)
                }
            }
            return isValid
        }
    }

    public static func verifyOrb(fromPNG pngData: Data) throws -> Bool {
        try verifyOrb(fromImageData: pngData)
    }

    private static func lastError() -> OrbCodeError {
        OrbCodeError.ffi(readLastErrorMessage() ?? "OrbCodeFFI returned an unknown error.")
    }

    private static func readLastErrorMessage() -> String? {
        guard let message = orb_code_last_error_message() else {
            return nil
        }
        defer { orb_code_string_free(message) }
        return String(validatingCString: message)
    }
}

#if canImport(UIKit)
public extension OrbCodeKit {
    static func scanOrbID(fromImage image: UIImage) throws -> String {
        try scanOrbID(fromImageData: normalizedPNGData(from: image))
    }

    static func verifyOrb(fromImage image: UIImage) throws -> Bool {
        try verifyOrb(fromImageData: normalizedPNGData(from: image))
    }

    private static func normalizedPNGData(from image: UIImage) throws -> Data {
        if image.imageOrientation == .up, let pngData = image.pngData() {
            return pngData
        }

        let pixelSize = CGSize(
            width: max(image.size.width * image.scale, 1),
            height: max(image.size.height * image.scale, 1)
        )
        let format = UIGraphicsImageRendererFormat.default()
        format.scale = 1
        format.opaque = false

        let normalizedImage = UIGraphicsImageRenderer(size: pixelSize, format: format).image { _ in
            image.draw(in: CGRect(origin: .zero, size: pixelSize))
        }

        guard let pngData = normalizedImage.pngData() else {
            throw OrbCodeError.imageNormalizationFailed
        }
        return pngData
    }
}
#endif

import Foundation
import Testing
@testable import OrbCodeKit
#if canImport(UIKit)
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
func orbCodeRoundTripFromUIImage() throws {
    let orb = try OrbCodeKit.generateOrb(fromData: "https://example.com")
    let image = try #require(UIImage(data: orb.pngData))

    let scannedOrbID = try OrbCodeKit.scanOrbID(fromImage: image)
    let isValid = try OrbCodeKit.verifyOrb(fromImage: image)

    #expect(scannedOrbID == orb.orbID)
    #expect(isValid)
}
#endif

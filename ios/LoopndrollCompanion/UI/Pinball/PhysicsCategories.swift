import Foundation

enum PinballPhysicsCategory {
    static let none: UInt32 = 0
    static let ball: UInt32 = 0x1 << 0
    static let uiElement: UInt32 = 0x1 << 1
    static let flipper: UInt32 = 0x1 << 2
    static let wall: UInt32 = 0x1 << 3
    static let bumper: UInt32 = 0x1 << 4
    static let anchor: UInt32 = 0x1 << 5
}

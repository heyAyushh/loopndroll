@preconcurrency import SpriteKit

extension SKNode {
    func setPinballDebugCircle(radius: CGFloat) {
        userData = userData ?? NSMutableDictionary()
        userData?["pinballDebugShape"] = "circle"
        userData?["pinballDebugRadius"] = radius
    }

    func setPinballDebugRect(size: CGSize) {
        userData = userData ?? NSMutableDictionary()
        userData?["pinballDebugShape"] = "rect"
        userData?["pinballDebugWidth"] = size.width
        userData?["pinballDebugHeight"] = size.height
    }
}

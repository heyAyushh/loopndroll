@preconcurrency import SpriteKit
@preconcurrency import UIKit

@MainActor
final class UIBodySynchronizer {
    private struct Registration {
        let node: SKNode
        weak var view: UIView?
        var lastFrame: CGRect
        var category: UInt32
    }

    private weak var scene: SKScene?
    private var registrations: [ObjectIdentifier: Registration] = [:]
    private var dirtyViews = Set<ObjectIdentifier>()
    private var displayLink: CADisplayLink?

    init(scene: SKScene) {
        self.scene = scene
        startDisplayLink()
    }

    func register(_ view: UIView, category: UInt32 = PinballPhysicsCategory.uiElement) {
        let id = ObjectIdentifier(view)
        let node = registrations[id]?.node ?? SKNode()
        node.name = "ui-\(id)"
        node.userData = node.userData ?? NSMutableDictionary()
        node.userData?["pinballDebugShape"] = "rect"

        if node.parent == nil {
            scene?.addChild(node)
        }

        registrations[id] = Registration(
            node: node,
            view: view,
            lastFrame: .null,
            category: category
        )
        markDirty(view)
    }

    func markDirty(_ view: UIView) {
        dirtyViews.insert(ObjectIdentifier(view))
    }

    func remove(_ view: UIView) {
        let id = ObjectIdentifier(view)
        registrations[id]?.node.removeFromParent()
        registrations[id] = nil
        dirtyViews.remove(id)
    }

    @objc private func updateDirtyBodies() {
        guard let scene else {
            return
        }

        let idsToUpdate = dirtyViews.isEmpty ? Array(registrations.keys) : Array(dirtyViews)
        for id in idsToUpdate {
            guard var registration = registrations[id], let view = registration.view else {
                registrations[id]?.node.removeFromParent()
                registrations[id] = nil
                continue
            }

            guard view.window != nil, !view.isHidden, view.alpha > 0.1, view.bounds.size != .zero else {
                registration.node.physicsBody = nil
                registrations[id] = registration
                continue
            }

            let frame = view.convert(view.bounds, to: nil)
            guard frame != registration.lastFrame else {
                registrations[id] = registration
                continue
            }

            updateBody(for: registration.node, from: frame, in: scene, category: registration.category)
            registration.lastFrame = frame
            registrations[id] = registration
        }

        dirtyViews.removeAll()
    }

    private func startDisplayLink() {
        displayLink = CADisplayLink(target: self, selector: #selector(updateDirtyBodies))
        displayLink?.preferredFrameRateRange = CAFrameRateRange(minimum: 30, maximum: 60, preferred: 60)
        displayLink?.add(to: .main, forMode: .common)
    }

    private func updateBody(for node: SKNode, from frame: CGRect, in scene: SKScene, category: UInt32) {
        let sceneCenter = scene.convertPoint(
            fromView: CGPoint(x: frame.midX, y: frame.midY)
        )
        node.position = sceneCenter
        node.zPosition = 12
        node.userData?["pinballDebugWidth"] = frame.width
        node.userData?["pinballDebugHeight"] = frame.height

        let body = SKPhysicsBody(rectangleOf: frame.size)
        body.isDynamic = false
        body.categoryBitMask = category
        body.collisionBitMask = PinballPhysicsCategory.ball
        body.contactTestBitMask = PinballPhysicsCategory.ball
        body.restitution = 0.7
        body.friction = 0.22
        node.physicsBody = body
    }

}

@preconcurrency import SpriteKit

@MainActor
final class PinballDebugOverlay {
    var isEnabled = false {
        didSet {
            persistentRoot.isHidden = !isEnabled
            if !isEnabled {
                persistentRoot.removeAllChildren()
            }
        }
    }

    private let persistentRoot = SKNode()
    private let interactionRoot = SKNode()

    init() {
        persistentRoot.name = "pinball-debug-overlay"
        persistentRoot.zPosition = 9_000
        persistentRoot.isHidden = true

        interactionRoot.name = "pinball-interaction-overlay"
        interactionRoot.zPosition = 9_100
    }

    func attach(to scene: SKScene) {
        if persistentRoot.parent == nil {
            scene.addChild(persistentRoot)
        }

        if interactionRoot.parent == nil {
            scene.addChild(interactionRoot)
        }
    }

    func update(in scene: SKScene) {
        guard isEnabled else {
            return
        }

        persistentRoot.removeAllChildren()
        scene.enumerateChildNodes(withName: "//*") { [weak self] node, _ in
            guard let self,
                  node.physicsBody != nil,
                  node !== persistentRoot,
                  node !== interactionRoot,
                  node.parent !== persistentRoot,
                  node.parent !== interactionRoot else {
                return
            }
            self.addPersistentOutline(for: node)
        }
    }

    func flashInteraction(for node: SKNode, impulse: CGFloat) {
        guard let outline = makeOutline(for: node) else {
            return
        }

        let intensity = min(max(impulse / 20, 0.2), 1.0)
        outline.strokeColor = UIColor(
            hue: CGFloat(0.34 - intensity * 0.34),
            saturation: 0.9,
            brightness: 1,
            alpha: 1
        )
        outline.lineWidth = 2 + intensity * 4
        outline.alpha = 0.95
        interactionRoot.addChild(outline)

        outline.run(
            .sequence([
                .group([
                    .fadeOut(withDuration: 0.28),
                    .scale(to: 1.04, duration: 0.28),
                ]),
                .removeFromParent(),
            ])
        )
    }

    func clearInteractionOutlines() {
        interactionRoot.removeAllActions()
        interactionRoot.removeAllChildren()
    }

    private func addPersistentOutline(for node: SKNode) {
        guard let outline = makeOutline(for: node) else {
            return
        }

        outline.strokeColor = .systemMint
        outline.lineWidth = 1.2
        outline.alpha = 0.86
        persistentRoot.addChild(outline)
    }

    private func makeOutline(for node: SKNode) -> SKShapeNode? {
        let shape = node.userData?["pinballDebugShape"] as? String
        let outline: SKShapeNode?

        switch shape {
        case "circle":
            let radius = node.userData?["pinballDebugRadius"] as? CGFloat ?? 8
            outline = SKShapeNode(circleOfRadius: radius)
        case "rect":
            let width = node.userData?["pinballDebugWidth"] as? CGFloat ?? 20
            let height = node.userData?["pinballDebugHeight"] as? CGFloat ?? 20
            let cornerRadius = node.userData?["pinballDebugCornerRadius"] as? CGFloat ?? min(width, height) * 0.18
            outline = SKShapeNode(rectOf: CGSize(width: width, height: height), cornerRadius: cornerRadius)
        default:
            outline = nil
        }

        guard let outline else {
            return nil
        }

        outline.position = node.position
        outline.zRotation = node.zRotation
        outline.fillColor = .clear
        return outline
    }
}

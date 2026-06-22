@preconcurrency import SpriteKit

@MainActor
final class PinballDebugOverlay {
    var isEnabled = false {
        didSet {
            persistentRoot.isHidden = !isEnabled
            if !isEnabled {
                persistentRoot.removeAllChildren()
                persistentOutlinesByNodeID = [:]
            }
        }
    }

    private let persistentRoot = SKNode()
    private let interactionRoot = SKNode()
    private var persistentOutlinesByNodeID: [ObjectIdentifier: PersistentOutline] = [:]

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

        var activeNodeIDs = Set<ObjectIdentifier>()
        scene.enumerateChildNodes(withName: "//*") { [weak self] node, _ in
            guard let self,
                  node.physicsBody != nil,
                  node !== persistentRoot,
                  node !== interactionRoot,
                  node.parent !== persistentRoot,
                  node.parent !== interactionRoot else {
                return
            }
            if let nodeID = self.syncPersistentOutline(for: node) {
                activeNodeIDs.insert(nodeID)
            }
        }

        let staleNodeIDs = persistentOutlinesByNodeID.keys.filter { nodeID in
            !activeNodeIDs.contains(nodeID)
        }
        for nodeID in staleNodeIDs {
            persistentOutlinesByNodeID[nodeID]?.node.removeFromParent()
            persistentOutlinesByNodeID[nodeID] = nil
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

    private func syncPersistentOutline(for node: SKNode) -> ObjectIdentifier? {
        guard let signature = outlineSignature(for: node) else {
            return nil
        }

        let nodeID = ObjectIdentifier(node)

        let outline: SKShapeNode
        if let existingOutline = persistentOutlinesByNodeID[nodeID],
           existingOutline.signature == signature {
            outline = existingOutline.node
        } else {
            persistentOutlinesByNodeID[nodeID]?.node.removeFromParent()
            outline = makeOutline(for: signature)
            outline.strokeColor = .systemMint
            outline.lineWidth = 1.2
            outline.alpha = 0.86
            persistentRoot.addChild(outline)
            persistentOutlinesByNodeID[nodeID] = PersistentOutline(
                signature: signature,
                node: outline
            )
        }

        applyNodeTransform(node, to: outline)
        return nodeID
    }

    private func makeOutline(for node: SKNode) -> SKShapeNode? {
        guard let signature = outlineSignature(for: node) else {
            return nil
        }

        let outline = makeOutline(for: signature)
        applyNodeTransform(node, to: outline)
        return outline
    }

    private func outlineSignature(for node: SKNode) -> OutlineSignature? {
        let shape = node.userData?["pinballDebugShape"] as? String

        switch shape {
        case "circle":
            let radius = node.userData?["pinballDebugRadius"] as? CGFloat ?? 8
            return OutlineSignature(
                shape: .circle,
                radius: radius,
                width: 0,
                height: 0,
                cornerRadius: 0
            )
        case "rect":
            let width = node.userData?["pinballDebugWidth"] as? CGFloat ?? 20
            let height = node.userData?["pinballDebugHeight"] as? CGFloat ?? 20
            let cornerRadius = node.userData?["pinballDebugCornerRadius"] as? CGFloat ?? min(width, height) * 0.18
            return OutlineSignature(
                shape: .rect,
                radius: 0,
                width: width,
                height: height,
                cornerRadius: cornerRadius
            )
        default:
            return nil
        }
    }

    private func makeOutline(for signature: OutlineSignature) -> SKShapeNode {
        switch signature.shape {
        case .circle:
            return SKShapeNode(circleOfRadius: signature.radius)
        case .rect:
            return SKShapeNode(
                rectOf: CGSize(width: signature.width, height: signature.height),
                cornerRadius: signature.cornerRadius
            )
        }
    }

    private func applyNodeTransform(_ node: SKNode, to outline: SKShapeNode) {
        outline.position = node.position
        outline.zRotation = node.zRotation
        outline.fillColor = .clear
    }

    private struct PersistentOutline {
        let signature: OutlineSignature
        let node: SKShapeNode
    }

    private struct OutlineSignature: Equatable {
        let shape: OutlineShape
        let radius: CGFloat
        let width: CGFloat
        let height: CGFloat
        let cornerRadius: CGFloat
    }

    private enum OutlineShape {
        case circle
        case rect
    }
}

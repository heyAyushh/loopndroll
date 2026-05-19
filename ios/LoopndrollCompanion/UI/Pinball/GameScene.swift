@preconcurrency import SpriteKit
import UIKit

@MainActor
final class GameScene: SKScene, @preconcurrency SKPhysicsContactDelegate {
    private enum Layout {
        static let ballRadius: CGFloat = 15
        static let launchImpulse = CGVector(dx: 0, dy: 7)
    }

    private var motionController: PinballMotionController?
    private var hapticManager = PinballHapticManager()
    private let debugOverlay = PinballDebugOverlay()
    private let surfaceRoot = SKNode()
    private let edgeNode = SKNode()
    private var surfaceNodes: [String: SKNode] = [:]
    private var ball: SKNode?
    private var lastSceneSize: CGSize = .zero
    private var latestSurfaces: [PinballSurface] = []
    private var latestSurfaceIDs = Set<String>()

    var isDebugOverlayEnabled = false {
        didSet {
            debugOverlay.isEnabled = isDebugOverlayEnabled
        }
    }

    override init(size: CGSize) {
        super.init(size: size)
        scaleMode = .resizeFill
        backgroundColor = .clear
    }

    required init?(coder aDecoder: NSCoder) {
        super.init(coder: aDecoder)
        scaleMode = .resizeFill
        backgroundColor = .clear
    }

    override func didMove(to view: SKView) {
        physicsWorld.contactDelegate = self
        physicsWorld.speed = 1.0
        physicsWorld.gravity = CGVector(dx: 0, dy: -9.8)
        anchorPoint = .zero

        surfaceRoot.name = "pinball-rendered-ui-surfaces"
        surfaceRoot.zPosition = 5
        addChild(surfaceRoot)
        addChild(edgeNode)

        motionController = PinballMotionController(scene: self)
        motionController?.start()
        debugOverlay.attach(to: self)
        rebuildWorldIfNeeded(force: true)
    }

    override func didChangeSize(_ oldSize: CGSize) {
        super.didChangeSize(oldSize)
        rebuildWorldIfNeeded(force: true)
    }

    override func update(_ currentTime: TimeInterval) {
        rebuildWorldIfNeeded()
        syncRenderedSurfaces()
        keepBallInPlay()
        debugOverlay.update(in: self)
    }

    func setRenderedSurfaces(_ surfaces: [PinballSurface]) {
        let nextSurfaceIDs = Set(surfaces.map(\.id))
        if nextSurfaceIDs != latestSurfaceIDs {
            debugOverlay.clearInteractionOutlines()
        }

        latestSurfaceIDs = nextSurfaceIDs
        latestSurfaces = surfaces
        syncRenderedSurfaces()
    }

    func applyNudge(_ impulse: CGVector) {
        ball?.physicsBody?.applyImpulse(impulse)
        hapticManager.playCollision(impulse: 10, sharpness: 0.82)
    }

    func prepareFeedback() {
        hapticManager.prepare()
    }

    func suspendFeedback() {
        hapticManager.suspend()
    }

    func didBegin(_ contact: SKPhysicsContact) {
        guard contact.bodyA.categoryBitMask != PinballPhysicsCategory.wall ||
            contact.bodyB.categoryBitMask != PinballPhysicsCategory.wall else {
            return
        }

        hapticManager.playCollision(impulse: contact.collisionImpulse)

        if let node = uiNode(in: contact) {
            debugOverlay.flashInteraction(for: node, impulse: contact.collisionImpulse)
        }
    }

    private func uiNode(in contact: SKPhysicsContact) -> SKNode? {
        if contact.bodyA.categoryBitMask & PinballPhysicsCategory.uiElement != 0 {
            return contact.bodyA.node
        }

        if contact.bodyB.categoryBitMask & PinballPhysicsCategory.uiElement != 0 {
            return contact.bodyB.node
        }

        return nil
    }

    private func rebuildWorldIfNeeded(force: Bool = false) {
        guard size.width > 10, size.height > 10 else {
            return
        }

        guard force || lastSceneSize != size else {
            return
        }

        lastSceneSize = size
        debugOverlay.attach(to: self)
        debugOverlay.isEnabled = isDebugOverlayEnabled
        setupScreenEdges()

        if ball == nil {
            setupBall()
        } else {
            keepBallInPlay()
        }
    }

    private func setupBall() {
        let diameter = Layout.ballRadius * 2
        let node = SKSpriteNode(
            texture: makeBallTexture(),
            size: CGSize(width: diameter, height: diameter)
        )
        node.name = "ball"
        node.position = CGPoint(x: size.width * 0.72, y: size.height * 0.42)
        node.zPosition = 30
        node.setPinballDebugCircle(radius: Layout.ballRadius)

        let body = SKPhysicsBody(circleOfRadius: Layout.ballRadius)
        body.categoryBitMask = PinballPhysicsCategory.ball
        body.collisionBitMask = PinballPhysicsCategory.wall | PinballPhysicsCategory.uiElement
        body.contactTestBitMask = body.collisionBitMask
        body.restitution = 0.9
        body.friction = 0.06
        body.linearDamping = 0.15
        body.angularDamping = 0.22
        body.allowsRotation = true
        node.physicsBody = body
        addChild(node)

        ball = node
        body.applyImpulse(Layout.launchImpulse)
    }

    private func makeBallTexture() -> SKTexture {
        let texture: SKTexture
        if let resourceURL = Bundle.main.url(forResource: "notification-orb", withExtension: "png"),
            let image = UIImage(contentsOfFile: resourceURL.path) {
            texture = SKTexture(image: image)
        } else {
            texture = SKTexture(imageNamed: "notification-orb")
        }

        texture.filteringMode = .linear
        return texture
    }

    private func setupScreenEdges() {
        edgeNode.name = "pinball-screen-edge"
        edgeNode.physicsBody = SKPhysicsBody(edgeLoopFrom: CGRect(origin: .zero, size: size).insetBy(dx: 5, dy: 5))
        edgeNode.physicsBody?.categoryBitMask = PinballPhysicsCategory.wall
        edgeNode.physicsBody?.collisionBitMask = PinballPhysicsCategory.ball
        edgeNode.physicsBody?.contactTestBitMask = PinballPhysicsCategory.ball
        edgeNode.physicsBody?.restitution = 0.78
        edgeNode.physicsBody?.friction = 0.16
    }

    private func syncRenderedSurfaces() {
        guard size != .zero else {
            return
        }

        let activeIDs = Set(latestSurfaces.map(\.id))
        for (id, node) in surfaceNodes where !activeIDs.contains(id) {
            node.removeFromParent()
            surfaceNodes[id] = nil
        }

        for surface in latestSurfaces {
            guard surface.frame.width > 2, surface.frame.height > 2 else {
                continue
            }

            let node = surfaceNodes[surface.id] ?? SKNode()
            node.name = "ui-\(surface.id)"
            if node.parent == nil {
                surfaceRoot.addChild(node)
            }
            surfaceNodes[surface.id] = node
            update(node: node, for: surface)
        }
    }

    private func update(node: SKNode, for surface: PinballSurface) {
        let frame = surface.frame
        let center = CGPoint(x: frame.midX, y: size.height - frame.midY)
        let body = SKPhysicsBody(rectangleOf: frame.size)
        body.isDynamic = false
        body.categoryBitMask = PinballPhysicsCategory.uiElement
        body.collisionBitMask = PinballPhysicsCategory.ball
        body.contactTestBitMask = PinballPhysicsCategory.ball
        body.restitution = surface.material.restitution
        body.friction = surface.material.friction

        node.position = center
        node.zPosition = 8
        node.physicsBody = body
        node.userData = node.userData ?? NSMutableDictionary()
        node.userData?["pinballDebugShape"] = "rect"
        node.userData?["pinballDebugWidth"] = frame.width
        node.userData?["pinballDebugHeight"] = frame.height
        node.userData?["pinballDebugCornerRadius"] = surface.cornerRadius
    }

    func clearInteractionDebug() {
        debugOverlay.clearInteractionOutlines()
    }

    private func keepBallInPlay() {
        guard let ball else {
            return
        }

        if ball.position.y < -60 || ball.position.x < -60 || ball.position.x > size.width + 60 {
            ball.position = CGPoint(x: size.width * 0.5, y: size.height * 0.55)
            ball.physicsBody?.velocity = .zero
            ball.physicsBody?.angularVelocity = 0
            ball.physicsBody?.applyImpulse(Layout.launchImpulse)
        }
    }
}

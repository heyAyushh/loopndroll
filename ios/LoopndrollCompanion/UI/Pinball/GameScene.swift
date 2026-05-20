@preconcurrency import SpriteKit
import UIKit

@MainActor
final class GameScene: SKScene, @preconcurrency SKPhysicsContactDelegate {
    private enum Layout {
        static let ballRadius: CGFloat = 18
        static let launchImpulse = CGVector(dx: 0, dy: 7)
    }

    private enum Drag {
        static let touchSlop: CGFloat = 16
        static let maximumReleaseSpeed: CGFloat = 900
    }

    private var motionController: PinballMotionController?
    private var hapticManager = PinballHapticManager()
    private let debugOverlay = PinballDebugOverlay()
    private let surfaceRoot = SKNode()
    private let edgeNode = SKNode()
    private var surfaceNodes: [String: SKNode] = [:]
    private var ball: SKNode?
    private var draggedTouch: UITouch?
    private var lastDragPoint: CGPoint?
    private var lastDragTimestamp: TimeInterval = 0
    private var releaseVelocity: CGVector = .zero
    private var wasBallDynamicBeforeDrag = true
    private var isSceneActive = false
    private var isShuttingDown = false
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
        debugOverlay.attach(to: self)
        rebuildWorldIfNeeded(force: true)
        setSceneActive(true)
    }

    override func willMove(from view: SKView) {
        shutdown()
    }

    override func didChangeSize(_ oldSize: CGSize) {
        super.didChangeSize(oldSize)
        rebuildWorldIfNeeded(force: true)
    }

    override func update(_ currentTime: TimeInterval) {
        guard isSceneActive, !isShuttingDown else {
            return
        }

        rebuildWorldIfNeeded()
        syncRenderedSurfaces()
        keepBallInPlay()
        debugOverlay.update(in: self)
    }

    func setRenderedSurfaces(_ surfaces: [PinballSurface]) {
        guard !isShuttingDown else {
            return
        }

        let nextSurfaceIDs = Set(surfaces.map(\.id))
        if nextSurfaceIDs != latestSurfaceIDs {
            debugOverlay.clearInteractionOutlines()
        }

        latestSurfaceIDs = nextSurfaceIDs
        latestSurfaces = surfaces
        syncRenderedSurfaces()
    }

    func applyNudge(_ impulse: CGVector) {
        guard isSceneActive, !isShuttingDown else {
            return
        }

        ball?.physicsBody?.applyImpulse(impulse)
        hapticManager.playCollision(impulse: 10, sharpness: 0.82)
    }

    func canBeginBallDrag(atViewPoint point: CGPoint) -> Bool {
        guard isSceneActive, !isShuttingDown else {
            return false
        }

        let scenePoint = convertPoint(fromView: point)
        return isPointInsideBall(scenePoint, hitSlop: Drag.touchSlop)
    }

    func setSceneActive(_ isActive: Bool) {
        guard !isShuttingDown else {
            return
        }

        let wasSceneActive = isSceneActive
        isSceneActive = isActive
        isPaused = !isActive

        if isActive {
            physicsWorld.contactDelegate = self
            if !wasSceneActive {
                motionController?.start()
                hapticManager.prepare()
                rebuildWorldIfNeeded(force: true)
            }
        } else {
            guard wasSceneActive else {
                return
            }

            cancelActiveDrag()
            motionController?.stop()
            hapticManager.suspend()
            debugOverlay.clearInteractionOutlines()
        }
    }

    func shutdown() {
        guard !isShuttingDown else {
            return
        }

        isShuttingDown = true
        isSceneActive = false
        isPaused = true
        cancelActiveDrag()
        motionController?.stop()
        motionController = nil
        hapticManager.suspend()
        physicsWorld.contactDelegate = nil
        physicsWorld.gravity = .zero
        latestSurfaces = []
        latestSurfaceIDs = []
        surfaceNodes.removeAll()
        surfaceRoot.removeAllActions()
        surfaceRoot.removeAllChildren()
        edgeNode.removeAllActions()
        edgeNode.removeFromParent()
        ball?.removeAllActions()
        ball?.physicsBody = nil
        ball?.removeFromParent()
        ball = nil
        debugOverlay.clearInteractionOutlines()
        removeAllActions()
        removeAllChildren()
        lastSceneSize = .zero
    }

    func didBegin(_ contact: SKPhysicsContact) {
        guard isSceneActive, !isShuttingDown else {
            return
        }

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

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard isSceneActive,
              !isShuttingDown,
              draggedTouch == nil,
              let view,
              let touch = touches.first else {
            return
        }

        let point = convertPoint(fromView: touch.location(in: view))
        guard isPointInsideBall(point, hitSlop: Drag.touchSlop) else {
            return
        }

        draggedTouch = touch
        lastDragPoint = point
        lastDragTimestamp = touch.timestamp
        releaseVelocity = .zero

        if let body = ball?.physicsBody {
            wasBallDynamicBeforeDrag = body.isDynamic
            body.isDynamic = false
            body.velocity = .zero
            body.angularVelocity = 0
        }

        ball?.position = constrainedBallPosition(point)
        hapticManager.playCollision(impulse: 8, sharpness: 0.74)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard isSceneActive,
              !isShuttingDown,
              let touch = trackedTouch(in: touches),
              let view else {
            return
        }

        let nextPoint = constrainedBallPosition(convertPoint(fromView: touch.location(in: view)))
        if let lastDragPoint {
            let timeDelta = max(touch.timestamp - lastDragTimestamp, 0.001)
            releaseVelocity = clampedVelocity(
                CGVector(
                    dx: (nextPoint.x - lastDragPoint.x) / timeDelta,
                    dy: (nextPoint.y - lastDragPoint.y) / timeDelta
                )
            )
        }

        ball?.position = nextPoint
        lastDragPoint = nextPoint
        lastDragTimestamp = touch.timestamp
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        finishDragIfNeeded(for: touches)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        finishDragIfNeeded(for: touches)
    }

    private func trackedTouch(in touches: Set<UITouch>) -> UITouch? {
        guard let draggedTouch else {
            return nil
        }

        return touches.first { $0 === draggedTouch }
    }

    private func finishDragIfNeeded(for touches: Set<UITouch>) {
        guard trackedTouch(in: touches) != nil else {
            return
        }

        if let body = ball?.physicsBody {
            body.isDynamic = wasBallDynamicBeforeDrag
            if body.isDynamic {
                body.velocity = releaseVelocity
                body.angularVelocity = -releaseVelocity.dx / max(Layout.ballRadius, 1)
            }
        }

        draggedTouch = nil
        lastDragPoint = nil
        lastDragTimestamp = 0
        releaseVelocity = .zero
        wasBallDynamicBeforeDrag = true
    }

    private func cancelActiveDrag() {
        guard draggedTouch != nil else {
            return
        }

        if let body = ball?.physicsBody {
            body.isDynamic = wasBallDynamicBeforeDrag
            body.velocity = .zero
            body.angularVelocity = 0
        }

        draggedTouch = nil
        lastDragPoint = nil
        lastDragTimestamp = 0
        releaseVelocity = .zero
        wasBallDynamicBeforeDrag = true
    }

    private func isPointInsideBall(_ point: CGPoint, hitSlop: CGFloat = 0) -> Bool {
        guard let ball else {
            return false
        }

        let radius = Layout.ballRadius + hitSlop
        let offsetX = point.x - ball.position.x
        let offsetY = point.y - ball.position.y
        return offsetX * offsetX + offsetY * offsetY <= radius * radius
    }

    private func constrainedBallPosition(_ point: CGPoint) -> CGPoint {
        let inset = Layout.ballRadius + 5
        guard size.width > inset * 2, size.height > inset * 2 else {
            return point
        }

        return CGPoint(
            x: min(max(point.x, inset), size.width - inset),
            y: min(max(point.y, inset), size.height - inset)
        )
    }

    private func clampedVelocity(_ velocity: CGVector) -> CGVector {
        let speed = hypot(velocity.dx, velocity.dy)
        guard speed > Drag.maximumReleaseSpeed else {
            return velocity
        }

        let scale = Drag.maximumReleaseSpeed / speed
        return CGVector(dx: velocity.dx * scale, dy: velocity.dy * scale)
    }

    private func rebuildWorldIfNeeded(force: Bool = false) {
        guard !isShuttingDown else {
            return
        }

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
        node.name = "pinball-ball"
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
        let texture = SKTexture(imageNamed: "PinballBall")
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

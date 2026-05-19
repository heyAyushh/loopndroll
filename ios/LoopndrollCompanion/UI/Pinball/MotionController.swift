@preconcurrency import CoreMotion
@preconcurrency import SpriteKit

@MainActor
final class PinballMotionController {
    private let motionManager = CMMotionManager()
    private weak var scene: GameScene?
    private var lastNudgeTime: TimeInterval = 0

    init(scene: GameScene) {
        self.scene = scene
        motionManager.deviceMotionUpdateInterval = 1.0 / 120.0
    }

    func start() {
        guard motionManager.isDeviceMotionAvailable else {
            scene?.physicsWorld.gravity = CGVector(dx: 0, dy: -9.8)
            return
        }

        motionManager.startDeviceMotionUpdates(to: .main) { [weak self] motion, _ in
            guard let self, let motion, let scene = self.scene else {
                return
            }

            let gravityScale: CGFloat = 34
            scene.physicsWorld.gravity = CGVector(
                dx: motion.gravity.x * gravityScale,
                dy: motion.gravity.y * gravityScale
            )

            let nudgeMagnitude = abs(motion.rotationRate.x) + abs(motion.rotationRate.z)
            let now = ProcessInfo.processInfo.systemUptime
            guard nudgeMagnitude > 3.6, now - lastNudgeTime > 0.55 else {
                return
            }

            scene.applyNudge(
                CGVector(
                    dx: motion.userAcceleration.x * 170,
                    dy: motion.userAcceleration.y * 170
                )
            )
            lastNudgeTime = now
        }
    }

    func stop() {
        motionManager.stopDeviceMotionUpdates()
    }

}

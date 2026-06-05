import CoreHaptics
import Foundation

final class PinballHapticManager {
    private var engine: CHHapticEngine?
    private var isEngineReady = false
    private var lastCollisionTime: TimeInterval = 0

    init() {
        prepareEngine()
    }

    func playCollision(impulse: CGFloat, sharpness: Float = 0.58) {
        prepare()

        guard isEngineReady, let engine else {
            return
        }

        let now = ProcessInfo.processInfo.systemUptime
        guard now - lastCollisionTime > 0.045 else {
            return
        }
        lastCollisionTime = now

        let intensity = Float(min(max(impulse / 18.0, 0.12), 1.0))
        let event = CHHapticEvent(
            eventType: .hapticTransient,
            parameters: [
                CHHapticEventParameter(parameterID: .hapticIntensity, value: intensity),
                CHHapticEventParameter(parameterID: .hapticSharpness, value: sharpness),
            ],
            relativeTime: 0
        )

        do {
            let pattern = try CHHapticPattern(events: [event], parameters: [])
            let player = try engine.makePlayer(with: pattern)
            try player.start(atTime: 0)
        } catch {
            isEngineReady = false
            prepareEngine()
        }
    }

    func playFlipper() {
        playCollision(impulse: 8, sharpness: 0.72)
    }

    func prepare() {
        if engine == nil {
            prepareEngine()
        } else if !isEngineReady {
            startEngine()
        }
    }

    func suspend() {
        engine?.stop()
        isEngineReady = false
    }

    private func prepareEngine() {
        guard CHHapticEngine.capabilitiesForHardware().supportsHaptics else {
            return
        }

        do {
            let engine = try CHHapticEngine()
            engine.resetHandler = { [weak self] in
                self?.prepare()
            }
            engine.stoppedHandler = { [weak self] _ in
                self?.isEngineReady = false
            }

            self.engine = engine
            startEngine()
        } catch {
            engine = nil
            isEngineReady = false
        }
    }

    private func startEngine() {
        guard let engine else {
            return
        }

        do {
            try engine.start()
            isEngineReady = true
        } catch {
            isEngineReady = false
        }
    }

    deinit {
        engine?.stop()
    }
}

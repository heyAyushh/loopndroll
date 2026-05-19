@preconcurrency import SpriteKit
import SwiftUI
import UIKit

struct PinballGameView: UIViewRepresentable {
    var isEnabled: Bool
    var showsDebugOverlay: Bool
    var surfaces: [PinballSurface]

    func makeUIView(context: Context) -> SKView {
        let view = PinballSKView(frame: .zero)
        view.backgroundColor = .clear
        view.allowsTransparency = true
        view.ignoresSiblingOrder = true
        view.preferredFramesPerSecond = 120
        view.isMultipleTouchEnabled = true

        let scene = GameScene(size: UIScreen.main.bounds.size)
        scene.isDebugOverlayEnabled = showsDebugOverlay
        view.presentScene(scene)
        context.coordinator.scene = scene
        context.coordinator.startKeyboardObserving()
        return view
    }

    func updateUIView(_ view: SKView, context: Context) {
        if let scene = context.coordinator.scene {
            scene.size = view.bounds.size
            scene.isPaused = !isEnabled
            scene.isDebugOverlayEnabled = showsDebugOverlay
            context.coordinator.setBaseSurfaces(surfaces)
            if isEnabled {
                scene.prepareFeedback()
            } else {
                scene.suspendFeedback()
                scene.clearInteractionDebug()
            }
        }

        view.isHidden = !isEnabled
        view.isUserInteractionEnabled = false
        view.showsPhysics = showsDebugOverlay
    }

    static func dismantleUIView(_ view: SKView, coordinator: Coordinator) {
        coordinator.stopKeyboardObserving()
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    @MainActor
    final class Coordinator {
        private enum KeyboardSurface {
            static let id = "system.keyboard"
            static let cornerRadius: CGFloat = 18
            static let hiddenInset: CGFloat = 2
        }

        var scene: GameScene?
        private var baseSurfaces: [PinballSurface] = []
        private var keyboardSurface: PinballSurface?
        private var keyboardObservers: [NSObjectProtocol] = []

        func setBaseSurfaces(_ surfaces: [PinballSurface]) {
            baseSurfaces = surfaces
            applySurfaces()
        }

        func startKeyboardObserving() {
            guard keyboardObservers.isEmpty else {
                return
            }

            let center = NotificationCenter.default
            keyboardObservers = [
                center.addObserver(
                    forName: UIResponder.keyboardWillChangeFrameNotification,
                    object: nil,
                    queue: .main
                ) { [weak self] notification in
                    let keyboardFrame = (
                        notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? NSValue
                    )?.cgRectValue
                    Task { @MainActor in
                        self?.updateKeyboardSurface(frame: keyboardFrame)
                    }
                },
                center.addObserver(
                    forName: UIResponder.keyboardWillHideNotification,
                    object: nil,
                    queue: .main
                ) { [weak self] _ in
                    Task { @MainActor in
                        self?.keyboardSurface = nil
                        self?.applySurfaces()
                    }
                },
            ]
        }

        func stopKeyboardObserving() {
            let center = NotificationCenter.default
            for observer in keyboardObservers {
                center.removeObserver(observer)
            }
            keyboardObservers = []
        }

        private func updateKeyboardSurface(frame keyboardFrame: CGRect?) {
            guard let keyboardFrame,
                  let screenBounds = scene?.view?.window?.screen.bounds ?? scene?.view?.window?.bounds else {
                keyboardSurface = nil
                applySurfaces()
                return
            }

            let visibleFrame = keyboardFrame.intersection(screenBounds)
            guard visibleFrame.height > KeyboardSurface.hiddenInset, visibleFrame.width > KeyboardSurface.hiddenInset else {
                keyboardSurface = nil
                applySurfaces()
                return
            }

            keyboardSurface = PinballSurface(
                id: KeyboardSurface.id,
                frame: visibleFrame,
                cornerRadius: KeyboardSurface.cornerRadius,
                material: .soft
            )
            applySurfaces()
        }

        private func applySurfaces() {
            var surfaces = baseSurfaces
            if let keyboardSurface {
                surfaces.append(keyboardSurface)
            }
            scene?.setRenderedSurfaces(surfaces)
        }
    }
}

private final class PinballSKView: SKView {
    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        false
    }
}

enum PinballSurfaceMaterial: Equatable {
    case glass
    case soft
    case metal

    var restitution: CGFloat {
        switch self {
        case .glass:
            return 0.82
        case .soft:
            return 0.58
        case .metal:
            return 0.95
        }
    }

    var friction: CGFloat {
        switch self {
        case .glass:
            return 0.16
        case .soft:
            return 0.32
        case .metal:
            return 0.08
        }
    }
}

struct PinballSurface: Equatable, Identifiable {
    let id: String
    let frame: CGRect
    let cornerRadius: CGFloat
    let material: PinballSurfaceMaterial
}

private struct PinballSurfacePreferenceKey: PreferenceKey {
    static let defaultValue: [PinballSurface] = []

    static func reduce(value: inout [PinballSurface], nextValue: () -> [PinballSurface]) {
        value.append(contentsOf: nextValue())
    }
}

private struct PinballSurfaceModifier: ViewModifier {
    @Environment(\.pinballSurfaceCollectionEnabled) private var isCollectionEnabled
    @State private var surfaceID = UUID().uuidString

    let explicitID: String?
    let cornerRadius: CGFloat
    let material: PinballSurfaceMaterial

    func body(content: Content) -> some View {
        content.background {
            GeometryReader { proxy in
                Color.clear.preference(
                    key: PinballSurfacePreferenceKey.self,
                    value: isCollectionEnabled ? [
                        PinballSurface(
                            id: explicitID ?? surfaceID,
                            frame: proxy.frame(in: .global),
                            cornerRadius: cornerRadius,
                            material: material
                        ),
                    ] : []
                )
            }
        }
    }
}

private struct PinballSurfaceCollector: ViewModifier {
    @Binding var surfaces: [PinballSurface]

    func body(content: Content) -> some View {
        content.onPreferenceChange(PinballSurfacePreferenceKey.self) { nextSurfaces in
            surfaces = PinballSurfaceNormalizer.normalized(nextSurfaces)
        }
    }
}

private enum PinballSurfaceNormalizer {
    static func normalized(_ surfaces: [PinballSurface]) -> [PinballSurface] {
        var latest: [String: PinballSurface] = [:]
        for surface in surfaces where isUsable(surface.frame) {
            latest[surface.id] = surface
        }
        return latest.values.sorted { $0.id < $1.id }
    }

    private static func isUsable(_ frame: CGRect) -> Bool {
        frame.width > 2 && frame.height > 2 && !frame.isNull && !frame.isInfinite
    }
}

extension View {
    func pinballSurface(
        id: String? = nil,
        cornerRadius: CGFloat = CompanionMetrics.compactCornerRadius,
        material: PinballSurfaceMaterial = .glass
    ) -> some View {
        modifier(
            PinballSurfaceModifier(
                explicitID: id,
                cornerRadius: cornerRadius,
                material: material
            )
        )
    }

    func collectPinballSurfaces(_ surfaces: Binding<[PinballSurface]>) -> some View {
        modifier(PinballSurfaceCollector(surfaces: surfaces))
    }

    func pinballSurfaceCollectionEnabled(_ isEnabled: Bool) -> some View {
        environment(\.pinballSurfaceCollectionEnabled, isEnabled)
    }
}

private struct PinballSurfaceCollectionEnabledKey: EnvironmentKey {
    static let defaultValue = true
}

private extension EnvironmentValues {
    var pinballSurfaceCollectionEnabled: Bool {
        get { self[PinballSurfaceCollectionEnabledKey.self] }
        set { self[PinballSurfaceCollectionEnabledKey.self] = newValue }
    }
}

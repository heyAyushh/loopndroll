import Foundation

public enum LooperHumanStatusKind: Equatable, Sendable {
    case starting
    case ready
    case needsAttention
    case unavailable
}

public struct LooperHumanStatus: Equatable, Sendable {
    private static let healthyValue = "healthy"

    public let kind: LooperHumanStatusKind
    public let title: String
    public let detail: String
    public let lifecycle: String

    public static func starting(detachOnQuit: Bool) -> Self {
        Self(
            kind: .starting,
            title: "Starting",
            detail: "Local server is starting",
            lifecycle: lifecycleText(detachOnQuit: detachOnQuit)
        )
    }

    public static func unavailable(detachOnQuit: Bool) -> Self {
        Self(
            kind: .unavailable,
            title: "Unavailable",
            detail: "Local server is unavailable",
            lifecycle: lifecycleText(detachOnQuit: detachOnQuit)
        )
    }

    public static func from(
        snapshot: DesktopSnapshotResponse,
        mobileHealth: MobileHealthResponse?,
        detachOnQuit: Bool
    ) -> Self {
        let sourceHealthy = snapshot.controlPlane.source.health == healthyValue
        let hooksHealthy = snapshot.controlPlane.hooks.health == healthyValue
        let mobileReady = mobileHealth?.ok == true && mobileHealth?.requiresAuthentication == true
        let detail = [
            "source=\(snapshot.controlPlane.source.health)",
            "hooks=\(snapshot.controlPlane.hooks.health)",
            "iPhone=\(mobileReady ? "ready" : "unknown")",
        ].joined(separator: " ")

        return Self(
            kind: sourceHealthy && hooksHealthy ? .ready : .needsAttention,
            title: sourceHealthy && hooksHealthy ? "Ready" : "Needs attention",
            detail: detail,
            lifecycle: lifecycleText(detachOnQuit: detachOnQuit)
        )
    }

    private static func lifecycleText(detachOnQuit: Bool) -> String {
        detachOnQuit ? "Detached on quit" : "Quit stops server"
    }
}

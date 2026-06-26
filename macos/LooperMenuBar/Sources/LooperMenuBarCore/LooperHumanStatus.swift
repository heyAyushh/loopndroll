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
        let codexHooksHealthy = snapshot.controlPlane.hooks.health == healthyValue
        let grokHooksHealthy = snapshot.grokBuild.map { $0.hooks.health == healthyValue } ?? true
        let mobileReady = mobileHealth?.ok == true && mobileHealth?.requiresAuthentication == true
        let detail = [
            "source=\(snapshot.controlPlane.source.health)",
            "codexHooks=\(snapshot.controlPlane.hooks.health)",
            "grokHooks=\(snapshot.grokBuild?.hooks.health ?? "unknown")",
            "iPhone=\(mobileReady ? "ready" : "unknown")",
        ].joined(separator: " ")

        let isReady = sourceHealthy && codexHooksHealthy && grokHooksHealthy

        return Self(
            kind: isReady ? .ready : .needsAttention,
            title: isReady ? "Ready" : "Needs attention",
            detail: detail,
            lifecycle: lifecycleText(detachOnQuit: detachOnQuit)
        )
    }

    public static func from(
        sessionMiniSnapshot snapshot: MenuBarSessionMiniLocalSnapshot,
        mobileHealth: MobileHealthResponse?,
        detachOnQuit: Bool
    ) -> Self {
        let activeSessions = snapshot.sessions.filter { !$0.isArchived }
        let blockedCount = activeSessions.filter { $0.blockedGoal != nil }.count
        let replyableCount = activeSessions.filter(\.replyable).count
        let pendingCount = snapshot.pendingCommands.count
        let mobileReady = mobileHealth?.ok == true && mobileHealth?.requiresAuthentication == true
        let detail = [
            "source=sessionMini",
            "seq=\(snapshot.latestSeq)",
            "active=\(activeSessions.count)",
            "replyable=\(replyableCount)",
            "blocked=\(blockedCount)",
            "pending=\(pendingCount)",
            "iPhone=\(mobileReady ? "ready" : "unknown")",
        ].joined(separator: " ")

        let needsAttention = blockedCount > 0

        return Self(
            kind: needsAttention ? .needsAttention : .ready,
            title: needsAttention ? "Needs attention" : "Realtime",
            detail: detail,
            lifecycle: lifecycleText(detachOnQuit: detachOnQuit)
        )
    }

    private static func lifecycleText(detachOnQuit: Bool) -> String {
        detachOnQuit ? "Detached on quit" : "Quit stops server"
    }
}

import Foundation

public struct LooperMenuSection: Equatable, Sendable {
    public let title: String
    public let rows: [LooperMenuRow]
}

public struct LooperMenuRow: Equatable, Sendable {
    public let threadId: String
    public let title: String
    public let subtitle: String
    public let archived: Bool
    public let openTarget: LooperThreadOpenTarget
    public let effectiveMode: String?
    public let replyable: Bool?
    public let blockedGoalTitle: String?
    public let queueCount: Int?
    public let lifecycle: String?
    public let notificationTitle: String?

    public init(
        threadId: String,
        title: String,
        subtitle: String,
        archived: Bool,
        openTarget: LooperThreadOpenTarget,
        effectiveMode: String? = nil,
        replyable: Bool? = nil,
        blockedGoalTitle: String? = nil,
        queueCount: Int? = nil,
        lifecycle: String? = nil,
        notificationTitle: String? = nil
    ) {
        self.threadId = threadId
        self.title = title
        self.subtitle = subtitle
        self.archived = archived
        self.openTarget = openTarget
        self.effectiveMode = effectiveMode
        self.replyable = replyable
        self.blockedGoalTitle = blockedGoalTitle
        self.queueCount = queueCount
        self.lifecycle = lifecycle
        self.notificationTitle = notificationTitle
    }
}

public struct LooperAcpTargetRow: Equatable, Sendable {
    public let id: String
    public let title: String
    public let subtitle: String
    public let detail: String
    public let ready: Bool
}

public enum LooperMenuContent {
    private enum AcpTargetText {
        static let none = "None"
        static let ready = "Ready"
        static let readySummary = "ready"
        static let readOnly = "Read-only"
        static let readOnlySummary = "read-only"
        static let blocked = "Blocked"
        static let blockedSummary = "blocked"
        static let missingLaunchMetadata = "no launch metadata"
    }

    private enum SurfaceLabel {
        static let tailscale = "Tailscale"
        static let zedACP = "Zed ACP"
    }

    public static func buildThreadSections(from threads: [DesktopThreadSummary]) -> [LooperMenuSection] {
        let activeRows = threads
            .filter { !$0.archived }
            .map(makeThreadRow)
        let archivedRows = threads
            .filter(\.archived)
            .map(makeThreadRow)

        return [
            activeRows.isEmpty ? nil : LooperMenuSection(title: "Active Chats", rows: activeRows),
            archivedRows.isEmpty ? nil : LooperMenuSection(title: "Archived Chats", rows: archivedRows),
        ]
        .compactMap { $0 }
    }

    public static func buildThreadSections(from minis: [MenuBarSessionMini]) -> [LooperMenuSection] {
        let activeRows = minis
            .filter { !$0.isArchived }
            .map(makeThreadRow)
        let archivedRows = minis
            .filter(\.isArchived)
            .map(makeThreadRow)

        return [
            activeRows.isEmpty ? nil : LooperMenuSection(title: "Active Chats", rows: activeRows),
            archivedRows.isEmpty ? nil : LooperMenuSection(title: "Archived Chats", rows: archivedRows),
        ]
        .compactMap { $0 }
    }

    public static func acpTargetStatusTitle(from targets: [AcpTargetSummary]) -> String {
        guard !targets.isEmpty else {
            return AcpTargetText.none
        }

        let readyCount = targets.filter(\.ready).count
        let readOnlyCount = targets.filter(isReadOnlyTarget).count
        let blockedCount = max(targets.count - readyCount - readOnlyCount, 0)
        guard readyCount != targets.count else {
            return "\(readyCount) \(AcpTargetText.readySummary)"
        }

        guard readOnlyCount != targets.count else {
            return "\(readOnlyCount) \(AcpTargetText.readOnlySummary)"
        }

        return [
            readyCount > 0 ? "\(readyCount) \(AcpTargetText.readySummary)" : nil,
            readOnlyCount > 0 ? "\(readOnlyCount) \(AcpTargetText.readOnlySummary)" : nil,
            blockedCount > 0 ? "\(blockedCount) \(AcpTargetText.blockedSummary)" : nil,
        ]
        .compactMap { $0 }
        .joined(separator: " / ")
    }

    public static func buildAcpTargetRows(from targets: [AcpTargetSummary]) -> [LooperAcpTargetRow] {
        targets.map(makeAcpTargetRow)
    }

    private static func makeThreadRow(_ thread: DesktopThreadSummary) -> LooperMenuRow {
        LooperMenuRow(
            threadId: thread.threadId,
            title: DesktopThreadDisplayText.title(for: thread),
            subtitle: subtitleText(for: thread),
            archived: thread.archived,
            openTarget: LooperThreadOpenTarget(
                threadId: thread.threadId,
                transcriptPath: thread.transcriptPath,
                workingDirectory: thread.cwd,
                agentPath: thread.capabilities.agentPath
            )
        )
    }

    private static func makeThreadRow(_ mini: MenuBarSessionMini) -> LooperMenuRow {
        LooperMenuRow(
            threadId: mini.sessionID,
            title: mini.title,
            subtitle: subtitleText(for: mini),
            archived: mini.isArchived,
            openTarget: LooperThreadOpenTarget(
                threadId: mini.sessionID,
                transcriptPath: nil,
                workingDirectory: mini.projectPath
            ),
            effectiveMode: mini.effectiveMode,
            replyable: mini.replyable,
            blockedGoalTitle: mini.blockedGoal?.title,
            queueCount: mini.queueCount,
            lifecycle: mini.lifecycle,
            notificationTitle: notificationText(for: mini.notificationStatus)
        )
    }

    private static func makeAcpTargetRow(_ target: AcpTargetSummary) -> LooperAcpTargetRow {
        LooperAcpTargetRow(
            id: target.id,
            title: "\(target.clientName): \(acpTargetDisplayName(target))",
            subtitle: "\(acpReadinessText(target)) - \(launchMethodText(target.launch))",
            detail: target.detail,
            ready: target.ready
        )
    }

    private static func acpTargetDisplayName(_ target: AcpTargetSummary) -> String {
        let name = target.name.trimmingCharacters(in: .whitespacesAndNewlines)
        if !name.isEmpty {
            return name
        }

        let agentId = target.agentId.trimmingCharacters(in: .whitespacesAndNewlines)
        if !agentId.isEmpty {
            return agentId
        }

        return target.id
    }

    private static func acpReadinessText(_ target: AcpTargetSummary) -> String {
        if isReadOnlyTarget(target) {
            return AcpTargetText.readOnly
        }

        return target.ready ? AcpTargetText.ready : AcpTargetText.blocked
    }

    private static func isReadOnlyTarget(_ target: AcpTargetSummary) -> Bool {
        target.status == "read-only"
    }

    private static func launchMethodText(_ launch: AcpLaunchMetadataSummary) -> String {
        guard !launch.methods.isEmpty else {
            return AcpTargetText.missingLaunchMetadata
        }
        return launch.methods.joined(separator: ", ")
    }

    private static func subtitleText(for thread: DesktopThreadSummary) -> String {
        let projectName = DesktopThreadDisplayText.projectName(for: thread)
        let subtitle = (
            sourceLabels(for: thread)
            + [projectName]
        )
        .joined(separator: " - ")
        return thread.archived ? "Archived - \(subtitle)" : subtitle
    }

    private static func subtitleText(for mini: MenuBarSessionMini) -> String {
        let subtitle = [
            modeText(for: mini.effectiveMode),
            mini.replyable ? "Reply ready" : mini.promptUnavailableReason,
            blockedGoalText(for: mini.blockedGoal),
            queueText(for: mini.queueCount),
            lifecycleText(for: mini.lifecycle),
            notificationText(for: mini.notificationStatus),
            mini.projectName,
        ]
        .compactMap { $0?.trimmingCharacters(in: .whitespacesAndNewlines) }
        .filter { !$0.isEmpty }
        .joined(separator: " - ")
        let fallback = subtitle.isEmpty ? mini.assistantSurface : subtitle
        return mini.isArchived ? "Archived - \(fallback)" : fallback
    }

    private static func modeText(for mode: String?) -> String? {
        guard let mode else {
            return nil
        }

        switch mode {
        case "infinite":
            return "Infinite"
        case "await-reply":
            return "Await Reply"
        case "completion-checks":
            return "Completion Checks"
        case "max-turns-1":
            return "Max Turns 1"
        case "max-turns-2":
            return "Max Turns 2"
        case "max-turns-3":
            return "Max Turns 3"
        default:
            return mode
        }
    }

    private static func blockedGoalText(for goal: MenuBarSessionMiniBlockedGoal?) -> String? {
        guard let goal else {
            return nil
        }
        let title = goal.title?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let title, !title.isEmpty {
            return "Blocked: \(title)"
        }
        let reason = goal.reason ?? goal.status
        return reason.map { "Blocked: \($0)" }
    }

    private static func queueText(for queueCount: Int) -> String? {
        guard queueCount > 0 else {
            return nil
        }
        return "Queue \(queueCount)"
    }

    private static func lifecycleText(for lifecycle: String?) -> String? {
        guard let lifecycle = lifecycle?.trimmingCharacters(in: .whitespacesAndNewlines),
              !lifecycle.isEmpty
        else {
            return nil
        }
        return "State \(lifecycle)"
    }

    private static func notificationText(
        for status: MenuBarSessionMiniNotificationStatus?
    ) -> String? {
        guard let status else {
            return nil
        }
        guard status.enabled else {
            return "Notify off"
        }
        guard !status.targetIds.isEmpty else {
            return "Notify ready"
        }
        let targets = status.targetIds.joined(separator: "/")
        return "Notify \(targets)"
    }

    private static func sourceLabels(for thread: DesktopThreadSummary) -> [String] {
        let fields = [
            thread.title,
            thread.source,
            thread.capabilities.agentNickname,
            thread.capabilities.agentRole,
            thread.capabilities.agentPath,
        ]

        return [
            fields.contains(where: containsZedACPReference) ? SurfaceLabel.zedACP : nil,
            fields.contains(where: containsTailscaleReference) ? SurfaceLabel.tailscale : nil,
        ]
        .compactMap { $0 }
    }

    private static func containsZedACPReference(_ value: String?) -> Bool {
        guard let value else {
            return false
        }

        let normalized = value
            .lowercased()
            .replacingOccurrences(of: "-", with: " ")
            .replacingOccurrences(of: "_", with: " ")
        return normalized.contains("zed acp") || normalized.contains("zed.dev")
    }

    private static func containsTailscaleReference(_ value: String?) -> Bool {
        guard let value else {
            return false
        }

        return TailscaleNetworkPattern.containsTailscaleReference(in: value)
    }
}

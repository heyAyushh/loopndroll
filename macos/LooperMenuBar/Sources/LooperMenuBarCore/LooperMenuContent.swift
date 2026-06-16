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
        static let blocked = "Blocked"
        static let missingLaunchMetadata = "no launch metadata"
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

    public static func acpTargetStatusTitle(from targets: [AcpTargetSummary]) -> String {
        guard !targets.isEmpty else {
            return AcpTargetText.none
        }

        let readyCount = targets.filter(\.ready).count
        guard readyCount != targets.count else {
            return "\(readyCount) ready"
        }

        return "\(readyCount)/\(targets.count) ready"
    }

    public static func buildAcpTargetRows(from targets: [AcpTargetSummary]) -> [LooperAcpTargetRow] {
        targets.map(makeAcpTargetRow)
    }

    private static func makeThreadRow(_ thread: DesktopThreadSummary) -> LooperMenuRow {
        LooperMenuRow(
            threadId: thread.threadId,
            title: titleText(for: thread),
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
        target.ready ? AcpTargetText.ready : AcpTargetText.blocked
    }

    private static func launchMethodText(_ launch: AcpLaunchMetadataSummary) -> String {
        guard !launch.methods.isEmpty else {
            return AcpTargetText.missingLaunchMetadata
        }
        return launch.methods.joined(separator: ", ")
    }

    private static func titleText(for thread: DesktopThreadSummary) -> String {
        let title = thread.title?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let title, !title.isEmpty {
            return title
        }
        return thread.threadId
    }

    private static func subtitleText(for thread: DesktopThreadSummary) -> String {
        let fallback = thread.source ?? thread.capabilities.spawn.launchKind
        let projectName = ProjectNameResolver.displayName(
            forWorkingDirectory: thread.cwd,
            fallback: fallback
        )
        return thread.archived ? "Archived - \(projectName)" : projectName
    }
}

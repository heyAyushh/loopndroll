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

public enum LooperMenuContent {
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

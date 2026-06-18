import Foundation

enum DesktopThreadDisplayText {
    static func title(for thread: DesktopThreadSummary) -> String {
        let title = thread.title?.trimmingCharacters(in: .whitespacesAndNewlines)
        if let title, !title.isEmpty {
            return title
        }
        return thread.threadId
    }

    static func projectName(for thread: DesktopThreadSummary) -> String {
        ProjectNameResolver.displayName(
            forWorkingDirectory: thread.cwd,
            fallback: projectFallback(for: thread)
        )
    }

    private static func projectFallback(for thread: DesktopThreadSummary) -> String {
        thread.source ?? thread.capabilities.spawn.launchKind
    }
}

import Foundation
import Testing
@testable import LooperMenuBarCore

struct ProjectNameResolverTests {
    @Test
    func findsNearestGitProjectName() throws {
        let parentProjectURL = try GitProjectFixture.makeProject(name: "parent-project")
        let childProjectURL = parentProjectURL.appendingPathComponent("child-project", isDirectory: true)
        let nestedWorkingDirectory = childProjectURL.appendingPathComponent("src", isDirectory: true)

        try FileManager.default.createDirectory(
            at: childProjectURL.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        try FileManager.default.createDirectory(at: nestedWorkingDirectory, withIntermediateDirectories: true)

        let projectName = ProjectNameResolver.displayName(
            forWorkingDirectory: nestedWorkingDirectory.path,
            fallback: "fallback"
        )

        #expect(projectName == "child-project")
    }

    @Test
    func fallsBackToDirectoryNameOutsideGitProject() throws {
        let workingDirectory = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent("scratch-task", isDirectory: true)

        try FileManager.default.createDirectory(at: workingDirectory, withIntermediateDirectories: true)

        let projectName = ProjectNameResolver.displayName(
            forWorkingDirectory: workingDirectory.path,
            fallback: "fallback"
        )

        #expect(projectName == "scratch-task")
    }
}

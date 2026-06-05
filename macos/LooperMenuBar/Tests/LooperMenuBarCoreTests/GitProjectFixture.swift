import Foundation

enum GitProjectFixture {
    static func makeProject(name: String) throws -> URL {
        let projectURL = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
            .appendingPathComponent(name, isDirectory: true)
        try FileManager.default.createDirectory(
            at: projectURL.appendingPathComponent(".git", isDirectory: true),
            withIntermediateDirectories: true
        )
        return projectURL
    }
}

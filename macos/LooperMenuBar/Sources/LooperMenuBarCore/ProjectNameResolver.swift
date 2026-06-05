import Foundation

private let GIT_METADATA_DIRECTORY_NAME = ".git"

public enum ProjectNameResolver {
    public static func displayName(
        forWorkingDirectory workingDirectory: String?,
        fallback: String,
        fileManager: FileManager = .default
    ) -> String {
        gitProjectName(forWorkingDirectory: workingDirectory, fileManager: fileManager)
            ?? directoryName(forWorkingDirectory: workingDirectory)
            ?? fallback
    }

    public static func gitProjectName(
        forWorkingDirectory workingDirectory: String?,
        fileManager: FileManager = .default
    ) -> String? {
        guard let directoryURL = directoryURL(from: workingDirectory) else {
            return nil
        }

        return nearestGitRoot(startingAt: directoryURL, fileManager: fileManager)?.lastPathComponent
    }

    public static func directoryName(forWorkingDirectory workingDirectory: String?) -> String? {
        directoryURL(from: workingDirectory)?.lastPathComponent
    }

    private static func nearestGitRoot(startingAt directoryURL: URL, fileManager: FileManager) -> URL? {
        var candidateURL = directoryURL

        while true {
            if hasGitMetadata(at: candidateURL, fileManager: fileManager) {
                return candidateURL
            }

            let parentURL = candidateURL.deletingLastPathComponent()
            if parentURL.path == candidateURL.path {
                return nil
            }

            candidateURL = parentURL
        }
    }

    private static func hasGitMetadata(at directoryURL: URL, fileManager: FileManager) -> Bool {
        let gitMetadataURL = directoryURL.appendingPathComponent(GIT_METADATA_DIRECTORY_NAME)
        return fileManager.fileExists(atPath: gitMetadataURL.path)
    }

    private static func directoryURL(from workingDirectory: String?) -> URL? {
        guard let workingDirectory else {
            return nil
        }

        let trimmedDirectory = workingDirectory.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedDirectory.isEmpty else {
            return nil
        }

        return URL(fileURLWithPath: trimmedDirectory, isDirectory: true).standardizedFileURL
    }
}

import Foundation

enum CompanionBaseURLIdentity {
    private static let pathSeparator = "/"
    private static let pathTrimCharacters = CharacterSet(charactersIn: pathSeparator)

    static func key(for baseURL: URL) -> String {
        guard var components = URLComponents(url: baseURL, resolvingAgainstBaseURL: false) else {
            return baseURL.absoluteString
        }

        components.scheme = components.scheme?.lowercased()
        components.host = components.host?.lowercased()
        components.percentEncodedPath = normalizedPath(components.percentEncodedPath)
        components.percentEncodedQuery = nil
        components.fragment = nil
        return components.string ?? baseURL.absoluteString
    }

    static func unique(_ baseURLs: [URL], including shouldInclude: (URL) -> Bool = { _ in true }) -> [URL] {
        var seen = Set<String>()
        return baseURLs.filter { baseURL in
            shouldInclude(baseURL) && seen.insert(key(for: baseURL)).inserted
        }
    }

    private static func normalizedPath(_ path: String) -> String {
        let trimmedPath = path.trimmingCharacters(in: pathTrimCharacters)
        guard !trimmedPath.isEmpty else {
            return ""
        }

        return "\(pathSeparator)\(trimmedPath)"
    }
}

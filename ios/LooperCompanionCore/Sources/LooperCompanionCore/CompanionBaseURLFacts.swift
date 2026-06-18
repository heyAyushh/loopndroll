import Foundation

enum CompanionBaseURLFacts {
    static let httpScheme = "http"
    static let httpsScheme = "https"
    static let httpDefaultPort = 80
    static let httpsDefaultPort = 443
}

extension URL {
    var companionNormalizedHost: String? {
        guard let host = host?.lowercased().trimmingCharacters(in: .whitespacesAndNewlines),
              !host.isEmpty
        else {
            return nil
        }

        return host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
    }

    var companionNormalizedServerPort: Int? {
        if let port {
            return port
        }

        switch companionLowercasedScheme {
        case CompanionBaseURLFacts.httpScheme:
            return CompanionBaseURLFacts.httpDefaultPort
        case CompanionBaseURLFacts.httpsScheme:
            return CompanionBaseURLFacts.httpsDefaultPort
        default:
            return nil
        }
    }

    var usesCompanionHTTP: Bool {
        companionLowercasedScheme == CompanionBaseURLFacts.httpScheme
    }

    var usesCompanionHTTPS: Bool {
        companionLowercasedScheme == CompanionBaseURLFacts.httpsScheme
    }

    var usesSupportedCompanionHTTPScheme: Bool {
        usesCompanionHTTP || usesCompanionHTTPS
    }

    private var companionLowercasedScheme: String? {
        scheme?.lowercased()
    }
}

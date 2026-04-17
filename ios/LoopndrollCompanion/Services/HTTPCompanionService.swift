import Foundation

struct HTTPCompanionService: CompanionService {
    let baseURL: URL

    func loadSnapshot() async throws -> MobileSnapshot {
        try await request(path: "/api/mobile/snapshot", method: "GET")
    }

    func loadSessionDetail(id: String) async throws -> SessionDetail {
        try await request(path: "/api/mobile/sessions/\(id)", method: "GET")
    }

    func setSessionMode(id: String, preset: SessionMode?) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/sessions/\(id)/mode",
            method: "POST",
            body: ["preset": preset?.rawValue ?? NSNull()]
        )
    }

    func setSessionArchived(id: String, archived: Bool) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/sessions/\(id)/archive",
            method: "POST",
            body: ["archived": archived]
        )
    }

    func deleteSession(id: String) async throws -> MobileSnapshot {
        try await request(path: "/api/mobile/sessions/\(id)", method: "DELETE")
    }

    func saveDefaultPrompt(_ prompt: String) async throws -> MobileSnapshot {
        try await request(
            path: "/api/mobile/settings/default-prompt",
            method: "POST",
            body: ["defaultPrompt": prompt]
        )
    }

    private func request<Response: Decodable>(
        path: String,
        method: String,
        body: [String: Any]? = nil
    ) async throws -> Response {
        var request = URLRequest(url: baseURL.appending(path: path))
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")

        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        guard (200..<300).contains(httpResponse.statusCode) else {
            let serverMessage = String(data: data, encoding: .utf8) ?? "Request failed."
            throw HTTPCompanionServiceError.serverError(serverMessage)
        }

        let decoder = JSONDecoder()
        return try decoder.decode(Response.self, from: data)
    }
}

enum HTTPCompanionServiceError: LocalizedError {
    case invalidResponse
    case serverError(String)

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "The Loopndroll API returned an invalid response."
        case let .serverError(message):
            return message
        }
    }
}

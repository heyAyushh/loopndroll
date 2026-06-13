import Foundation
import LooperCompanionCore

struct CompanionPasskeyChallenge: Decodable, Sendable {
    let challengeId: String
    let challenge: String
    let message: String
}

struct CompanionPasskeyRegistration: Decodable, Sendable {
    let credentialId: String
    let session: CompanionMobileSession
}

struct CompanionPasskeyAuthentication: Decodable, Sendable {
    let ok: Bool
    let credentialId: String
    let session: CompanionMobileSession
}

protocol CompanionPasskeyAuthenticationClient: Sendable {
    func issueRegistrationChallenge() async throws -> CompanionPasskeyChallenge
    func completeRegistration(
        challengeID: String,
        publicKeyX963: String,
        signature: String
    ) async throws -> CompanionPasskeyRegistration
    func issueAuthenticationChallenge(credentialID: String) async throws -> CompanionPasskeyChallenge
    func completeAuthentication(
        credentialID: String,
        challengeID: String,
        signature: String
    ) async throws -> CompanionPasskeyAuthentication
    func revokeCredential(credentialID: String) async throws
}

struct CompanionHTTPPasskeyAuthenticationClient: CompanionPasskeyAuthenticationClient {
    func issueRegistrationChallenge() async throws -> CompanionPasskeyChallenge {
        try await post(path: "/api/mobile/passkeys/registration-challenge")
    }

    func completeRegistration(
        challengeID: String,
        publicKeyX963: String,
        signature: String
    ) async throws -> CompanionPasskeyRegistration {
        try await post(
            path: "/api/mobile/passkeys/register",
            body: [
                "challengeId": challengeID,
                "publicKeyX963": publicKeyX963,
                "signature": signature,
            ]
        )
    }

    func issueAuthenticationChallenge(credentialID: String) async throws -> CompanionPasskeyChallenge {
        try await post(
            path: "/api/mobile/passkeys/authentication-challenge",
            body: ["credentialId": credentialID]
        )
    }

    func completeAuthentication(
        credentialID: String,
        challengeID: String,
        signature: String
    ) async throws -> CompanionPasskeyAuthentication {
        try await post(
            path: "/api/mobile/passkeys/authenticate",
            body: [
                "credentialId": credentialID,
                "challengeId": challengeID,
                "signature": signature,
            ]
        )
    }

    func revokeCredential(credentialID: String) async throws {
        let _: EmptyPasskeyResponse = try await request(
            path: "/api/mobile/passkeys/\(credentialID)",
            method: "DELETE"
        )
    }

    private func post<Response: Decodable>(
        path: String,
        body: [String: Any] = [:]
    ) async throws -> Response {
        try await request(path: path, method: "POST", body: body)
    }

    private func request<Response: Decodable>(
        path: String,
        method: String,
        body: [String: Any]? = nil
    ) async throws -> Response {
        let connection = CompanionConfiguration.resolvedConnection()
        guard !connection.baseURLs.isEmpty else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        var lastError: Error?
        for baseURL in CompanionBaseURLFiltering.uniqueAttemptableBaseURLs(connection.baseURLs) {
            do {
                let data = try await responseData(
                    baseURL: baseURL,
                    bearerToken: connection.bearerToken,
                    path: path,
                    method: method,
                    body: body
                )
                return try JSONDecoder().decode(Response.self, from: data)
            } catch {
                lastError = error
            }
        }

        throw lastError ?? HTTPCompanionServiceError.invalidResponse
    }

    private func responseData(
        baseURL: URL,
        bearerToken: String?,
        path: String,
        method: String,
        body: [String: Any]? = nil
    ) async throws -> Data {
        var request = URLRequest(url: baseURL.appending(path: path))
        request.httpMethod = method
        request.timeoutInterval = CompanionPasskeyHTTPConstants.timeoutInterval
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        if let bearerToken {
            request.setValue("Bearer \(bearerToken)", forHTTPHeaderField: "Authorization")
        }

        if let body {
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw HTTPCompanionServiceError.invalidResponse
        }

        guard httpResponse.statusCode != CompanionPasskeyHTTPConstants.unauthorizedStatus else {
            throw HTTPCompanionServiceError.unauthorized
        }

        guard CompanionPasskeyHTTPConstants.successStatusRange.contains(httpResponse.statusCode) else {
            let serverMessage = String(data: data, encoding: .utf8) ?? "Passkey request failed."
            throw HTTPCompanionServiceError.serverError(serverMessage)
        }

        return data
    }
}

private enum CompanionPasskeyHTTPConstants {
    static let timeoutInterval: TimeInterval = 5
    static let unauthorizedStatus = 401
    static let successStatusRange = 200..<300
}

private struct EmptyPasskeyResponse: Decodable {}

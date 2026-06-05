import Foundation
import LocalAuthentication
import Observation

private enum CompanionAppAuthenticationConstants {
    static let faceIDUnlockStorageKey = "faceIDUnlockEnabled"
    static let passkeyCredentialIDStorageKey = "faceIDPasskeyCredentialID"
    static let localUnlockReason = "Unlock looper on this iPhone."
    static let registrationReason = "Register a Face ID protected passkey with your Mac."
    static let unlockReason = "Verify your Face ID passkey with your Mac."
}

private enum CompanionAppAuthenticationError: LocalizedError {
    case faceIDUnavailable(String)
    case faceIDRequired
    case passkeyMissing
    case passkeyVerificationFailed

    var errorDescription: String? {
        switch self {
        case let .faceIDUnavailable(reason):
            return reason
        case .faceIDRequired:
            return "Face ID is required for app unlock on this iPhone."
        case .passkeyMissing:
            return "The Face ID passkey is missing. Turn Face ID Unlock on again in Settings."
        case .passkeyVerificationFailed:
            return "The Mac could not verify this Face ID passkey."
        }
    }
}

@MainActor
@Observable
final class CompanionAppAuthenticator {
    private(set) var errorMessage: String?
    private(set) var isAuthenticating = false
    private(set) var isUnlocked: Bool
    private(set) var isFaceIDUnlockEnabled: Bool

    @ObservationIgnored private let userDefaults: UserDefaults
    @ObservationIgnored private let passkeyStore: CompanionDevicePasskeyStore
    @ObservationIgnored private let authenticationClient: any CompanionPasskeyAuthenticationClient

    init(
        userDefaults: UserDefaults = .standard,
        passkeyStore: CompanionDevicePasskeyStore = .shared,
        authenticationClient: any CompanionPasskeyAuthenticationClient = CompanionHTTPPasskeyAuthenticationClient()
    ) {
        self.userDefaults = userDefaults
        self.passkeyStore = passkeyStore
        self.authenticationClient = authenticationClient

        let storedPasskeyIsUsable = Self.storedCredentialID(in: userDefaults) != nil &&
            passkeyStore.containsPrivateKey()
        let storedFaceIDUnlockEnabled = userDefaults.bool(
            forKey: CompanionAppAuthenticationConstants.faceIDUnlockStorageKey
        ) && storedPasskeyIsUsable

        if !storedFaceIDUnlockEnabled {
            Self.clearStoredPasskeyState(in: userDefaults)
            CompanionMobileSessionStore.clear()
        }

        isFaceIDUnlockEnabled = storedFaceIDUnlockEnabled
        isUnlocked = !storedFaceIDUnlockEnabled
        CompanionDiagnostics.record(
            "auth:init faceIDEnabled=\(isFaceIDUnlockEnabled) unlocked=\(isUnlocked)"
        )
    }

    var faceIDStatusMessage: String {
        if isFaceIDUnlockEnabled {
            return "Your Mac verifies a Face ID protected passkey before looper unlocks."
        }

        return "Register a device passkey with your Mac and require Face ID before sessions are shown."
    }

    func setFaceIDUnlockEnabled(_ isEnabled: Bool) async {
        if isEnabled {
            await enableFaceIDUnlock()
        } else {
            await disableFaceIDUnlock()
        }
    }

    func unlock() async {
        guard isFaceIDUnlockEnabled else {
            isUnlocked = true
            errorMessage = nil
            return
        }

        await unlockWithFaceID()
    }

    func lockIfNeeded() {
        guard isFaceIDUnlockEnabled, !isAuthenticating else {
            return
        }

        CompanionMobileSessionStore.clear()
        isUnlocked = false
    }

    private func enableFaceIDUnlock() async {
        isAuthenticating = true
        defer {
            isAuthenticating = false
        }

        do {
            let context = try makeFaceIDContext()
            let challenge = try await authenticationClient.issueRegistrationChallenge()
            let devicePasskey = try passkeyStore.createPasskeyAndSign(
                message: challenge.message,
                context: context,
                prompt: CompanionAppAuthenticationConstants.registrationReason
            )
            let registration = try await authenticationClient.completeRegistration(
                challengeID: challenge.challengeId,
                publicKeyX963: devicePasskey.publicKeyX963,
                signature: devicePasskey.signature
            )

            userDefaults.set(
                registration.credentialId,
                forKey: CompanionAppAuthenticationConstants.passkeyCredentialIDStorageKey
            )
            userDefaults.set(true, forKey: CompanionAppAuthenticationConstants.faceIDUnlockStorageKey)
            CompanionMobileSessionStore.store(registration.session)
            isFaceIDUnlockEnabled = true
            isUnlocked = true
            errorMessage = nil
            Haptics.success()
        } catch {
            passkeyStore.deletePrivateKey()
            Self.clearStoredPasskeyState(in: userDefaults)
            CompanionMobileSessionStore.clear()
            isFaceIDUnlockEnabled = false
            isUnlocked = true
            errorMessage = displayMessage(for: error)
            Haptics.error()
        }
    }

    private func unlockWithFaceID() async {
        isAuthenticating = true
        defer {
            isAuthenticating = false
        }

        do {
            let context = try makeFaceIDContext()
            try await evaluateFaceID(
                context: context,
                reason: CompanionAppAuthenticationConstants.localUnlockReason
            )
            try await refreshRegisteredPasskeySession(context: context)
            errorMessage = nil
            isUnlocked = true
            Haptics.success()
        } catch {
            CompanionMobileSessionStore.clear()
            errorMessage = unlockFailureMessage(for: error)
            isUnlocked = false
            Haptics.error()
        }
    }

    private func refreshRegisteredPasskeySession(context: LAContext) async throws {
        let credentialID = try storedCredentialID()
        let challenge = try await authenticationClient.issueAuthenticationChallenge(
            credentialID: credentialID
        )
        let signature = try passkeyStore.sign(
            message: challenge.message,
            context: context,
            prompt: CompanionAppAuthenticationConstants.unlockReason
        )
        let result = try await authenticationClient.completeAuthentication(
            credentialID: credentialID,
            challengeID: challenge.challengeId,
            signature: signature
        )

        guard result.ok else {
            throw CompanionAppAuthenticationError.passkeyVerificationFailed
        }

        CompanionMobileSessionStore.store(result.session)
    }

    private func disableFaceIDUnlock() async {
        if let credentialID = Self.storedCredentialID(in: userDefaults) {
            try? await authenticationClient.revokeCredential(credentialID: credentialID)
        }

        passkeyStore.deletePrivateKey()
        Self.clearStoredPasskeyState(in: userDefaults)
        CompanionMobileSessionStore.clear()
        isFaceIDUnlockEnabled = false
        isUnlocked = true
        errorMessage = nil
        Haptics.selectionChanged()
    }

    private func storedCredentialID() throws -> String {
        guard let credentialID = Self.storedCredentialID(in: userDefaults) else {
            throw CompanionAppAuthenticationError.passkeyMissing
        }

        return credentialID
    }

    private static func storedCredentialID(in userDefaults: UserDefaults) -> String? {
        let storedValue = userDefaults.string(
            forKey: CompanionAppAuthenticationConstants.passkeyCredentialIDStorageKey
        )?
            .trimmingCharacters(in: .whitespacesAndNewlines)

        return storedValue?.isEmpty == false ? storedValue : nil
    }

    private static func clearStoredPasskeyState(in userDefaults: UserDefaults) {
        userDefaults.set(false, forKey: CompanionAppAuthenticationConstants.faceIDUnlockStorageKey)
        userDefaults.removeObject(
            forKey: CompanionAppAuthenticationConstants.passkeyCredentialIDStorageKey
        )
    }

    private func makeFaceIDContext() throws -> LAContext {
        let context = LAContext()
        var evaluationError: NSError?

        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &evaluationError) else {
            throw CompanionAppAuthenticationError.faceIDUnavailable(
                faceIDUnavailableMessage(from: evaluationError)
            )
        }

        guard context.biometryType == .faceID else {
            throw CompanionAppAuthenticationError.faceIDRequired
        }

        return context
    }

    private func evaluateFaceID(context: LAContext, reason: String) async throws {
        try await withCheckedThrowingContinuation { continuation in
            context.evaluatePolicy(
                .deviceOwnerAuthenticationWithBiometrics,
                localizedReason: reason
            ) { didAuthenticate, error in
                if didAuthenticate {
                    continuation.resume()
                } else {
                    continuation.resume(throwing: error ?? CompanionAppAuthenticationError.faceIDRequired)
                }
            }
        }
    }

    private func faceIDUnavailableMessage(from error: NSError?) -> String {
        guard let error else {
            return "Face ID is not available on this iPhone."
        }

        switch LAError.Code(rawValue: error.code) {
        case .biometryNotEnrolled:
            return "Set up Face ID in iOS Settings before enabling app unlock."
        case .biometryLockout:
            return "Face ID is locked. Unlock the iPhone once, then try again."
        case .biometryNotAvailable:
            return "Face ID is not available on this iPhone."
        case .passcodeNotSet:
            return "Set an iPhone passcode before enabling Face ID Unlock."
        default:
            return error.localizedDescription
        }
    }

    private func displayMessage(for error: any Error) -> String {
        if let localizedError = error as? LocalizedError,
           let description = localizedError.errorDescription {
            return description
        }

        return error.localizedDescription
    }

    private func unlockFailureMessage(for error: any Error) -> String {
        CompanionDiagnostics.lifecycle.error(
            "Face ID passkey unlock failed error=\(error.localizedDescription, privacy: .public)"
        )

        if let httpError = error as? HTTPCompanionServiceError {
            switch httpError {
            case .unauthorized:
                return "This iPhone is no longer paired with the Mac. Login again with a device code."
            case let .passkeySessionRequired(message):
                return message
            case .invalidResponse, .serverError:
                return displayMessage(for: error)
            }
        }

        let nsError = error as NSError
        if nsError.domain == NSURLErrorDomain {
            return "Face ID matched, but this iPhone could not reach the Mac to verify the passkey."
        }

        return displayMessage(for: error)
    }
}

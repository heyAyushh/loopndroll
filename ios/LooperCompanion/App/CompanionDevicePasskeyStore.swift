import Foundation
import LocalAuthentication
import Security

struct CompanionDevicePasskeySignature: Sendable {
    let publicKeyX963: String
    let signature: String
}

enum CompanionDevicePasskeyError: LocalizedError {
    case accessControlCreationFailed
    case keyGenerationFailed
    case privateKeyMissing
    case publicKeyMissing
    case publicKeyExportFailed
    case signatureUnsupported
    case signingFailed

    var errorDescription: String? {
        switch self {
        case .accessControlCreationFailed:
            return "Could not create Face ID protection for the passkey."
        case .keyGenerationFailed:
            return "Could not create the Secure Enclave passkey."
        case .privateKeyMissing:
            return "The Face ID passkey is missing. Turn Face ID Unlock on again in Settings."
        case .publicKeyMissing:
            return "Could not read the passkey public key."
        case .publicKeyExportFailed:
            return "Could not export the passkey public key."
        case .signatureUnsupported:
            return "This iPhone cannot sign the passkey challenge."
        case .signingFailed:
            return "Face ID passkey signing failed."
        }
    }
}

struct CompanionDevicePasskeyStore: Sendable {
    static let shared = CompanionDevicePasskeyStore()

    private static let keySizeInBits = 256
    private static let privateKeyTag = Data("dev.looper.app.ios.face-id-passkey.v1".utf8)
    private static let signingAlgorithm = SecKeyAlgorithm.ecdsaSignatureMessageX962SHA256

    func createPasskeyAndSign(
        message: String,
        context: LAContext,
        prompt: String
    ) throws -> CompanionDevicePasskeySignature {
        deletePrivateKey()
        let privateKey = try createPrivateKey()
        guard let publicKey = SecKeyCopyPublicKey(privateKey) else {
            throw CompanionDevicePasskeyError.publicKeyMissing
        }

        var exportError: Unmanaged<CFError>?
        guard let publicKeyData = SecKeyCopyExternalRepresentation(publicKey, &exportError) as Data? else {
            throw CompanionDevicePasskeyError.publicKeyExportFailed
        }

        return CompanionDevicePasskeySignature(
            publicKeyX963: publicKeyData.base64URLEncodedString(),
            signature: try sign(message: message, context: context, prompt: prompt)
        )
    }

    func sign(message: String, context: LAContext, prompt: String) throws -> String {
        let privateKey = try loadPrivateKey(context: context, prompt: prompt)
        guard SecKeyIsAlgorithmSupported(privateKey, .sign, Self.signingAlgorithm) else {
            throw CompanionDevicePasskeyError.signatureUnsupported
        }

        var signingError: Unmanaged<CFError>?
        guard let signature = SecKeyCreateSignature(
            privateKey,
            Self.signingAlgorithm,
            Data(message.utf8) as CFData,
            &signingError
        ) as Data? else {
            throw CompanionDevicePasskeyError.signingFailed
        }

        return signature.base64URLEncodedString()
    }

    func containsPrivateKey() -> Bool {
        var item: CFTypeRef?
        let status = SecItemCopyMatching(privateKeyLookupQuery(returnAttributes: true) as CFDictionary, &item)
        return status == errSecSuccess
    }

    func deletePrivateKey() {
        SecItemDelete(privateKeyLookupQuery(returnAttributes: false) as CFDictionary)
    }

    private func createPrivateKey() throws -> SecKey {
        let accessControl = try makeAccessControl()
        let privateKeyAttributes: [String: Any] = [
            kSecAttrIsPermanent as String: true,
            kSecAttrApplicationTag as String: Self.privateKeyTag,
            kSecAttrAccessControl as String: accessControl,
        ]
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: Self.keySizeInBits,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs as String: privateKeyAttributes,
        ]

        var creationError: Unmanaged<CFError>?
        guard let privateKey = SecKeyCreateRandomKey(attributes as CFDictionary, &creationError) else {
            throw CompanionDevicePasskeyError.keyGenerationFailed
        }

        return privateKey
    }

    private func makeAccessControl() throws -> SecAccessControl {
        var accessControlError: Unmanaged<CFError>?
        guard let accessControl = SecAccessControlCreateWithFlags(
            nil,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            [.biometryCurrentSet, .privateKeyUsage],
            &accessControlError
        ) else {
            throw CompanionDevicePasskeyError.accessControlCreationFailed
        }

        return accessControl
    }

    private func loadPrivateKey(context: LAContext, prompt: String) throws -> SecKey {
        var item: CFTypeRef?
        context.localizedReason = prompt
        var query = privateKeyLookupQuery(returnAttributes: false)
        query[kSecReturnRef as String] = true
        query[kSecUseAuthenticationContext as String] = context

        let status = SecItemCopyMatching(query as CFDictionary, &item)
        guard status == errSecSuccess, item != nil else {
            throw CompanionDevicePasskeyError.privateKeyMissing
        }

        return item as! SecKey
    }

    private func privateKeyLookupQuery(returnAttributes: Bool) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassKey,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrApplicationTag as String: Self.privateKeyTag,
        ]

        if returnAttributes {
            query[kSecReturnAttributes as String] = true
        }

        return query
    }
}

private extension Data {
    func base64URLEncodedString() -> String {
        base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

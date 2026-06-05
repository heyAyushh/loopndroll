use agent_control_plane::mobile_auth::{
    CompleteMobilePasskeyAuthenticationInput, CompleteMobilePasskeyRegistrationInput,
    MobileAuthError, MobileAuthService, mobile_authorization_header,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use tempfile::TempDir;

const FIXED_SIGNING_KEY_BYTES: [u8; 32] = [7; 32];

struct MobileAuthFixture {
    _temp_dir: TempDir,
    service: MobileAuthService,
}

struct TestDevicePasskey {
    signing_key: SigningKey,
    public_key_x963: String,
}

impl MobileAuthFixture {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let service = MobileAuthService::new(temp_dir.path().join("control-plane.sqlite"));
        Self {
            _temp_dir: temp_dir,
            service,
        }
    }
}

impl TestDevicePasskey {
    fn new() -> Self {
        let signing_key =
            SigningKey::from_slice(&FIXED_SIGNING_KEY_BYTES).expect("fixed signing key");
        let public_key = signing_key.verifying_key().to_encoded_point(false);
        Self {
            signing_key,
            public_key_x963: URL_SAFE_NO_PAD.encode(public_key.as_bytes()),
        }
    }

    fn sign(&self, message: &str) -> String {
        let signature: Signature = self.signing_key.sign(message.as_bytes());
        URL_SAFE_NO_PAD.encode(signature.to_der().as_bytes())
    }
}

#[test]
fn mobile_pairing_tokens_validate_authorization_headers() {
    let fixture = MobileAuthFixture::new();
    let token = fixture
        .service
        .issue_pairing_token()
        .expect("issue pairing token");
    let authorization_header = mobile_authorization_header(&token);

    assert!(
        fixture
            .service
            .validate_authorization_header(Some(&authorization_header))
            .expect("validate token")
    );
    assert!(
        !fixture
            .service
            .validate_authorization_header(Some("Bearer bad.token"))
            .expect("reject bad token")
    );
    assert!(
        !fixture
            .service
            .validate_authorization_header(None)
            .expect("reject missing token")
    );
}

#[test]
fn mobile_connection_codes_include_one_time_resolvable_orbs() {
    let fixture = MobileAuthFixture::new();
    let connection_code = fixture
        .service
        .issue_connection_code(vec!["http://192.168.1.4:8765".to_owned()])
        .expect("issue connection code");

    assert!(!connection_code.code.is_empty());
    assert!(connection_code.orb_id.starts_with("orb1_"));

    let resolved_connection_code = fixture
        .service
        .resolve_connection_orb(&connection_code.orb_id)
        .expect("resolve connection orb once");
    assert_eq!(resolved_connection_code.code, connection_code.code);
    assert_eq!(resolved_connection_code.orb_id, connection_code.orb_id);

    let error = fixture
        .service
        .resolve_connection_orb(&connection_code.orb_id)
        .expect_err("connection orb should be one-time");
    assert!(matches!(error, MobileAuthError::ConnectionOrbExpired));
}

#[test]
fn mobile_connection_orb_image_round_trips_to_resolver() {
    let fixture = MobileAuthFixture::new();
    let orb_image = fixture
        .service
        .issue_connection_orb_image(vec!["http://192.168.1.4:8765".to_owned()])
        .expect("issue connection orb image");

    assert!(!orb_image.png_data.is_empty());
    assert!(orb_image.connection_code.orb_id.starts_with("orb1_"));

    let resolved_connection_code = fixture
        .service
        .resolve_connection_orb(&orb_image.connection_code.orb_id)
        .expect("resolve image orb");
    assert_eq!(
        resolved_connection_code.code,
        orb_image.connection_code.code
    );
}

#[test]
fn mobile_passkey_registration_and_authentication_issue_sessions() {
    let fixture = MobileAuthFixture::new();
    let pairing_token = fixture
        .service
        .issue_pairing_token()
        .expect("issue pairing token");
    let passkey = TestDevicePasskey::new();

    let registration_challenge = fixture
        .service
        .issue_registration_challenge(&pairing_token.id)
        .expect("registration challenge");
    let registration = fixture
        .service
        .complete_registration(
            CompleteMobilePasskeyRegistrationInput {
                challenge_id: registration_challenge.challenge_id,
                public_key_x963: passkey.public_key_x963.clone(),
                signature: passkey.sign(&registration_challenge.message),
                label: Some("Test iPhone".to_owned()),
            },
            &pairing_token.id,
        )
        .expect("complete registration");
    let registration_session = format!(
        "{}.{}",
        registration.session.session_id, registration.session.session_token
    );

    assert!(
        fixture
            .service
            .has_active_passkey_credentials(&pairing_token.id)
            .expect("active credential")
    );
    assert!(matches!(
        fixture
            .service
            .validate_api_session_header(None, &pairing_token.id)
            .expect_err("missing session should require Face ID"),
        MobileAuthError::PasskeySessionRequired
    ));
    assert!(
        fixture
            .service
            .validate_session_header(Some(&registration_session), &pairing_token.id)
            .expect("registration session")
    );
    fixture
        .service
        .validate_api_session_header(Some(&registration_session), &pairing_token.id)
        .expect("registration session authorizes API");

    let authentication_challenge = fixture
        .service
        .issue_authentication_challenge(&registration.credential_id, &pairing_token.id)
        .expect("authentication challenge");
    let authentication = fixture
        .service
        .complete_authentication(
            CompleteMobilePasskeyAuthenticationInput {
                credential_id: registration.credential_id.clone(),
                challenge_id: authentication_challenge.challenge_id,
                signature: passkey.sign(&authentication_challenge.message),
            },
            &pairing_token.id,
        )
        .expect("complete authentication");
    let authentication_session = format!(
        "{}.{}",
        authentication.session.session_id, authentication.session.session_token
    );

    assert!(authentication.ok);
    assert_eq!(authentication.credential_id, registration.credential_id);
    assert!(
        fixture
            .service
            .validate_session_header(Some(&authentication_session), &pairing_token.id)
            .expect("authentication session")
    );
}

#[test]
fn mobile_connection_management_renames_and_revokes_pairing() {
    let fixture = MobileAuthFixture::new();
    let pairing_token = fixture
        .service
        .issue_pairing_token()
        .expect("issue pairing token");
    let passkey = TestDevicePasskey::new();
    let registration_challenge = fixture
        .service
        .issue_registration_challenge(&pairing_token.id)
        .expect("registration challenge");
    let registration = fixture
        .service
        .complete_registration(
            CompleteMobilePasskeyRegistrationInput {
                challenge_id: registration_challenge.challenge_id,
                public_key_x963: passkey.public_key_x963.clone(),
                signature: passkey.sign(&registration_challenge.message),
                label: Some("Original iPhone".to_owned()),
            },
            &pairing_token.id,
        )
        .expect("complete registration");
    let session_header = format!(
        "{}.{}",
        registration.session.session_id, registration.session.session_token
    );

    fixture
        .service
        .rename_mobile_connection(&pairing_token.id, "Desk iPhone")
        .expect("rename connection");
    let renamed_connection = fixture
        .service
        .managed_connections()
        .expect("managed connections")
        .into_iter()
        .find(|connection| connection.id == pairing_token.id)
        .expect("renamed connection");

    assert_eq!(renamed_connection.label, "Desk iPhone");
    assert_eq!(renamed_connection.status, "secured");
    assert!(renamed_connection.can_revoke);

    fixture
        .service
        .revoke_mobile_connection(&pairing_token.id)
        .expect("revoke connection");
    assert!(
        !fixture
            .service
            .validate_authorization_header(Some(&mobile_authorization_header(&pairing_token)))
            .expect("revoked token is rejected")
    );
    assert!(
        !fixture
            .service
            .validate_session_header(Some(&session_header), &pairing_token.id)
            .expect("revoked session is rejected")
    );

    let revoked_connection = fixture
        .service
        .managed_connections()
        .expect("managed connections")
        .into_iter()
        .find(|connection| connection.id == pairing_token.id)
        .expect("revoked connection");
    assert_eq!(revoked_connection.status, "revoked");
    assert!(!revoked_connection.can_revoke);
}

#[test]
fn mobile_passkey_registration_rejects_wrong_signature() {
    let fixture = MobileAuthFixture::new();
    let pairing_token = fixture
        .service
        .issue_pairing_token()
        .expect("issue pairing token");
    let passkey = TestDevicePasskey::new();
    let challenge = fixture
        .service
        .issue_registration_challenge(&pairing_token.id)
        .expect("registration challenge");

    let error = fixture
        .service
        .complete_registration(
            CompleteMobilePasskeyRegistrationInput {
                challenge_id: challenge.challenge_id,
                public_key_x963: passkey.public_key_x963.clone(),
                signature: passkey.sign("not the server challenge"),
                label: None,
            },
            &pairing_token.id,
        )
        .expect_err("wrong signature should fail");

    assert!(matches!(error, MobileAuthError::InvalidSignature));
}

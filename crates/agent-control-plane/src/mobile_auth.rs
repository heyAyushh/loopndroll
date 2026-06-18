use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use orb_code::{GenerateOrbRequest, OrbId, derive_orb_id, generate_orb_image};
use p256::ecdsa::signature::Verifier;
use p256::ecdsa::{Signature, VerifyingKey};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

const BEARER_PREFIX: &str = "Bearer ";
const DEFAULT_PAIRING_LABEL: &str = "Mobile companion";
const DEFAULT_PASSKEY_LABEL: &str = "Face ID passkey";
const MOBILE_PASSKEY_MESSAGE_PREFIX: &str = "looper-mobile-passkey-v1";
const TOKEN_RANDOM_BYTE_COUNT: usize = 32;
const PASSKEY_PUBLIC_KEY_X963_BYTE_COUNT: usize = 65;
const PASSKEY_PUBLIC_KEY_UNCOMPRESSED_PREFIX: u8 = 0x04;
const PASSKEY_CHALLENGE_TTL_SECONDS: i64 = 5 * 60;
const PASSKEY_SESSION_TTL_SECONDS: i64 = 12 * 60 * 60;
const PASSKEY_SESSION_SEPARATOR: char = '.';
pub const CONNECTION_ORB_TTL_SECONDS: i64 = 10 * 60;
const CONNECTION_ORB_IMAGE_SIZE: u32 = 1024;
const MOBILE_CONNECTION_KIND: &str = "mobile";

#[derive(Clone, Debug)]
pub struct MobileAuthService {
    store_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MobileAuthorizationCredential {
    pub id: String,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct IssuedMobilePairingToken {
    pub id: String,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileConnectionCode {
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "baseURLs")]
    pub base_urls: Vec<String>,
    pub pairing_token_id: String,
    pub pairing_token: String,
    pub code: String,
    pub orb_id: String,
    pub generated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobileConnectionOrbImage {
    pub connection_code: MobileConnectionCode,
    pub png_data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MobileConnectionCodePayload {
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "baseURLs")]
    base_urls: Vec<String>,
    pairing_token_id: String,
    pairing_token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePasskeyChallengeResponse {
    pub challenge_id: String,
    pub challenge: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompleteMobilePasskeyRegistrationInput {
    pub challenge_id: String,
    pub public_key_x963: String,
    pub signature: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CompleteMobilePasskeyAuthenticationInput {
    pub credential_id: String,
    pub challenge_id: String,
    pub signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePasskeyRegistrationResponse {
    pub credential_id: String,
    pub session: MobilePasskeySessionResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePasskeyAuthenticationResponse {
    pub ok: bool,
    pub credential_id: String,
    pub session: MobilePasskeySessionResponse,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MobilePasskeySessionResponse {
    pub session_id: String,
    pub session_token: String,
    pub expires_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManagedMobileConnection {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub status: String,
    pub passkey_credential_id: Option<String>,
    pub passkey_label: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub revoked_at: Option<String>,
    pub can_rename: bool,
    pub can_revoke: bool,
}

#[derive(Debug, Error)]
pub enum MobileAuthError {
    #[error("mobile auth store failed: {0}")]
    Store(#[from] rusqlite::Error),
    #[error("mobile auth filesystem failed: {0}")]
    Filesystem(#[from] std::io::Error),
    #[error("mobile auth timestamp failed: {0}")]
    TimeFormat(#[from] time::error::Format),
    #[error("pairing token is required")]
    PairingTokenRequired,
    #[error("Face ID unlock is required for this iPhone")]
    PasskeySessionRequired,
    #[error("passkey credential ID is required")]
    CredentialRequired,
    #[error("passkey request is missing required values")]
    MissingRequiredValues,
    #[error("passkey challenge is invalid or expired")]
    InvalidChallenge,
    #[error("passkey challenge does not match this iPhone")]
    ChallengePairingMismatch,
    #[error("passkey challenge does not match the credential")]
    ChallengeCredentialMismatch,
    #[error("passkey credential is not registered")]
    CredentialNotRegistered,
    #[error("passkey credential is not registered for this iPhone")]
    CredentialPairingMismatch,
    #[error("passkey public key is invalid")]
    InvalidPublicKey,
    #[error("passkey signature is invalid")]
    InvalidSignature,
    #[error("passkey payload is invalid")]
    InvalidEncoding,
    #[error("connection orb is required")]
    ConnectionOrbRequired,
    #[error("connection orb was not found")]
    ConnectionOrbNotFound,
    #[error("connection orb expired or was already used")]
    ConnectionOrbExpired,
    #[error("connection orb image failed: {0}")]
    Orb(#[from] orb_code::OrbError),
    #[error("mobile auth JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

type MobileAuthResult<T> = Result<T, MobileAuthError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MobilePasskeyPurpose {
    Registration,
    Authentication,
}

impl MobilePasskeyPurpose {
    fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "registration",
            Self::Authentication => "authentication",
        }
    }
}

#[derive(Clone, Debug)]
struct MobilePasskeyChallengeRow {
    id: String,
    challenge: String,
    purpose: String,
    credential_id: Option<String>,
    pairing_token_id: String,
    created_at: String,
    consumed_at: Option<String>,
}

#[derive(Clone, Debug)]
struct MobilePasskeyCredentialRow {
    id: String,
    public_key_x963: String,
    pairing_token_id: String,
    revoked_at: Option<String>,
}

#[derive(Clone, Debug)]
struct ManagedMobileConnectionRow {
    pairing_token_id: String,
    pairing_label: Option<String>,
    pairing_created_at: String,
    pairing_last_used_at: Option<String>,
    pairing_revoked_at: Option<String>,
    passkey_credential_id: Option<String>,
    passkey_label: Option<String>,
    passkey_created_at: Option<String>,
    passkey_last_used_at: Option<String>,
    passkey_revoked_at: Option<String>,
}

#[derive(Clone, Debug)]
struct MobilePasskeySessionCredential {
    id: String,
    token: String,
}

impl MobileAuthService {
    pub fn new(store_path: PathBuf) -> Self {
        Self { store_path }
    }

    pub fn store_path(&self) -> &Path {
        &self.store_path
    }

    pub fn initialize(&self) -> MobileAuthResult<()> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.store_path)?;
        connection.execute_batch(
            r#"
create table if not exists mobile_pairing_tokens (
  id text primary key,
  token_hash text not null,
  label text,
  created_at text not null,
  last_used_at text,
  revoked_at text
);

create index if not exists mobile_pairing_tokens_active_idx
  on mobile_pairing_tokens(revoked_at, created_at desc, id);

create table if not exists mobile_connection_orbs (
  orb_id text primary key,
  connection_code text not null,
  created_at text not null,
  consumed_at text
);

create index if not exists mobile_connection_orbs_lookup_idx
  on mobile_connection_orbs(consumed_at, created_at desc, orb_id);

create table if not exists mobile_passkey_credentials (
  id text primary key,
  public_key_x963 text not null,
  pairing_token_id text not null,
  label text,
  created_at text not null,
  last_used_at text,
  revoked_at text
);

create index if not exists mobile_passkey_credentials_pairing_token_idx
  on mobile_passkey_credentials(pairing_token_id, revoked_at, created_at desc, id);

create table if not exists mobile_passkey_challenges (
  id text primary key,
  challenge text not null,
  purpose text not null check (purpose in ('registration', 'authentication')),
  credential_id text,
  pairing_token_id text not null,
  created_at text not null,
  consumed_at text
);

create index if not exists mobile_passkey_challenges_lookup_idx
  on mobile_passkey_challenges(purpose, credential_id, consumed_at, created_at desc);

create table if not exists mobile_passkey_sessions (
  id text primary key,
  token_hash text not null,
  credential_id text not null,
  pairing_token_id text not null,
  created_at text not null,
  last_used_at text,
  expires_at text not null,
  revoked_at text
);

create index if not exists mobile_passkey_sessions_lookup_idx
  on mobile_passkey_sessions(pairing_token_id, revoked_at, expires_at, id);
"#,
        )?;
        Ok(())
    }

    pub fn issue_pairing_token(&self) -> MobileAuthResult<IssuedMobilePairingToken> {
        let issued_token = IssuedMobilePairingToken {
            id: new_id(),
            token: random_base64_url(TOKEN_RANDOM_BYTE_COUNT),
        };
        self.connection()?.execute(
            "insert into mobile_pairing_tokens (
                id, token_hash, label, created_at, last_used_at, revoked_at
            ) values (?1, ?2, ?3, ?4, null, null)",
            params![
                issued_token.id.as_str(),
                hash_token(&issued_token.token),
                DEFAULT_PAIRING_LABEL,
                now_iso_string()?,
            ],
        )?;
        Ok(issued_token)
    }

    pub fn issue_connection_code(
        &self,
        base_urls: Vec<String>,
    ) -> MobileAuthResult<MobileConnectionCode> {
        self.prune_stale_connection_orbs()?;
        let issued_token = self.issue_pairing_token()?;
        let base_url = base_urls.first().cloned().unwrap_or_default();
        let pairing_token_id = issued_token.id.clone();
        let pairing_token = issued_token.token.clone();
        let payload = serde_json::json!({
            "baseURL": &base_url,
            "baseURLs": &base_urls,
            "pairingTokenId": &pairing_token_id,
            "pairingToken": &pairing_token,
        });
        let code = encode_base64_url(&serde_json::to_vec(&payload)?);
        let orb_id = derive_orb_id(&code).to_string();
        let generated_at = now_iso_string()?;
        self.connection()?.execute(
            "insert or replace into mobile_connection_orbs (
                orb_id, connection_code, created_at, consumed_at
            ) values (?1, ?2, ?3, null)",
            params![orb_id.as_str(), code.as_str(), generated_at.as_str()],
        )?;
        Ok(MobileConnectionCode {
            base_url,
            base_urls,
            pairing_token_id,
            pairing_token,
            code,
            orb_id,
            generated_at,
        })
    }

    pub fn issue_connection_orb_image(
        &self,
        base_urls: Vec<String>,
    ) -> MobileAuthResult<MobileConnectionOrbImage> {
        let connection_code = self.issue_connection_code(base_urls)?;
        let orb_id = OrbId::parse(&connection_code.orb_id)?;
        let request = GenerateOrbRequest::new(orb_id).with_image_size(CONNECTION_ORB_IMAGE_SIZE)?;
        let png_data = generate_orb_image(&request)?.to_png_bytes()?;
        Ok(MobileConnectionOrbImage {
            connection_code,
            png_data,
        })
    }

    pub fn connection_orb_png_data(&self, orb_id: &str) -> MobileAuthResult<Vec<u8>> {
        self.prune_stale_connection_orbs()?;
        let orb_id = normalized_required(orb_id).ok_or(MobileAuthError::ConnectionOrbRequired)?;
        let row = self
            .connection()?
            .query_row(
                "select created_at, consumed_at
                 from mobile_connection_orbs
                 where orb_id = ?1
                 limit 1",
                [orb_id.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .optional()?;
        let Some((created_at, consumed_at)) = row else {
            return Err(MobileAuthError::ConnectionOrbNotFound);
        };
        if consumed_at.is_some() || !is_connection_orb_fresh(&created_at) {
            return Err(MobileAuthError::ConnectionOrbExpired);
        }

        let orb_id = OrbId::parse(&orb_id)?;
        let request = GenerateOrbRequest::new(orb_id).with_image_size(CONNECTION_ORB_IMAGE_SIZE)?;
        Ok(generate_orb_image(&request)?.to_png_bytes()?)
    }

    pub fn resolve_connection_orb(&self, orb_id: &str) -> MobileAuthResult<MobileConnectionCode> {
        self.prune_stale_connection_orbs()?;
        let orb_id = normalized_required(orb_id).ok_or(MobileAuthError::ConnectionOrbRequired)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let row = transaction
            .query_row(
                "select connection_code, created_at, consumed_at
                 from mobile_connection_orbs
                 where orb_id = ?1
                 limit 1",
                [orb_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((connection_code, generated_at, consumed_at)) = row else {
            return Err(MobileAuthError::ConnectionOrbNotFound);
        };
        if consumed_at.is_some() || !is_connection_orb_fresh(&generated_at) {
            return Err(MobileAuthError::ConnectionOrbExpired);
        }

        let changed = transaction.execute(
            "update mobile_connection_orbs
             set consumed_at = ?1
             where orb_id = ?2
               and consumed_at is null",
            params![now_iso_string()?, orb_id.as_str()],
        )?;
        if changed != 1 {
            return Err(MobileAuthError::ConnectionOrbExpired);
        }
        transaction.commit()?;
        mobile_connection_code_from_encoded_payload(&connection_code, &orb_id, &generated_at)
    }

    pub fn validate_authorization_header(
        &self,
        authorization_header: Option<&str>,
    ) -> MobileAuthResult<bool> {
        self.validate_authorization_credential(
            parse_mobile_authorization_header(authorization_header).as_ref(),
        )
    }

    pub fn validate_authorization_credential(
        &self,
        credential: Option<&MobileAuthorizationCredential>,
    ) -> MobileAuthResult<bool> {
        let Some(credential) = credential else {
            return Ok(false);
        };
        let connection = self.connection()?;
        let stored_hash = connection
            .query_row(
                "select token_hash
                 from mobile_pairing_tokens
                 where id = ?1
                   and revoked_at is null
                 limit 1",
                [credential.id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(stored_hash) = stored_hash else {
            return Ok(false);
        };
        let is_valid = constant_time_equals(&stored_hash, &hash_token(&credential.token));
        if is_valid {
            connection.execute(
                "update mobile_pairing_tokens set last_used_at = ?1 where id = ?2",
                params![now_iso_string()?, credential.id],
            )?;
        }
        Ok(is_valid)
    }

    pub fn issue_registration_challenge(
        &self,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeyChallengeResponse> {
        self.issue_challenge(MobilePasskeyPurpose::Registration, None, pairing_token_id)
    }

    pub fn complete_registration(
        &self,
        input: CompleteMobilePasskeyRegistrationInput,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeyRegistrationResponse> {
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        let challenge_id = normalized_required(&input.challenge_id)
            .ok_or(MobileAuthError::MissingRequiredValues)?;
        let public_key_x963 = normalized_required(&input.public_key_x963)
            .ok_or(MobileAuthError::MissingRequiredValues)?;
        let signature =
            normalized_required(&input.signature).ok_or(MobileAuthError::MissingRequiredValues)?;
        let challenge = self.load_challenge(&challenge_id, MobilePasskeyPurpose::Registration)?;
        if challenge.pairing_token_id != pairing_token_id {
            return Err(MobileAuthError::ChallengePairingMismatch);
        }
        if !verify_passkey_signature(&public_key_x963, &signing_message(&challenge), &signature)? {
            return Err(MobileAuthError::InvalidSignature);
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let credential_id = new_id();
        let credential_label = normalized_optional(input.label.as_deref())
            .unwrap_or_else(|| DEFAULT_PASSKEY_LABEL.to_owned());
        Self::consume_challenge_on(&transaction, &challenge.id)?;
        transaction.execute(
            "insert into mobile_passkey_credentials (
                id, public_key_x963, pairing_token_id, label, created_at, last_used_at, revoked_at
            ) values (?1, ?2, ?3, ?4, ?5, null, null)",
            params![
                credential_id.as_str(),
                public_key_x963.as_str(),
                pairing_token_id.as_str(),
                credential_label,
                now_iso_string()?,
            ],
        )?;
        let session =
            Self::create_passkey_session_on(&transaction, &credential_id, &pairing_token_id)?;
        transaction.commit()?;

        Ok(MobilePasskeyRegistrationResponse {
            credential_id: credential_id.clone(),
            session,
        })
    }

    pub fn issue_authentication_challenge(
        &self,
        credential_id: &str,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeyChallengeResponse> {
        let credential_id =
            normalized_required(credential_id).ok_or(MobileAuthError::CredentialRequired)?;
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        self.load_credential_for_pairing_token(&credential_id, &pairing_token_id)?;
        self.issue_challenge(
            MobilePasskeyPurpose::Authentication,
            Some(&credential_id),
            &pairing_token_id,
        )
    }

    pub fn complete_authentication(
        &self,
        input: CompleteMobilePasskeyAuthenticationInput,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeyAuthenticationResponse> {
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        let credential_id = normalized_required(&input.credential_id)
            .ok_or(MobileAuthError::MissingRequiredValues)?;
        let challenge_id = normalized_required(&input.challenge_id)
            .ok_or(MobileAuthError::MissingRequiredValues)?;
        let signature =
            normalized_required(&input.signature).ok_or(MobileAuthError::MissingRequiredValues)?;
        let credential =
            self.load_credential_for_pairing_token(&credential_id, &pairing_token_id)?;
        let challenge = self.load_challenge(&challenge_id, MobilePasskeyPurpose::Authentication)?;
        if challenge.credential_id.as_deref() != Some(credential.id.as_str()) {
            return Err(MobileAuthError::ChallengeCredentialMismatch);
        }
        if challenge.pairing_token_id != pairing_token_id {
            return Err(MobileAuthError::ChallengePairingMismatch);
        }
        if !verify_passkey_signature(
            &credential.public_key_x963,
            &signing_message(&challenge),
            &signature,
        )? {
            return Err(MobileAuthError::InvalidSignature);
        }

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        Self::consume_challenge_on(&transaction, &challenge.id)?;
        let timestamp = now_iso_string()?;
        transaction.execute(
            "update mobile_passkey_credentials set last_used_at = ?1 where id = ?2",
            params![timestamp, credential.id],
        )?;
        let session =
            Self::create_passkey_session_on(&transaction, &credential.id, &pairing_token_id)?;
        transaction.commit()?;

        Ok(MobilePasskeyAuthenticationResponse {
            ok: true,
            credential_id: credential.id.clone(),
            session,
        })
    }

    pub fn revoke_credential(
        &self,
        credential_id: &str,
        pairing_token_id: &str,
    ) -> MobileAuthResult<()> {
        let credential_id =
            normalized_required(credential_id).ok_or(MobileAuthError::CredentialRequired)?;
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        let timestamp = now_iso_string()?;
        let connection = self.connection()?;
        connection.execute(
            "update mobile_passkey_credentials
             set revoked_at = coalesce(revoked_at, ?1)
             where id = ?2
               and pairing_token_id = ?3",
            params![timestamp, credential_id, pairing_token_id],
        )?;
        connection.execute(
            "update mobile_passkey_sessions
             set revoked_at = coalesce(revoked_at, ?1)
             where credential_id = ?2
               and pairing_token_id = ?3",
            params![timestamp, credential_id, pairing_token_id],
        )?;
        Ok(())
    }

    pub fn has_active_passkey_credentials(&self, pairing_token_id: &str) -> MobileAuthResult<bool> {
        let Some(pairing_token_id) = normalized_required(pairing_token_id) else {
            return Ok(false);
        };
        let credential_id = self
            .connection()?
            .query_row(
                "select id
                 from mobile_passkey_credentials
                 where pairing_token_id = ?1
                   and revoked_at is null
                 limit 1",
                [pairing_token_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(credential_id.is_some())
    }

    pub fn validate_session_header(
        &self,
        session_header: Option<&str>,
        pairing_token_id: &str,
    ) -> MobileAuthResult<bool> {
        let Some(pairing_token_id) = normalized_required(pairing_token_id) else {
            return Ok(false);
        };
        let Some(session_credential) = parse_session_header(session_header) else {
            return Ok(false);
        };
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "select token_hash, expires_at, revoked_at
                 from mobile_passkey_sessions
                 where id = ?1
                   and pairing_token_id = ?2
                 limit 1",
                params![session_credential.id, pairing_token_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((token_hash, expires_at, revoked_at)) = row else {
            return Ok(false);
        };
        if revoked_at.is_some() || !is_future_timestamp(&expires_at) {
            return Ok(false);
        }
        if !constant_time_equals(&token_hash, &hash_token(&session_credential.token)) {
            return Ok(false);
        }
        connection.execute(
            "update mobile_passkey_sessions set last_used_at = ?1 where id = ?2",
            params![now_iso_string()?, session_credential.id],
        )?;
        Ok(true)
    }

    pub fn validate_api_session_header(
        &self,
        session_header: Option<&str>,
        pairing_token_id: &str,
    ) -> MobileAuthResult<()> {
        if !self.has_active_passkey_credentials(pairing_token_id)? {
            return Ok(());
        }

        if self.validate_session_header(session_header, pairing_token_id)? {
            return Ok(());
        }

        Err(MobileAuthError::PasskeySessionRequired)
    }

    pub fn managed_connections(&self) -> MobileAuthResult<Vec<ManagedMobileConnection>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "select
                token.id,
                token.label,
                token.created_at,
                token.last_used_at,
                token.revoked_at,
                credential.id,
                credential.label,
                credential.created_at,
                credential.last_used_at,
                credential.revoked_at
             from mobile_pairing_tokens token
             left join mobile_passkey_credentials credential
               on credential.id = (
                 select latest_credential.id
                 from mobile_passkey_credentials latest_credential
                 where latest_credential.pairing_token_id = token.id
                 order by
                   latest_credential.revoked_at is not null,
                   latest_credential.last_used_at desc,
                   latest_credential.created_at desc,
                   latest_credential.id desc
                 limit 1
               )
             order by
               token.revoked_at is not null,
               coalesce(token.last_used_at, token.created_at) desc,
               token.id desc",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ManagedMobileConnectionRow {
                pairing_token_id: row.get(0)?,
                pairing_label: row.get(1)?,
                pairing_created_at: row.get(2)?,
                pairing_last_used_at: row.get(3)?,
                pairing_revoked_at: row.get(4)?,
                passkey_credential_id: row.get(5)?,
                passkey_label: row.get(6)?,
                passkey_created_at: row.get(7)?,
                passkey_last_used_at: row.get(8)?,
                passkey_revoked_at: row.get(9)?,
            })
        })?;

        let mut connections = Vec::new();
        for row in rows {
            connections.push(managed_connection_from_row(row?));
        }
        Ok(connections)
    }

    pub fn rename_mobile_connection(
        &self,
        pairing_token_id: &str,
        label: &str,
    ) -> MobileAuthResult<()> {
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        let label = normalized_required(label).ok_or(MobileAuthError::MissingRequiredValues)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "update mobile_pairing_tokens
             set label = ?1
             where id = ?2
               and revoked_at is null",
            params![label.as_str(), pairing_token_id.as_str()],
        )?;
        if changed == 0 {
            return Err(MobileAuthError::CredentialNotRegistered);
        }

        connection.execute(
            "update mobile_passkey_credentials
             set label = ?1
             where pairing_token_id = ?2
               and revoked_at is null",
            params![label.as_str(), pairing_token_id.as_str()],
        )?;
        Ok(())
    }

    pub fn revoke_mobile_connection(&self, pairing_token_id: &str) -> MobileAuthResult<()> {
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        let timestamp = now_iso_string()?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "update mobile_pairing_tokens
             set revoked_at = coalesce(revoked_at, ?1)
             where id = ?2",
            params![timestamp.as_str(), pairing_token_id.as_str()],
        )?;
        if changed == 0 {
            return Err(MobileAuthError::CredentialNotRegistered);
        }

        connection.execute(
            "update mobile_passkey_credentials
             set revoked_at = coalesce(revoked_at, ?1)
             where pairing_token_id = ?2",
            params![timestamp.as_str(), pairing_token_id.as_str()],
        )?;
        connection.execute(
            "update mobile_passkey_sessions
             set revoked_at = coalesce(revoked_at, ?1)
             where pairing_token_id = ?2",
            params![timestamp.as_str(), pairing_token_id.as_str()],
        )?;
        Ok(())
    }

    fn connection(&self) -> MobileAuthResult<Connection> {
        self.initialize()?;
        Ok(Connection::open(&self.store_path)?)
    }

    fn prune_stale_connection_orbs(&self) -> MobileAuthResult<()> {
        let cutoff = (OffsetDateTime::now_utc() - Duration::seconds(CONNECTION_ORB_TTL_SECONDS))
            .format(&Rfc3339)?;
        self.connection()?.execute(
            "delete from mobile_connection_orbs
             where created_at < ?1",
            [cutoff],
        )?;
        Ok(())
    }

    fn issue_challenge(
        &self,
        purpose: MobilePasskeyPurpose,
        credential_id: Option<&str>,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeyChallengeResponse> {
        let pairing_token_id =
            normalized_required(pairing_token_id).ok_or(MobileAuthError::PairingTokenRequired)?;
        let row = MobilePasskeyChallengeRow {
            id: new_id(),
            challenge: random_base64_url(TOKEN_RANDOM_BYTE_COUNT),
            purpose: purpose.as_str().to_owned(),
            credential_id: credential_id.map(str::to_owned),
            pairing_token_id,
            created_at: now_iso_string()?,
            consumed_at: None,
        };
        self.connection()?.execute(
            "insert into mobile_passkey_challenges (
                id, challenge, purpose, credential_id, pairing_token_id, created_at, consumed_at
            ) values (?1, ?2, ?3, ?4, ?5, ?6, null)",
            params![
                row.id.as_str(),
                row.challenge.as_str(),
                row.purpose.as_str(),
                row.credential_id.as_deref(),
                row.pairing_token_id.as_str(),
                row.created_at.as_str(),
            ],
        )?;
        Ok(challenge_response(&row))
    }

    fn load_challenge(
        &self,
        challenge_id: &str,
        purpose: MobilePasskeyPurpose,
    ) -> MobileAuthResult<MobilePasskeyChallengeRow> {
        let row = self
            .connection()?
            .query_row(
                "select id, challenge, purpose, credential_id, pairing_token_id, created_at,
                        consumed_at
                 from mobile_passkey_challenges
                 where id = ?1
                   and purpose = ?2
                 limit 1",
                params![challenge_id, purpose.as_str()],
                |row| {
                    Ok(MobilePasskeyChallengeRow {
                        id: row.get(0)?,
                        challenge: row.get(1)?,
                        purpose: row.get(2)?,
                        credential_id: row.get(3)?,
                        pairing_token_id: row.get(4)?,
                        created_at: row.get(5)?,
                        consumed_at: row.get(6)?,
                    })
                },
            )
            .optional()?;
        let Some(row) = row else {
            return Err(MobileAuthError::InvalidChallenge);
        };
        if row.consumed_at.is_some() || !is_challenge_fresh(&row.created_at) {
            return Err(MobileAuthError::InvalidChallenge);
        }
        Ok(row)
    }

    fn consume_challenge_on(connection: &Connection, challenge_id: &str) -> MobileAuthResult<()> {
        let changed = connection.execute(
            "update mobile_passkey_challenges
             set consumed_at = ?1
             where id = ?2
               and consumed_at is null",
            params![now_iso_string()?, challenge_id],
        )?;
        if changed != 1 {
            return Err(MobileAuthError::InvalidChallenge);
        }
        Ok(())
    }

    fn load_credential_for_pairing_token(
        &self,
        credential_id: &str,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeyCredentialRow> {
        let credential = self
            .connection()?
            .query_row(
                "select id, public_key_x963, pairing_token_id, revoked_at
                 from mobile_passkey_credentials
                 where id = ?1
                 limit 1",
                [credential_id],
                |row| {
                    Ok(MobilePasskeyCredentialRow {
                        id: row.get(0)?,
                        public_key_x963: row.get(1)?,
                        pairing_token_id: row.get(2)?,
                        revoked_at: row.get(3)?,
                    })
                },
            )
            .optional()?;
        let Some(credential) = credential else {
            return Err(MobileAuthError::CredentialNotRegistered);
        };
        if credential.revoked_at.is_some() {
            return Err(MobileAuthError::CredentialNotRegistered);
        }
        if credential.pairing_token_id != pairing_token_id {
            return Err(MobileAuthError::CredentialPairingMismatch);
        }
        Ok(credential)
    }

    fn create_passkey_session_on(
        connection: &Connection,
        credential_id: &str,
        pairing_token_id: &str,
    ) -> MobileAuthResult<MobilePasskeySessionResponse> {
        let session_token = random_base64_url(TOKEN_RANDOM_BYTE_COUNT);
        let session = MobilePasskeySessionResponse {
            session_id: new_id(),
            session_token,
            expires_at: (OffsetDateTime::now_utc()
                + Duration::seconds(PASSKEY_SESSION_TTL_SECONDS))
            .format(&Rfc3339)?,
        };
        connection.execute(
            "insert into mobile_passkey_sessions (
                id, token_hash, credential_id, pairing_token_id, created_at, last_used_at,
                expires_at, revoked_at
            ) values (?1, ?2, ?3, ?4, ?5, null, ?6, null)",
            params![
                session.session_id.as_str(),
                hash_token(&session.session_token),
                credential_id,
                pairing_token_id,
                now_iso_string()?,
                session.expires_at.as_str(),
            ],
        )?;
        Ok(session)
    }
}

pub fn parse_mobile_authorization_header(
    authorization_header: Option<&str>,
) -> Option<MobileAuthorizationCredential> {
    let header = authorization_header?;
    let credential = header.strip_prefix(BEARER_PREFIX)?.trim();
    let (id, token) = credential.split_once('.')?;
    let id = normalized_required(id)?;
    let token = normalized_required(token)?;
    Some(MobileAuthorizationCredential { id, token })
}

pub fn mobile_authorization_header(token: &IssuedMobilePairingToken) -> String {
    format!("{BEARER_PREFIX}{}.{}", token.id, token.token)
}

fn mobile_connection_code_from_encoded_payload(
    code: &str,
    orb_id: &str,
    generated_at: &str,
) -> MobileAuthResult<MobileConnectionCode> {
    let payload = decode_base64_url_json::<MobileConnectionCodePayload>(code)?;
    Ok(MobileConnectionCode {
        base_url: payload.base_url,
        base_urls: payload.base_urls,
        pairing_token_id: payload.pairing_token_id,
        pairing_token: payload.pairing_token,
        code: code.to_owned(),
        orb_id: orb_id.to_owned(),
        generated_at: generated_at.to_owned(),
    })
}

fn managed_connection_from_row(row: ManagedMobileConnectionRow) -> ManagedMobileConnection {
    let is_revoked = row.pairing_revoked_at.is_some();
    let active_passkey = row.passkey_credential_id.is_some() && row.passkey_revoked_at.is_none();
    let label = row
        .pairing_label
        .filter(|value| !value.trim().is_empty())
        .or(row.passkey_label.clone())
        .unwrap_or_else(|| DEFAULT_PAIRING_LABEL.to_owned());

    ManagedMobileConnection {
        id: row.pairing_token_id,
        kind: MOBILE_CONNECTION_KIND.to_owned(),
        label,
        status: if is_revoked {
            "revoked"
        } else if active_passkey {
            "secured"
        } else {
            "paired"
        }
        .to_owned(),
        passkey_credential_id: row.passkey_credential_id,
        passkey_label: row.passkey_label,
        created_at: row.passkey_created_at.unwrap_or(row.pairing_created_at),
        last_used_at: row.passkey_last_used_at.or(row.pairing_last_used_at),
        revoked_at: row.pairing_revoked_at.or(row.passkey_revoked_at),
        can_rename: !is_revoked,
        can_revoke: !is_revoked,
    }
}

fn challenge_response(row: &MobilePasskeyChallengeRow) -> MobilePasskeyChallengeResponse {
    MobilePasskeyChallengeResponse {
        challenge_id: row.id.clone(),
        challenge: row.challenge.clone(),
        message: signing_message(row),
    }
}

fn signing_message(row: &MobilePasskeyChallengeRow) -> String {
    format!(
        "{MOBILE_PASSKEY_MESSAGE_PREFIX}\n{}\n{}\n{}",
        row.purpose, row.id, row.challenge
    )
}

fn verify_passkey_signature(
    public_key_x963: &str,
    message: &str,
    signature: &str,
) -> MobileAuthResult<bool> {
    let verifying_key = verifying_key_from_x963(public_key_x963)?;
    let signature = decode_signature(signature)?;
    Ok(verifying_key.verify(message.as_bytes(), &signature).is_ok())
}

fn verifying_key_from_x963(public_key_x963: &str) -> MobileAuthResult<VerifyingKey> {
    let public_key_bytes = decode_base64_url(public_key_x963)?;
    if public_key_bytes.len() != PASSKEY_PUBLIC_KEY_X963_BYTE_COUNT
        || public_key_bytes[0] != PASSKEY_PUBLIC_KEY_UNCOMPRESSED_PREFIX
    {
        return Err(MobileAuthError::InvalidPublicKey);
    }
    VerifyingKey::from_sec1_bytes(&public_key_bytes).map_err(|_| MobileAuthError::InvalidPublicKey)
}

fn decode_signature(signature: &str) -> MobileAuthResult<Signature> {
    let signature_bytes = decode_base64_url(signature)?;
    Signature::from_der(&signature_bytes).map_err(|_| MobileAuthError::InvalidSignature)
}

fn parse_session_header(session_header: Option<&str>) -> Option<MobilePasskeySessionCredential> {
    let header = session_header?.trim();
    let (id, token) = header.split_once(PASSKEY_SESSION_SEPARATOR)?;
    Some(MobilePasskeySessionCredential {
        id: normalized_required(id)?,
        token: normalized_required(token)?,
    })
}

fn is_challenge_fresh(created_at: &str) -> bool {
    let Some(created_at) = parse_iso_timestamp(created_at) else {
        return false;
    };
    OffsetDateTime::now_utc() - created_at <= Duration::seconds(PASSKEY_CHALLENGE_TTL_SECONDS)
}

fn is_connection_orb_fresh(created_at: &str) -> bool {
    let Some(created_at) = parse_iso_timestamp(created_at) else {
        return false;
    };
    OffsetDateTime::now_utc() - created_at <= Duration::seconds(CONNECTION_ORB_TTL_SECONDS)
}

fn is_future_timestamp(value: &str) -> bool {
    parse_iso_timestamp(value).is_some_and(|timestamp| timestamp > OffsetDateTime::now_utc())
}

fn parse_iso_timestamp(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

fn now_iso_string() -> MobileAuthResult<String> {
    Ok(OffsetDateTime::now_utc().format(&Rfc3339)?)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn random_base64_url(byte_count: usize) -> String {
    let mut bytes = Vec::with_capacity(byte_count);
    while bytes.len() < byte_count {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    bytes.truncate(byte_count);
    encode_base64_url(&bytes)
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    encode_base64_url(&hasher.finalize())
}

fn encode_base64_url(value: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(value)
}

fn decode_base64_url(value: &str) -> MobileAuthResult<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| MobileAuthError::InvalidEncoding)
}

fn decode_base64_url_json<T>(value: &str) -> MobileAuthResult<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = decode_base64_url(value)?;
    serde_json::from_slice(&bytes).map_err(MobileAuthError::Json)
}

fn constant_time_equals(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0_u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}

fn normalized_required(value: &str) -> Option<String> {
    normalized_optional(Some(value))
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_owned())
}

use tonic::Status;
use tonic::metadata::MetadataMap;

use crate::control_plane::ControlPlane;
use crate::mobile::auth::{MobileAuthError, parse_mobile_authorization_header};

const AUTHORIZATION_METADATA: &str = "authorization";
const MOBILE_SESSION_METADATA: &str = "x-looper-mobile-session";

pub fn authorize_mobile_api_request(
    control_plane: &ControlPlane,
    metadata: &MetadataMap,
) -> Result<(), Status> {
    let authorization = metadata_value(metadata, AUTHORIZATION_METADATA);
    let credential = parse_mobile_authorization_header(authorization)
        .ok_or_else(|| Status::unauthenticated("pairing token required"))?;
    let service = control_plane.mobile_auth_service();
    let is_authorized = service
        .validate_authorization_credential(Some(&credential))
        .map_err(mobile_auth_status)?;
    if !is_authorized {
        return Err(Status::unauthenticated("pairing token required"));
    }
    service
        .validate_api_session_header(
            metadata_value(metadata, MOBILE_SESSION_METADATA),
            &credential.id,
        )
        .map_err(mobile_auth_status)
}

fn metadata_value<'a>(metadata: &'a MetadataMap, name: &str) -> Option<&'a str> {
    metadata.get(name).and_then(|value| value.to_str().ok())
}

fn mobile_auth_status(error: MobileAuthError) -> Status {
    match error {
        MobileAuthError::PairingTokenRequired | MobileAuthError::PasskeySessionRequired => {
            Status::unauthenticated(error.to_string())
        }
        MobileAuthError::CredentialNotRegistered => Status::not_found(error.to_string()),
        MobileAuthError::ConnectionOrbExpired => Status::deadline_exceeded(error.to_string()),
        MobileAuthError::ConnectionOrbRequired
        | MobileAuthError::ConnectionOrbNotFound
        | MobileAuthError::CredentialRequired
        | MobileAuthError::ChallengePairingMismatch
        | MobileAuthError::ChallengeCredentialMismatch
        | MobileAuthError::CredentialPairingMismatch
        | MobileAuthError::InvalidSignature
        | MobileAuthError::InvalidChallenge
        | MobileAuthError::InvalidEncoding
        | MobileAuthError::InvalidPublicKey
        | MobileAuthError::MissingRequiredValues => Status::invalid_argument(error.to_string()),
        MobileAuthError::Store(_)
        | MobileAuthError::Filesystem(_)
        | MobileAuthError::TimeFormat(_)
        | MobileAuthError::Json(_)
        | MobileAuthError::Orb(_) => Status::internal(error.to_string()),
    }
}

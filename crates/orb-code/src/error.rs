use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrbError {
    #[error("orb id must match orb1_<32 lowercase hex characters>")]
    InvalidOrbId,
    #[error("generate requires exactly one of --id or --data")]
    GenerateInputRequired,
    #[error("image size must be at least 512 pixels")]
    ImageSizeTooSmall,
    #[error("orb payload is too large for this image format")]
    PayloadTooLarge,
    #[error("unsupported orb payload version {found}")]
    UnsupportedPayloadVersion { found: u8 },
    #[error("malformed orb payload")]
    MalformedPayload,
    #[error("no orb marker found in image")]
    OrbNotFound,
    #[error("no orb payload found in image")]
    PayloadNotFound,
    #[error("orb sync pattern could not be found")]
    SyncPatternNotFound,
    #[error("orb payload could not be decoded: {details}")]
    PayloadDecode { details: String },
    #[error("orb payload checksum mismatch")]
    ChecksumMismatch,
    #[error("image could not be decoded: {0}")]
    ImageDecode(#[from] image::ImageError),
    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("PNG encoding failed: {details}")]
    PngEncode { details: String },
    #[error("orb art does not match payload (distance {distance} > threshold {threshold})")]
    VerificationFailed { distance: u32, threshold: u32 },
}

pub type Result<T> = std::result::Result<T, OrbError>;

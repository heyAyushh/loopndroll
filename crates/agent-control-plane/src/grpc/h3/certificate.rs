use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rcgen::{CertifiedKey, generate_simple_self_signed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::control_plane::ControlPlane;

use super::{GrpcH3Certificate, H3_CERT_SERVER_NAME, H3_CERT_STATE_EXTENSION};

#[derive(Serialize, Deserialize)]
struct PersistedGrpcH3Certificate {
    certificate_der_base64: String,
    private_key_der_base64: String,
}

pub fn load_or_create_h3_certificate(control_plane: &ControlPlane) -> Result<GrpcH3Certificate> {
    let path = h3_certificate_state_path(control_plane.store_path());
    if path.exists() {
        let (certificate_der, private_key_der) = load_persisted_h3_certificate(&path)?;
        return Ok(GrpcH3Certificate::new(certificate_der, private_key_der));
    }

    let certificate = generate_h3_certificate()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create H3 certificate state dir {}", parent.display()))?;
    }
    let persisted = PersistedGrpcH3Certificate {
        certificate_der_base64: BASE64.encode(&certificate.certificate_der),
        private_key_der_base64: BASE64.encode(&certificate.private_key_der),
    };
    fs::write(
        &path,
        serde_json::to_vec_pretty(&persisted).context("serialize H3 certificate state")?,
    )
    .with_context(|| format!("write H3 certificate state {}", path.display()))?;
    Ok(certificate)
}

pub fn load_persisted_h3_certificate_sha256(store_path: &Path) -> Result<Option<String>> {
    let path = h3_certificate_state_path(store_path);
    if !path.exists() {
        return Ok(None);
    }
    let (certificate_der, _) = load_persisted_h3_certificate(&path)?;
    Ok(Some(format!("sha256:{}", sha256_hex(&certificate_der))))
}

impl GrpcH3Certificate {
    fn new(certificate_der: Vec<u8>, private_key_der: Vec<u8>) -> Self {
        let certificate_sha256 = format!("sha256:{}", sha256_hex(&certificate_der));
        Self {
            certificate_der,
            private_key_der,
            certificate_sha256,
        }
    }
}

pub(super) fn generate_h3_certificate() -> Result<GrpcH3Certificate> {
    let CertifiedKey { cert, key_pair } =
        generate_simple_self_signed(vec![H3_CERT_SERVER_NAME.to_owned()])
            .context("generate H3 local certificate")?;
    Ok(GrpcH3Certificate::new(
        cert.der().as_ref().to_vec(),
        key_pair.serialize_der(),
    ))
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn h3_certificate_state_path(store_path: &Path) -> PathBuf {
    store_path.with_extension(H3_CERT_STATE_EXTENSION)
}

fn load_persisted_h3_certificate(path: &Path) -> Result<(Vec<u8>, Vec<u8>)> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("read H3 certificate state {}", path.display()))?;
    let persisted: PersistedGrpcH3Certificate = serde_json::from_str(&content)
        .with_context(|| format!("parse H3 certificate state {}", path.display()))?;
    let certificate_der = BASE64
        .decode(persisted.certificate_der_base64)
        .context("decode H3 certificate DER")?;
    let private_key_der = BASE64
        .decode(persisted.private_key_der_base64)
        .context("decode H3 private key DER")?;
    Ok((certificate_der, private_key_der))
}

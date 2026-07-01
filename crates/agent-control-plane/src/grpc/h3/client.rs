use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use anyhow::{Result, anyhow, bail};
use tonic_h3::quinn::h3_quinn::Endpoint;
use tonic_h3::quinn::h3_quinn::quinn::{
    ClientConfig, crypto::rustls::QuicClientConfig, rustls as quinn_rustls,
};

use super::H3_ALPN;
use super::certificate::sha256_hex;

pub(crate) fn pinned_h3_client_endpoint(certificate_sha256: &str) -> Result<Endpoint> {
    let certificate_sha256 = normalized_sha256_pin(certificate_sha256)?;
    let mut endpoint = Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
        .map_err(|error| anyhow!("create H3 client endpoint: {error}"))?;
    let tls_config = pinned_h3_client_tls_config(certificate_sha256)?;
    let quic_config = QuicClientConfig::try_from(tls_config)
        .map_err(|error| anyhow!("configure H3 QUIC client TLS: {error:?}"))?;
    endpoint.set_default_client_config(ClientConfig::new(Arc::new(quic_config)));
    Ok(endpoint)
}

fn pinned_h3_client_tls_config(certificate_sha256: String) -> Result<quinn_rustls::ClientConfig> {
    let provider = quinn_rustls::crypto::ring::default_provider();
    let verifier = Arc::new(PinnedH3CertificateVerifier {
        certificate_sha256,
        supported: provider.signature_verification_algorithms,
    });
    let mut tls_config = quinn_rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&quinn_rustls::version::TLS13])
        .map_err(|error| anyhow!("configure H3 TLS client protocol versions: {error:?}"))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    tls_config.alpn_protocols = vec![H3_ALPN.to_vec()];
    Ok(tls_config)
}

#[derive(Debug)]
struct PinnedH3CertificateVerifier {
    certificate_sha256: String,
    supported: quinn_rustls::crypto::WebPkiSupportedAlgorithms,
}

impl quinn_rustls::client::danger::ServerCertVerifier for PinnedH3CertificateVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &quinn_rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[quinn_rustls::pki_types::CertificateDer<'_>],
        _server_name: &quinn_rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: quinn_rustls::pki_types::UnixTime,
    ) -> std::result::Result<quinn_rustls::client::danger::ServerCertVerified, quinn_rustls::Error>
    {
        let actual = sha256_hex(end_entity.as_ref());
        if self.certificate_sha256 == actual {
            return Ok(quinn_rustls::client::danger::ServerCertVerified::assertion());
        }
        Err(quinn_rustls::Error::InvalidCertificate(
            quinn_rustls::CertificateError::ApplicationVerificationFailure,
        ))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &quinn_rustls::pki_types::CertificateDer<'_>,
        dss: &quinn_rustls::DigitallySignedStruct,
    ) -> std::result::Result<
        quinn_rustls::client::danger::HandshakeSignatureValid,
        quinn_rustls::Error,
    > {
        quinn_rustls::crypto::verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &quinn_rustls::pki_types::CertificateDer<'_>,
        dss: &quinn_rustls::DigitallySignedStruct,
    ) -> std::result::Result<
        quinn_rustls::client::danger::HandshakeSignatureValid,
        quinn_rustls::Error,
    > {
        quinn_rustls::crypto::verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<quinn_rustls::SignatureScheme> {
        self.supported.supported_schemes()
    }
}

fn normalized_sha256_pin(value: &str) -> Result<String> {
    let normalized = value.trim().strip_prefix("sha256:").unwrap_or(value.trim());
    if normalized.len() != 64
        || !normalized
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        bail_invalid_h3_pin()?;
    }
    Ok(normalized.to_ascii_lowercase())
}

fn bail_invalid_h3_pin() -> Result<()> {
    bail!("H3 certificate pin must be sha256-prefixed or raw 64-character hex")
}

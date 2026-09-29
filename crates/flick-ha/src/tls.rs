//! TLS configuration for `wss://` Home Assistant connections.
//!
//! Uses an explicit `ring` provider (the process may link several rustls
//! providers, which breaks auto-detection) and the OS trust store, matching
//! how the Home Assistant Companion apps validate certificates. An optional
//! SHA-256 leaf pin accepts a self-signed certificate the user approved, and an
//! optional client identity answers servers that require mutual TLS.

use std::sync::Arc;

use rustls::{
    ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use rustls_platform_verifier::Verifier;
use sha2::{Digest, Sha256};

use crate::{ClientIdentity, HaError};

/// Builds the rustls client config used for `wss://` URLs.
pub fn client_config(
    cert_sha256: Option<&str>,
    identity: Option<&ClientIdentity>,
) -> Result<Arc<ClientConfig>, HaError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let platform = Verifier::new(provider.clone()).map_err(tls_error)?;
    let verifier = PinningVerifier {
        pin: cert_sha256.and_then(normalize_fingerprint),
        algorithms: provider.signature_verification_algorithms,
        platform,
    };
    let builder = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(tls_error)?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier));
    let config = match identity {
        Some(identity) => builder
            .with_client_auth_cert(identity.chain(), identity.key())
            .map_err(tls_error)?,
        None => builder.with_no_client_auth(),
    };
    Ok(Arc::new(config))
}

fn tls_error(err: TlsError) -> HaError {
    HaError::WebSocket(format!("TLS setup failed: {err}"))
}

fn normalize_fingerprint(value: &str) -> Option<[u8; 32]> {
    let hex: String = value.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    hex::decode(hex).ok()?.try_into().ok()
}

#[derive(Debug)]
struct PinningVerifier {
    pin: Option<[u8; 32]>,
    algorithms: WebPkiSupportedAlgorithms,
    platform: Verifier,
}

impl ServerCertVerifier for PinningVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if let Some(pin) = self.pin
            && Sha256::digest(end_entity.as_ref()).as_slice() == pin
        {
            return Ok(ServerCertVerified::assertion());
        }
        self.platform
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_config_and_parses_pins() {
        assert!(client_config(None, None).is_ok());
        let identity = ClientIdentity::import(
            include_bytes!("../tests/fixtures/mtls/client.p12"),
            Some("flick"),
        );
        assert!(identity.is_ok_and(|id| client_config(None, Some(&id)).is_ok()));
        let pin = "AB:".repeat(31) + "AB";
        assert_eq!(normalize_fingerprint(&pin), Some([0xab; 32]));
        assert_eq!(normalize_fingerprint("abcd"), None);
    }
}

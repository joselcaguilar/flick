//! Client certificates for mutual TLS (mTLS).
//!
//! Some remote setups (Cloudflare mTLS rules, NGINX `ssl_verify_client`) only
//! accept connections that present a client certificate. `wss://` uses the same
//! TLS handshake as HTTPS, so Flick needs the certificate for the WebSocket too.
//! Like the Home Assistant Companion apps, Flick imports a PKCS#12 file
//! (`.p12`/`.pfx`) with its password, and also accepts PEM certificate and key
//! files as issued by Cloudflare. The identity is only sent when the server
//! asks for one during the handshake.

use std::{fmt, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use p12_keystore::{KeyStore, Pkcs12ImportPolicy, error::Error as P12Error};
use rustls::{
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject},
    sign::CertifiedKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x509_parser::{certificate::X509Certificate, prelude::FromDer, x509::X509Name};

/// Why a client certificate could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientCertError {
    /// The PKCS#12 password is wrong.
    #[error("The password for this certificate is wrong.")]
    WrongPassword,
    /// The file is not a certificate Flick can read.
    #[error("{0}")]
    Invalid(String),
    /// The file uses an algorithm or format Flick does not support.
    #[error("{0}")]
    Unsupported(String),
    /// The private key does not belong to any certificate in the file.
    #[error("The private key doesn't match the certificate.")]
    KeyMismatch,
}

impl ClientCertError {
    /// Stable API error code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::WrongPassword => "ha_cert_password",
            Self::Invalid(_) => "ha_cert_invalid",
            Self::Unsupported(_) => "ha_cert_unsupported",
            Self::KeyMismatch => "ha_cert_mismatch",
        }
    }
}

/// Display details of an imported client certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientCertificateInfo {
    /// Subject common name, or the full subject when it has none.
    pub subject: String,
    /// Issuer common name, or the full issuer when it has none.
    pub issuer: String,
    /// Expiry as Unix seconds.
    pub not_after: i64,
    /// SHA-256 fingerprint of the certificate, colon-separated hex.
    pub sha256: String,
}

/// A client certificate chain and its private key.
pub struct ClientIdentity {
    chain: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
    info: ClientCertificateInfo,
}

impl ClientIdentity {
    /// Imports a PKCS#12 bundle or PEM certificate(s) plus private key.
    pub fn import(data: &[u8], password: Option<&str>) -> Result<Self, ClientCertError> {
        if is_pem(data) {
            Self::from_pem(data)
        } else {
            Self::from_pkcs12(data, password.unwrap_or_default())
        }
    }

    /// Imports a PKCS#12 (`.p12`/`.pfx`) bundle.
    pub fn from_pkcs12(data: &[u8], password: &str) -> Result<Self, ClientCertError> {
        let store =
            KeyStore::from_pkcs12(data, password, Pkcs12ImportPolicy::Strict).map_err(p12_error)?;
        let (_, entry) = store.private_key_chain().ok_or_else(|| {
            ClientCertError::Invalid(
                "This file has no private key. Export the certificate together with its key."
                    .to_owned(),
            )
        })?;
        let chain = entry
            .certs()
            .iter()
            .map(|cert| CertificateDer::from(cert.as_der().to_vec()))
            .collect();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(entry.key().as_der().to_vec()));
        Self::new(chain, key)
    }

    /// Imports PEM certificates and one private key, in any order.
    pub fn from_pem(data: &[u8]) -> Result<Self, ClientCertError> {
        if is_encrypted_pem(data) {
            return Err(ClientCertError::Unsupported(
                "Encrypted PEM keys aren't supported. Import a .p12 file with its password instead."
                    .to_owned(),
            ));
        }
        let chain = CertificateDer::pem_slice_iter(data)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                ClientCertError::Invalid(format!("Couldn't read the PEM file: {err}"))
            })?;
        let key = PrivateKeyDer::from_pem_slice(data).map_err(|_| {
            ClientCertError::Invalid(
                "No private key found. Choose the certificate and its private key together."
                    .to_owned(),
            )
        })?;
        Self::new(chain, key)
    }

    fn new(
        chain: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> Result<Self, ClientCertError> {
        if chain.is_empty() {
            return Err(ClientCertError::Invalid(
                "No certificate found. Choose the certificate and its private key together."
                    .to_owned(),
            ));
        }
        let chain = leaf_first(chain, &key, &provider())?;
        let info = describe(&chain[0])?;
        Ok(Self { chain, key, info })
    }

    /// Display details of the leaf certificate.
    #[must_use]
    pub fn info(&self) -> &ClientCertificateInfo {
        &self.info
    }

    /// Certificate chain, leaf first.
    #[must_use]
    pub fn chain(&self) -> Vec<CertificateDer<'static>> {
        self.chain.clone()
    }

    /// Private key.
    #[must_use]
    pub fn key(&self) -> PrivateKeyDer<'static> {
        self.key.clone_key()
    }

    /// Serializes the chain and key as PEM for secure storage.
    #[must_use]
    pub fn to_pem(&self) -> String {
        let mut pem = String::new();
        for cert in &self.chain {
            push_pem(&mut pem, "CERTIFICATE", cert.as_ref());
        }
        let (label, der) = match &self.key {
            PrivateKeyDer::Pkcs1(key) => ("RSA PRIVATE KEY", key.secret_pkcs1_der()),
            PrivateKeyDer::Sec1(key) => ("EC PRIVATE KEY", key.secret_sec1_der()),
            PrivateKeyDer::Pkcs8(key) => ("PRIVATE KEY", key.secret_pkcs8_der()),
            _ => ("PRIVATE KEY", self.key.secret_der()),
        };
        push_pem(&mut pem, label, der);
        pem
    }
}

impl Clone for ClientIdentity {
    fn clone(&self) -> Self {
        Self {
            chain: self.chain.clone(),
            key: self.key.clone_key(),
            info: self.info.clone(),
        }
    }
}

impl fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("info", &self.info)
            .field("chain_len", &self.chain.len())
            .field("key", &"<redacted>")
            .finish()
    }
}

fn provider() -> CryptoProvider {
    rustls::crypto::ring::default_provider()
}

fn is_pem(data: &[u8]) -> bool {
    data.windows(10).any(|window| window == b"-----BEGIN")
}

fn is_encrypted_pem(data: &[u8]) -> bool {
    let text = String::from_utf8_lossy(data);
    text.contains("ENCRYPTED PRIVATE KEY") || text.contains("Proc-Type: 4,ENCRYPTED")
}

fn p12_error(err: P12Error) -> ClientCertError {
    match err {
        P12Error::MacError(_) | P12Error::UnpadError => ClientCertError::WrongPassword,
        P12Error::UnsupportedEncryptionScheme
        | P12Error::UnsupportedMacAlgorithm
        | P12Error::UnsupportedContentType
        | P12Error::UnsupportedCertificateType => ClientCertError::Unsupported(format!(
            "This certificate file uses an unsupported format ({err})."
        )),
        _ => ClientCertError::Invalid(
            "This isn't a certificate file Flick can read. Choose a .p12, .pfx or PEM file."
                .to_owned(),
        ),
    }
}

/// Puts the certificate that matches the key first, as TLS requires.
fn leaf_first(
    mut chain: Vec<CertificateDer<'static>>,
    key: &PrivateKeyDer<'static>,
    provider: &CryptoProvider,
) -> Result<Vec<CertificateDer<'static>>, ClientCertError> {
    let signing_key = provider
        .key_provider
        .load_private_key(key.clone_key())
        .map_err(|err| {
            ClientCertError::Unsupported(format!("This private key type isn't supported ({err})."))
        })?;
    let mut unknown = false;
    for index in 0..chain.len() {
        let candidate = CertifiedKey::new(vec![chain[index].clone()], Arc::clone(&signing_key));
        match candidate.keys_match() {
            Ok(()) => {
                let leaf = chain.remove(index);
                chain.insert(0, leaf);
                return Ok(chain);
            }
            Err(rustls::Error::InconsistentKeys(rustls::InconsistentKeys::Unknown)) => {
                unknown = true;
            }
            Err(rustls::Error::InconsistentKeys(_)) => {}
            Err(err) => {
                return Err(ClientCertError::Invalid(format!(
                    "The certificate couldn't be read ({err})."
                )));
            }
        }
    }
    if unknown {
        Ok(chain)
    } else {
        Err(ClientCertError::KeyMismatch)
    }
}

fn describe(leaf: &CertificateDer<'_>) -> Result<ClientCertificateInfo, ClientCertError> {
    let (_, cert) = X509Certificate::from_der(leaf.as_ref()).map_err(|err| {
        ClientCertError::Invalid(format!("The certificate couldn't be read ({err})."))
    })?;
    let sha256 = Sha256::digest(leaf.as_ref())
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok(ClientCertificateInfo {
        subject: display_name(cert.subject()),
        issuer: display_name(cert.issuer()),
        not_after: cert.validity().not_after.timestamp(),
        sha256,
    })
}

fn display_name(name: &X509Name<'_>) -> String {
    name.iter_common_name()
        .find_map(|cn| cn.as_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| name.to_string())
}

fn push_pem(out: &mut String, label: &str, der: &[u8]) {
    out.push_str("-----BEGIN ");
    out.push_str(label);
    out.push_str("-----\n");
    let encoded = STANDARD.encode(der);
    for line in encoded.as_bytes().chunks(64) {
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CERT: &[u8] = include_bytes!("../tests/fixtures/mtls/client.crt");
    const KEY: &[u8] = include_bytes!("../tests/fixtures/mtls/client.key");
    const OTHER_KEY: &[u8] = include_bytes!("../tests/fixtures/mtls/other.key");
    const P12: &[u8] = include_bytes!("../tests/fixtures/mtls/client.p12");
    const LEGACY_P12: &[u8] = include_bytes!("../tests/fixtures/mtls/client-legacy.p12");

    fn joined(parts: &[&[u8]]) -> Vec<u8> {
        parts.concat()
    }

    #[test]
    fn imports_pem_in_any_order_and_round_trips() {
        let identity = ClientIdentity::import(&joined(&[KEY, CERT]), None).unwrap();
        assert_eq!(identity.info().subject, "Flick Test Client");
        assert_eq!(identity.info().issuer, "Flick Test Client");
        assert_eq!(identity.info().sha256.len(), 95);
        let restored = ClientIdentity::from_pem(identity.to_pem().as_bytes()).unwrap();
        assert_eq!(restored.info(), identity.info());
        assert!(format!("{identity:?}").contains("<redacted>"));
    }

    #[test]
    fn imports_modern_and_legacy_pkcs12() {
        for data in [P12, LEGACY_P12] {
            let identity = ClientIdentity::import(data, Some("flick")).unwrap();
            assert_eq!(identity.info().subject, "Flick Test Client");
            assert_eq!(identity.chain().len(), 1);
        }
    }

    #[test]
    fn reports_wrong_password_mismatch_and_garbage() {
        assert_eq!(
            ClientIdentity::import(P12, Some("nope")).unwrap_err(),
            ClientCertError::WrongPassword
        );
        assert_eq!(
            ClientIdentity::import(&joined(&[CERT, OTHER_KEY]), None).unwrap_err(),
            ClientCertError::KeyMismatch
        );
        assert_eq!(
            ClientIdentity::import(CERT, None).unwrap_err().code(),
            "ha_cert_invalid"
        );
        assert_eq!(
            ClientIdentity::import(b"not a certificate", None)
                .unwrap_err()
                .code(),
            "ha_cert_invalid"
        );
        let encrypted =
            b"-----BEGIN ENCRYPTED PRIVATE KEY-----\nAA==\n-----END ENCRYPTED PRIVATE KEY-----\n";
        assert_eq!(
            ClientIdentity::import(encrypted, None).unwrap_err().code(),
            "ha_cert_unsupported"
        );
    }
}

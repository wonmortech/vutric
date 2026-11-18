//! `LocalSend` HTTPS identity: a self-signed certificate kept across restarts; its SHA-256 is the device's
//! fingerprint (spec §2), which is what a user pins. Peers' certificates are self-signed too, so a client accepts
//! any certificate (the handshake signature is still verified) and reports its fingerprint instead.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use sha2::{Digest, Sha256};

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// SHA-256 of a DER certificate, upper-case hex.
#[must_use]
pub fn fingerprint(cert_der: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(cert_der)
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02X}");
            s
        })
}

/// This device's certificate and key.
#[derive(Debug, Clone)]
pub struct Identity {
    cert: CertificateDer<'static>,
    key: Vec<u8>,
}

impl Identity {
    /// A fresh certificate (tests, or the first start).
    pub fn generate() -> Result<Self> {
        let ck = rcgen::generate_simple_self_signed(vec!["vutric".to_owned()])
            .context("self-signed certificate")?;
        Ok(Self {
            cert: ck.cert.der().clone(),
            key: ck.signing_key.serialize_der(),
        })
    }

    /// Loads `path` (certificate DER, length-prefixed, then the PKCS#8 key) or creates and saves a new one, so
    /// the fingerprint other devices pinned stays the same. A damaged file is replaced.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if let Ok(bytes) = std::fs::read(path)
            && let Some(id) = Self::decode(&bytes)
        {
            return Ok(id);
        }
        let id = Self::generate()?;
        let mut out = Vec::new();
        out.extend_from_slice(&u32::try_from(id.cert.len())?.to_le_bytes());
        out.extend_from_slice(&id.cert);
        out.extend_from_slice(&id.key);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &out).with_context(|| format!("write {}", tmp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(id)
    }

    fn decode(b: &[u8]) -> Option<Self> {
        let n = u32::from_le_bytes(b.get(..4)?.try_into().ok()?) as usize;
        let cert = b.get(4..4usize.checked_add(n)?)?.to_vec();
        let key = b.get(4 + n..)?.to_vec();
        if key.is_empty() {
            return None;
        }
        let id = Self {
            cert: CertificateDer::from(cert),
            key,
        };
        id.server_config().ok()?;
        Some(id)
    }

    #[must_use]
    pub fn fingerprint(&self) -> String {
        fingerprint(&self.cert)
    }

    fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()))
    }

    /// The receiver's TLS config: our certificate; a client certificate is asked for but optional (its hash
    /// is the sender's identity when present).
    pub fn server_config(&self) -> Result<rustls::ServerConfig> {
        let mut c = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_client_cert_verifier(Arc::new(AnyCert(provider())))
            .with_single_cert(vec![self.cert.clone()], self.key())
            .context("certificate and key")?;
        c.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(c)
    }

    /// The sender's TLS config: presents our certificate, accepts the receiver's self-signed one.
    pub fn client_config(&self) -> Result<rustls::ClientConfig> {
        let mut c = rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCert(provider())))
            .with_client_auth_cert(vec![self.cert.clone()], self.key())
            .context("client certificate")?;
        c.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(c)
    }
}

/// Accepts any certificate chain (self-signed by design) but still checks the handshake signature, so the
/// peer really holds the key of the certificate whose fingerprint we report.
#[derive(Debug)]
struct AnyCert(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AnyCert {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

impl rustls::server::danger::ClientCertVerifier for AnyCert {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        false
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<rustls::server::danger::ClientCertVerified, rustls::Error> {
        Ok(rustls::server::danger::ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("vutric-ls-id-{}", std::process::id()));
        let path = dir.join("localsend.id");
        let a = Identity::load_or_create(&path).unwrap();
        let b = Identity::load_or_create(&path).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), 64);
        std::fs::write(&path, b"junk").unwrap();
        let c = Identity::load_or_create(&path).unwrap();
        assert_ne!(
            c.fingerprint(),
            a.fingerprint(),
            "a damaged file is replaced"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

//! The device certificate (self-signed, Common Name = device id, as the spec requires), the TLS configs, and the
//! v8 pairing verification key.
//!
//! Before pairing any certificate is accepted (trust on first use, as the spec says); after pairing the stored
//! certificate must match ([`crate::trust`]). Either way the handshake signature is verified, so the peer holds
//! the key of the certificate we look at. Both sides present a certificate.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, DistinguishedName, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::proto;

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// SHA-256 of a DER certificate, lower-case hex: what the trust file pins.
#[must_use]
pub fn fingerprint(cert_der: &[u8]) -> String {
    hex(&Sha256::digest(cert_der))
}

fn hex(b: &[u8]) -> String {
    use std::fmt::Write as _;
    b.iter()
        .fold(String::with_capacity(b.len() * 2), |mut s, x| {
            let _ = write!(s, "{x:02x}");
            s
        })
}

/// This device: its id, certificate and key, kept across restarts so paired devices keep trusting it.
#[derive(Debug, Clone)]
pub struct Identity {
    pub device_id: String,
    cert: CertificateDer<'static>,
    key: Vec<u8>,
}

const MAGIC: &[u8; 4] = b"KDC1";

impl Identity {
    /// A fresh device id and certificate.
    pub fn generate() -> Result<Self> {
        let device_id = proto::new_device_id();
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new())?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, device_id.as_str());
        params
            .distinguished_name
            .push(rcgen::DnType::OrganizationName, "Vutric");
        params
            .distinguished_name
            .push(rcgen::DnType::OrganizationalUnitName, "KDE Connect adapter");
        let key = rcgen::KeyPair::generate().context("key pair")?;
        let cert = params
            .self_signed(&key)
            .context("self-signed certificate")?;
        Ok(Self {
            device_id,
            cert: cert.der().clone(),
            key: key.serialize_der(),
        })
    }

    /// Loads `path` (`KDC1`, the 32-character id, the certificate length-prefixed, the PKCS#8 key) or creates and
    /// saves a new identity. A damaged file is replaced.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if let Ok(bytes) = std::fs::read(path)
            && let Some(id) = Self::decode(&bytes)
        {
            return Ok(id);
        }
        let id = Self::generate()?;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(id.device_id.as_bytes());
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
        let rest = b.strip_prefix(MAGIC)?;
        let device_id = std::str::from_utf8(rest.get(..32)?).ok()?.to_owned();
        if !proto::valid_device_id(&device_id) {
            return None;
        }
        let rest = &rest[32..];
        let n = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
        let cert = rest.get(4..4usize.checked_add(n)?)?.to_vec();
        let key = rest.get(4 + n..)?.to_vec();
        if key.is_empty() {
            return None;
        }
        let id = Self {
            device_id,
            cert: CertificateDer::from(cert),
            key,
        };
        id.server_config().ok()?;
        Some(id)
    }

    #[must_use]
    pub fn cert(&self) -> &[u8] {
        &self.cert
    }

    fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()))
    }

    /// We are the TLS server (we opened the TCP connection, or a peer fetches a payload from us).
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>> {
        let c = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .with_client_cert_verifier(Arc::new(AnyCert(provider())))
            .with_single_cert(vec![self.cert.clone()], self.key())
            .context("certificate and key")?;
        Ok(Arc::new(c))
    }

    /// We are the TLS client.
    pub fn client_config(&self) -> Result<Arc<rustls::ClientConfig>> {
        let c = rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AnyCert(provider())))
            .with_client_auth_cert(vec![self.cert.clone()], self.key())
            .context("client certificate")?;
        Ok(Arc::new(c))
    }
}

/// The `SubjectPublicKeyInfo` of a DER certificate, as DER (what `getPublicKey().getEncoded()` and
/// `QSslKey::toDer()` return). A small bounded reader: no recursion, no allocation.
pub fn spki(cert: &[u8]) -> Result<&[u8]> {
    let (_, cert_body, _) = tlv(cert, 0x30)?;
    let (_, tbs, _) = tlv(cert_body, 0x30)?;
    let mut rest = tbs;
    if rest.first() == Some(&0xa0) {
        rest = tlv(rest, 0xa0)?.2;
    }
    for tag in [0x02, 0x30, 0x30, 0x30, 0x30] {
        rest = tlv(rest, tag)?.2; // serial, signature algorithm, issuer, validity, subject
    }
    let (whole, _, _) = tlv(rest, 0x30)?;
    Ok(whole)
}

/// One DER element with tag `want`: (the whole element, its contents, what follows).
fn tlv(b: &[u8], want: u8) -> Result<(&[u8], &[u8], &[u8])> {
    if b.first() != Some(&want) {
        bail!("unexpected DER tag");
    }
    let first = *b.get(1).context("short DER")?;
    let (len, head) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 {
            bail!("bad DER length");
        }
        let mut len = 0usize;
        for i in 0..n {
            len = len << 8 | usize::from(*b.get(2 + i).context("short DER")?);
        }
        (len, 2 + n)
    };
    let end = head.checked_add(len).context("DER length")?;
    let whole = b.get(..end).context("short DER")?;
    Ok((whole, &whole[head..], &b[end..]))
}

/// The v8 pairing verification key: the first 8 hex characters (upper case) of SHA-256 over both public keys
/// in byte order, then the pairing timestamp as decimal text. Both screens show it.
pub fn verification_key(ours: &[u8], theirs: &[u8], timestamp: u64) -> Result<String> {
    let (a, b) = (spki(ours)?, spki(theirs)?);
    let (first, second) = if a <= b { (a, b) } else { (b, a) };
    let mut h = Sha256::new();
    h.update(first);
    h.update(second);
    h.update(timestamp.to_string().as_bytes());
    Ok(hex(&h.finalize()[..4]).to_ascii_uppercase())
}

/// Accepts any certificate (self-signed by design) but still checks the handshake signature. Pinning happens
/// after the handshake, against [`crate::trust`].
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
        true
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
    fn identity_keeps_its_id_and_spki_is_found() {
        let dir = std::env::temp_dir().join(format!("vutric-kdc-id-{}", std::process::id()));
        let path = dir.join("kdeconnect.id");
        let a = Identity::load_or_create(&path).unwrap();
        let b = Identity::load_or_create(&path).unwrap();
        assert_eq!(a.device_id, b.device_id);
        assert_eq!(a.cert(), b.cert());
        let spki = spki(a.cert()).unwrap();
        assert_eq!(spki[0], 0x30);
        assert!(
            spki.len() > 60 && spki.len() < 200,
            "P-256 SPKI is 91 bytes: {}",
            spki.len()
        );
        std::fs::write(&path, b"junk").unwrap();
        assert_ne!(
            Identity::load_or_create(&path).unwrap().device_id,
            a.device_id
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn verification_key_is_symmetric_and_depends_on_time() {
        let (a, b) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let k = verification_key(a.cert(), b.cert(), 642_247_200).unwrap();
        assert_eq!(k.len(), 8);
        assert!(
            k.bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
        );
        assert_eq!(
            k,
            verification_key(b.cert(), a.cert(), 642_247_200).unwrap()
        );
        assert_ne!(
            k,
            verification_key(a.cert(), b.cert(), 642_247_201).unwrap()
        );
    }

    #[test]
    fn der_reader_refuses_garbage() {
        for bad in [
            &b""[..],
            &[0x30],
            &[0x30, 0x85, 1, 1, 1, 1, 1],
            &[0x30, 0x10, 0],
        ] {
            assert!(spki(bad).is_err());
        }
    }
}

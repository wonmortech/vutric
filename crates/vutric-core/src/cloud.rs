//! Encrypted cloud handoff, opt-in (backlog R8, ADR-0072): when the two devices cannot reach each other at all,
//! the sender uploads the file **encrypted on the device** to any S3-compatible storage and passes a link. The key
//! is only in the link's fragment, so the storage holds ciphertext it cannot read, and the link expires. Pure:
//! the container format, the link and AWS Signature V4 presigning. The upload and download are in
//! `vutric_net::cloud`.
//!
//! **Container** (`VTCLOUD1`), AES-256-GCM with a one-time random key:
//!
//! ```text
//! header (20 B, the AAD of every chunk): "VTCLOUD1" | chunk_size u32 LE | plain_len u64 LE
//! chunk 0: metadata, postcard `Meta { name, mime }` padded to META_BYTES, + 16 B tag
//! chunk 1..=n: the file in chunk_size pieces (the last may be shorter, or empty for an empty file), + 16 B tag
//! nonce of chunk i: i as u64 LE | 1 if it is the last chunk else 0 | 3 zero bytes
//! ```
//!
//! The key is never reused, so counter nonces are safe. The "last" bit and `plain_len` make truncation and
//! appended data errors. The name is inside the ciphertext, and its length is hidden by the padding.

use std::fmt;

use aes_gcm::aead::{Aead as _, KeyInit as _, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hmac::{Hmac, Mac as _};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::{Error, Result};

/// Magic of the container header.
pub const MAGIC: &[u8; 8] = b"VTCLOUD1";

/// Bytes of the container header.
pub const HEADER_BYTES: usize = 20;

/// Plaintext bytes of the metadata chunk (padded).
pub const META_BYTES: usize = 1024;

/// AES-GCM tag per chunk.
pub const TAG_BYTES: usize = 16;

/// File bytes per chunk.
pub const CHUNK_BYTES: usize = 64 * 1024;

/// Longest presigned download URL in a link.
pub const MAX_URL_BYTES: usize = 4096;

/// The one-time content key. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct ContentKey(pub [u8; 32]);

impl fmt::Debug for ContentKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ContentKey(..)")
    }
}

impl ContentKey {
    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.0).expect("32-byte key")
    }
}

/// What the receiver learns only after decrypting chunk 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// File name, hostile until passed through `filename::sanitize`; at most `message::MAX_FILE_NAME_BYTES`.
    pub name: String,
    pub mime: String,
}

fn nonce(index: u64, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&index.to_le_bytes());
    n[8] = u8::from(last);
    n
}

/// Number of file chunks for `plain_len` bytes (at least one, so an empty file still has a last chunk).
#[must_use]
pub fn data_chunks(plain_len: u64) -> u64 {
    plain_len.div_ceil(CHUNK_BYTES as u64).max(1)
}

/// The container's size for a file of `plain_len` bytes (what the upload's `Content-Length` says).
#[must_use]
pub fn sealed_len(plain_len: u64) -> u64 {
    (HEADER_BYTES + META_BYTES + TAG_BYTES) as u64
        + plain_len
        + data_chunks(plain_len) * TAG_BYTES as u64
}

/// Encrypts one file, chunk by chunk, so a big file never sits in memory.
#[derive(Debug)]
pub struct Sealer {
    key: ContentKey,
    header: [u8; HEADER_BYTES],
    plain_len: u64,
    next: u64,
    sealed: u64,
}

impl Sealer {
    /// Starts a container; returns the sealer and the first bytes to upload (header + sealed metadata).
    pub fn start(key: ContentKey, plain_len: u64, meta: &Meta) -> Result<(Self, Vec<u8>)> {
        if meta.name.is_empty() || meta.name.len() > crate::message::MAX_FILE_NAME_BYTES {
            return Err(Error::Malformed("cloud: file name".into()));
        }
        let mut padded =
            postcard::to_allocvec(meta).map_err(|e| Error::Malformed(e.to_string()))?;
        if padded.len() > META_BYTES {
            return Err(Error::Malformed("cloud: metadata too long".into()));
        }
        padded.resize(META_BYTES, 0);
        let mut header = [0u8; HEADER_BYTES];
        header[..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(&(CHUNK_BYTES as u32).to_le_bytes());
        header[12..].copy_from_slice(&plain_len.to_le_bytes());
        let mut out = header.to_vec();
        out.extend(seal(&key, &header, 0, false, &padded)?);
        Ok((
            Self {
                key,
                header,
                plain_len,
                next: 1,
                sealed: 0,
            },
            out,
        ))
    }

    /// Seals the next piece of the file; every piece but the last must be exactly [`CHUNK_BYTES`].
    pub fn chunk(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        let total = data_chunks(self.plain_len);
        if self.next > total {
            return Err(Error::Malformed("cloud: past the last chunk".into()));
        }
        let last = self.next == total;
        let want = if last {
            self.plain_len - self.sealed
        } else {
            CHUNK_BYTES as u64
        };
        if data.len() as u64 != want {
            return Err(Error::Malformed("cloud: chunk size".into()));
        }
        let out = seal(&self.key, &self.header, self.next, last, data)?;
        self.next += 1;
        self.sealed += want;
        Ok(out)
    }

    /// Whether every chunk was sealed.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.next > data_chunks(self.plain_len)
    }
}

fn seal(key: &ContentKey, header: &[u8], index: u64, last: bool, data: &[u8]) -> Result<Vec<u8>> {
    key.cipher()
        .encrypt(
            Nonce::from_slice(&nonce(index, last)),
            Payload {
                msg: data,
                aad: header,
            },
        )
        .map_err(|_| Error::Malformed("cloud: encrypt".into()))
}

/// Decrypts a container as its bytes arrive. Anything forged, truncated, reordered or appended is an error.
#[derive(Debug)]
pub struct Opener {
    key: ContentKey,
    buf: Vec<u8>,
    header: Option<[u8; HEADER_BYTES]>,
    plain_len: u64,
    meta: Option<Meta>,
    next: u64,
    opened: u64,
    limit: u64,
}

impl Opener {
    /// `limit` is the largest file the receiver accepts (the link's size is checked against it too).
    #[must_use]
    pub fn new(key: ContentKey, limit: u64) -> Self {
        Self {
            key,
            buf: Vec::new(),
            header: None,
            plain_len: 0,
            meta: None,
            next: 0,
            opened: 0,
            limit,
        }
    }

    /// The metadata, once chunk 0 is in.
    #[must_use]
    pub fn meta(&self) -> Option<&Meta> {
        self.meta.as_ref()
    }

    /// The file's size, once the header is in.
    #[must_use]
    pub fn plain_len(&self) -> Option<u64> {
        self.header.map(|_| self.plain_len)
    }

    /// Feeds downloaded bytes; returns the file bytes they completed (possibly none).
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let bad = |w: &str| Err(Error::Malformed(format!("cloud: {w}")));
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let Some(header) = self.header else {
                if self.buf.len() < HEADER_BYTES {
                    return Ok(out);
                }
                let h: [u8; HEADER_BYTES] = self.buf[..HEADER_BYTES].try_into().expect("20 bytes");
                if &h[..8] != MAGIC
                    || u32::from_le_bytes(h[8..12].try_into().expect("4")) as usize != CHUNK_BYTES
                {
                    return bad("not a Vutric container");
                }
                self.plain_len = u64::from_le_bytes(h[12..].try_into().expect("8"));
                if self.plain_len > self.limit {
                    return bad("bigger than allowed");
                }
                self.header = Some(h);
                self.buf.drain(..HEADER_BYTES);
                continue;
            };
            let total = data_chunks(self.plain_len);
            if self.next > total {
                if self.buf.is_empty() {
                    return Ok(out);
                }
                return bad("bytes after the last chunk");
            }
            let plain = if self.next == 0 {
                META_BYTES
            } else if self.next == total {
                (self.plain_len - self.opened) as usize
            } else {
                CHUNK_BYTES
            };
            let need = plain + TAG_BYTES;
            if self.buf.len() < need {
                return Ok(out);
            }
            let last = self.next == total && self.next != 0;
            let Ok(clear) = self.key.cipher().decrypt(
                Nonce::from_slice(&nonce(self.next, last)),
                Payload {
                    msg: &self.buf[..need],
                    aad: &header,
                },
            ) else {
                return bad("wrong key or damaged data");
            };
            self.buf.drain(..need);
            if self.next == 0 {
                let (meta, _): (Meta, _) = postcard::take_from_bytes(&clear)
                    .map_err(|_| Error::Malformed("cloud: metadata".into()))?;
                if meta.name.is_empty() || meta.name.len() > crate::message::MAX_FILE_NAME_BYTES {
                    return bad("file name");
                }
                self.meta = Some(meta);
            } else {
                self.opened += clear.len() as u64;
                out.extend(clear);
            }
            self.next += 1;
        }
    }

    /// Whether the whole file was opened and verified (call after the download ends).
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.header.is_some() && self.next > data_chunks(self.plain_len) && self.buf.is_empty()
    }
}

/// The link the receiver gets: `vutric://cloud?v=1&u=<download url>&x=<expiry>&s=<container bytes>#<key hex>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudLink {
    /// A presigned `GET` URL; `https`, or `http` only to a loopback host (tests, a local `MinIO`).
    pub url: String,
    /// Unix second after which the link is dead (the presigned URL dies at the same time).
    pub expires: u64,
    /// Container size, so the receiver can check room before downloading.
    pub size: u64,
    pub key: ContentKey,
}

fn pct(s: &str, keep: &[u8]) -> String {
    use fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || keep.contains(&b) {
            out.push(b as char);
        } else {
            write!(out, "%{b:02X}").expect("String");
        }
    }
    out
}

fn unpct(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn url_ok(url: &str) -> bool {
    if url.len() > MAX_URL_BYTES || !url.bytes().all(|b| b.is_ascii_graphic()) {
        return false;
    }
    if url.starts_with("https://") {
        return url.len() > 8;
    }
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', '?']).next().unwrap_or_default();
    let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
    host == "localhost"
        || host == "[::1]"
        || host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_loopback())
}

impl CloudLink {
    #[must_use]
    pub fn to_uri(&self) -> String {
        let key: String = self.key.0.iter().fold(String::new(), |mut s, b| {
            use fmt::Write as _;
            write!(s, "{b:02x}").expect("String");
            s
        });
        format!(
            "vutric://cloud?v=1&u={}&x={}&s={}#{key}",
            pct(&self.url, b"-._~"),
            self.expires,
            self.size
        )
    }

    /// Reads a link; `now` refuses an expired one. Any unknown, missing or repeated field is an error.
    pub fn parse(uri: &str, now: u64) -> Result<Self> {
        let bad = |w: &str| Err(Error::Malformed(format!("cloud link: {w}")));
        if uri.len() > MAX_URL_BYTES * 3 + 256 {
            return bad("too long");
        }
        let Some(rest) = uri.strip_prefix("vutric://cloud?") else {
            return bad("not a cloud link");
        };
        let Some((query, frag)) = rest.split_once('#') else {
            return bad("no key");
        };
        if frag.len() != 64 || !frag.bytes().all(|b| b.is_ascii_hexdigit()) {
            return bad("key");
        }
        let mut key = [0u8; 32];
        for (i, k) in key.iter_mut().enumerate() {
            *k = u8::from_str_radix(&frag[2 * i..2 * i + 2], 16)
                .map_err(|_| Error::Malformed("key".into()))?;
        }
        let (mut ver, mut url, mut exp, mut size) = (None, None, None, None);
        for pair in query.split('&') {
            let (k, val) = pair.split_once('=').unwrap_or((pair, ""));
            let slot = match k {
                "v" => &mut ver,
                "u" => &mut url,
                "x" => &mut exp,
                "s" => &mut size,
                _ => return bad("unknown field"),
            };
            if slot.replace(val).is_some() {
                return bad("repeated field");
            }
        }
        if ver != Some("1") {
            return bad("version");
        }
        let url = url.and_then(unpct).filter(|u| url_ok(u));
        let expires = exp.and_then(|x| x.parse::<u64>().ok());
        let size = size.and_then(|s| s.parse::<u64>().ok());
        let (Some(url), Some(expires), Some(size)) = (url, expires, size) else {
            return bad("fields");
        };
        if expires <= now {
            return bad("expired");
        }
        Ok(Self {
            url,
            expires,
            size,
            key: ContentKey(key),
        })
    }
}

/// The user's storage choice, off by default (Settings `cloud`, ADR-0072). Credentials are not here: the host
/// keeps them in the OS keychain and passes them to the upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CloudSettings {
    pub enabled: bool,
    /// `https://s3.eu-central-1.amazonaws.com`, `https://<account>.r2.cloudflarestorage.com`, a `MinIO` URL.
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// `https://endpoint/bucket/key` (`MinIO`, R2) instead of `https://bucket.endpoint/key`.
    pub path_style: bool,
    /// Largest file sent this way.
    pub max_bytes: u64,
    /// How long a link works, 1 ..= 168 hours (a `SigV4` presigned URL lives at most 7 days).
    pub expiry_hours: u32,
}

impl Default for CloudSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            region: "us-east-1".into(),
            bucket: String::new(),
            path_style: true,
            max_bytes: 2 << 30,
            expiry_hours: 24,
        }
    }
}

/// Most a user may set for [`CloudSettings::max_bytes`]: a single S3 `PUT` takes at most 5 GiB.
pub const MAX_CLOUD_BYTES: u64 = 5 << 30;

impl CloudSettings {
    /// Values clamped into range (called by `Settings::migrate`).
    #[must_use]
    pub fn clamped(mut self) -> Self {
        self.expiry_hours = self.expiry_hours.clamp(1, 168);
        self.max_bytes = self.max_bytes.clamp(1, MAX_CLOUD_BYTES);
        self
    }

    /// Whether the handoff can be offered: on, and an endpoint and bucket set.
    #[must_use]
    pub fn ready(&self) -> bool {
        self.enabled
            && url_ok(&format!("{}/", self.endpoint.trim_end_matches('/')))
            && !self.bucket.is_empty()
    }
}

/// S3 credentials (never logged).
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key: String,
    pub secret_key: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Credentials({}, ..)", self.access_key)
    }
}

/// One object request to sign.
#[derive(Debug, Clone)]
pub struct Object<'a> {
    pub endpoint: &'a str,
    pub region: &'a str,
    pub bucket: &'a str,
    pub key: &'a str,
    pub path_style: bool,
}

impl Object<'_> {
    /// `(host, path)` of the object.
    #[must_use]
    pub fn host_path(&self) -> (String, String, String) {
        let (scheme, host) = self
            .endpoint
            .split_once("://")
            .unwrap_or(("https", self.endpoint));
        let host = host.trim_end_matches('/');
        let key = pct(self.key, b"-._~/");
        if self.path_style {
            (
                scheme.into(),
                host.into(),
                format!("/{}/{key}", pct(self.bucket, b"-._~")),
            )
        } else {
            (
                scheme.into(),
                format!("{}.{host}", self.bucket),
                format!("/{key}"),
            )
        }
    }
}

type HmacSha256 = Hmac<Sha256>;

fn hmac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = <HmacSha256 as hmac::Mac>::new_from_slice(key).expect("any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

fn hex(b: &[u8]) -> String {
    b.iter().fold(String::new(), |mut s, b| {
        use fmt::Write as _;
        write!(s, "{b:02x}").expect("String");
        s
    })
}

fn signing_key(secret: &str, date: &str, region: &str) -> [u8; 32] {
    let k = hmac(format!("AWS4{secret}").as_bytes(), date.as_bytes());
    let k = hmac(&k, region.as_bytes());
    let k = hmac(&k, b"s3");
    hmac(&k, b"aws4_request")
}

/// A presigned URL (AWS Signature V4, query parameters, `UNSIGNED-PAYLOAD`) for `method` on `obj`, valid for
/// `expires_secs` from `amz_date` (`YYYYMMDDTHHMMSSZ`).
#[must_use]
pub fn presign(
    method: &str,
    obj: &Object<'_>,
    c: &Credentials,
    amz_date: &str,
    expires_secs: u64,
) -> String {
    let (scheme, host, path) = obj.host_path();
    let date = &amz_date[..8.min(amz_date.len())];
    let scope = format!("{date}/{}/s3/aws4_request", obj.region);
    let mut q = [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
        ("X-Amz-Credential", format!("{}/{scope}", c.access_key)),
        ("X-Amz-Date", amz_date.to_owned()),
        ("X-Amz-Expires", expires_secs.to_string()),
        ("X-Amz-SignedHeaders", "host".to_owned()),
    ];
    q.sort_by(|a, b| a.0.cmp(b.0));
    let query = q
        .iter()
        .map(|(k, v)| format!("{}={}", pct(k, b"-._~"), pct(v, b"-._~")))
        .collect::<Vec<_>>()
        .join("&");
    let canonical = format!("{method}\n{path}\n{query}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD");
    let to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&Sha256::digest(canonical.as_bytes()))
    );
    let sig = hex(&hmac(
        &signing_key(&c.secret_key, date, obj.region),
        to_sign.as_bytes(),
    ));
    format!("{scheme}://{host}{path}?{query}&X-Amz-Signature={sig}")
}

/// `amz_date` (`YYYYMMDDTHHMMSSZ`) for Unix second `now`.
#[must_use]
pub fn amz_date(now: u64) -> String {
    let days = now / 86_400;
    let secs = now % 86_400;
    // Civil from days (Howard Hinnant), proleptic Gregorian, UTC.
    let z = i64::try_from(days).unwrap_or(i64::MAX / 2) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> ContentKey {
        ContentKey([42; 32])
    }

    fn meta() -> Meta {
        Meta {
            name: "Hisobot 2026.pdf".into(),
            mime: "application/pdf".into(),
        }
    }

    fn sealed(data: &[u8]) -> Vec<u8> {
        let (mut s, mut out) = Sealer::start(key(), data.len() as u64, &meta()).unwrap();
        let mut parts: Vec<&[u8]> = data.chunks(CHUNK_BYTES).collect();
        if parts.is_empty() {
            parts.push(&[]);
        }
        for p in parts {
            out.extend(s.chunk(p).unwrap());
        }
        assert!(s.is_done());
        assert_eq!(out.len() as u64, sealed_len(data.len() as u64));
        out
    }

    #[test]
    fn seal_and_open_in_any_slicing() {
        for len in [
            0,
            1,
            CHUNK_BYTES - 1,
            CHUNK_BYTES,
            CHUNK_BYTES + 1,
            3 * CHUNK_BYTES + 7,
        ] {
            let data: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
            let wire = sealed(&data);
            for step in [1usize, 17, 4096, wire.len().max(1)] {
                if step == 1 && len > CHUNK_BYTES {
                    continue;
                }
                let mut o = Opener::new(key(), 1 << 30);
                let mut got = Vec::new();
                for piece in wire.chunks(step) {
                    got.extend(o.push(piece).unwrap());
                }
                assert!(o.is_done(), "len {len} step {step}");
                assert_eq!(got, data);
                assert_eq!(o.meta(), Some(&meta()));
            }
        }
    }

    #[test]
    fn forged_truncated_or_appended_containers_fail() {
        let data = vec![5u8; CHUNK_BYTES + 10];
        let wire = sealed(&data);
        let mut wrong = Opener::new(ContentKey([1; 32]), u64::MAX);
        assert!(wrong.push(&wire).is_err(), "wrong key");
        let mut flipped = wire.clone();
        flipped[HEADER_BYTES + META_BYTES + 40] ^= 1;
        assert!(Opener::new(key(), u64::MAX).push(&flipped).is_err());
        let mut longer = wire.clone();
        longer[19] ^= 0x01; // plain_len in the header is authenticated
        assert!(Opener::new(key(), u64::MAX).push(&longer).is_err());
        let mut o = Opener::new(key(), u64::MAX);
        o.push(&wire[..wire.len() - 1]).unwrap();
        assert!(!o.is_done(), "truncated");
        let mut appended = wire.clone();
        appended.push(0);
        assert!(Opener::new(key(), u64::MAX).push(&appended).is_err());
        assert!(
            Opener::new(key(), 10).push(&wire).is_err(),
            "over the receiver's limit"
        );
        // Chunk 1 and 2 swapped: nonces bind the order.
        let base = HEADER_BYTES + META_BYTES + TAG_BYTES;
        let c1 = wire[base..base + CHUNK_BYTES + TAG_BYTES].to_vec();
        let mut swapped = wire[..base].to_vec();
        swapped.extend_from_slice(&wire[base + CHUNK_BYTES + TAG_BYTES..]);
        swapped.extend(c1);
        assert!(Opener::new(key(), u64::MAX).push(&swapped).is_err());
    }

    #[test]
    fn sealer_refuses_wrong_pieces() {
        let (mut s, _) = Sealer::start(key(), 10, &meta()).unwrap();
        assert!(s.chunk(&[0; 9]).is_err());
        s.chunk(&[0; 10]).unwrap();
        assert!(s.chunk(&[]).is_err(), "past the end");
        let long = Meta {
            name: "a".repeat(600),
            mime: String::new(),
        };
        assert!(Sealer::start(key(), 1, &long).is_err());
        assert_eq!(format!("{:?}", key()), "ContentKey(..)");
    }

    #[test]
    fn links_round_trip_and_refuse_what_they_should() {
        let l = CloudLink {
            url: "https://s3.example.com/b/x?X-Amz-Signature=ab&a=1".into(),
            expires: 2_000,
            size: 1234,
            key: key(),
        };
        let uri = l.to_uri();
        assert!(uri.is_ascii() && uri.ends_with(&"2a".repeat(32)), "{uri}");
        assert_eq!(CloudLink::parse(&uri, 1_000).unwrap(), l);
        assert!(CloudLink::parse(&uri, 2_000).is_err(), "expired");
        assert!(
            CloudLink::parse(uri.split('#').next().unwrap(), 1).is_err(),
            "no key"
        );
        assert!(CloudLink::parse(&uri.replace("v=1", "v=2"), 1).is_err());
        assert!(CloudLink::parse(&uri.replace("&s=", "&s=1&s="), 1).is_err());
        assert!(CloudLink::parse(&uri.replace("&s=", "&z=1&s="), 1).is_err());
        let plain = CloudLink {
            url: "http://evil.example/x".into(),
            ..l.clone()
        };
        assert!(
            CloudLink::parse(&plain.to_uri(), 1).is_err(),
            "plain http only on loopback"
        );
        let local = CloudLink {
            url: "http://127.0.0.1:9000/b/x".into(),
            ..l
        };
        assert!(CloudLink::parse(&local.to_uri(), 1).is_ok());
    }

    #[test]
    fn settings_are_off_and_clamped() {
        let s = CloudSettings::default();
        assert!(!s.enabled && !s.ready());
        let s = CloudSettings {
            enabled: true,
            endpoint: "https://s3.example.com".into(),
            bucket: "vutric".into(),
            expiry_hours: 1000,
            max_bytes: u64::MAX,
            ..CloudSettings::default()
        }
        .clamped();
        assert!(s.ready());
        assert_eq!((s.expiry_hours, s.max_bytes), (168, MAX_CLOUD_BYTES));
    }

    #[test]
    fn amz_dates() {
        assert_eq!(amz_date(1_369_353_600), "20130524T000000Z");
        assert_eq!(amz_date(0), "19700101T000000Z");
        assert_eq!(amz_date(1_795_000_000), "20261118T110640Z");
        assert_eq!(amz_date(951_782_400), "20000229T000000Z");
    }

    /// The presigned-URL example of the AWS documentation ("Authenticating Requests: Using Query Parameters").
    #[test]
    fn presign_matches_the_aws_example() {
        let c = Credentials {
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
        };
        let obj = Object {
            endpoint: "https://s3.amazonaws.com",
            region: "us-east-1",
            bucket: "examplebucket",
            key: "test.txt",
            path_style: false,
        };
        let url = presign("GET", &obj, &c, "20130524T000000Z", 86_400);
        assert!(url.starts_with(
            "https://examplebucket.s3.amazonaws.com/test.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256"
        ));
        assert!(
            url.ends_with(
                "X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"
            ),
            "{url}"
        );
        assert_eq!(format!("{c:?}"), "Credentials(AKIAIOSFODNN7EXAMPLE, ..)");
    }
}

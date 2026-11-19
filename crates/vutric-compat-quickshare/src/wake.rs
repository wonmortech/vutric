//! Sending to Android (backlog C3, ADR-0076). An Android phone only advertises its Quick Share receiver over mDNS
//! after something asks it to. There are two documented ways (`NearDrop` `PROTOCOL.md` at `f75f5fb`, read again
//! 2026-11-19; no code read):
//!
//! 1. **BLE wake-up**: service UUID `0xFE2C` with the service data [`wake_service_data`]. A desktop cannot send it
//!    with `btleplug` (central only, ADR-0071) and macOS has no API for it; a Vutric phone (W6 advertiser) or a
//!    Windows advertiser (W2) can.
//! 2. **QR code**: the sender shows [`QrInvite::url`]; the phone scans it with its camera, starts advertising, and
//!    puts a token derived from the QR key into its endpoint info. [`QrInvite::matches`] recognises it, then
//!    [`send_via_qr`] sends. This works from any desktop with no Bluetooth at all.
//!
//! Experimental, like the rest of this crate, until tested against real phones (backlog M5).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, anyhow, bail};
use data_encoding::BASE64URL_NOPAD;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use ring::agreement::{ECDH_P256, EphemeralPrivateKey};
use ring::hkdf;
use ring::rand::{SecureRandom as _, SystemRandom};

use crate::{client, endpoint};

/// 16-bit service UUID of the BLE wake-up.
pub const WAKE_SERVICE_UUID16: u16 = 0xFE2C;

/// The fixed start of the wake-up's service data; 10 random bytes follow.
pub const WAKE_PREFIX: [u8; 14] = [0xfc, 0x12, 0x8e, 0x01, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Service data bytes of the wake-up (fits a legacy advertisement: 3 flags + 4 header + 24 = 31).
pub const WAKE_SERVICE_DATA_BYTES: usize = 24;

/// The wake-up's service data with these 10 random bytes.
#[must_use]
pub fn wake_service_data(random: [u8; 10]) -> [u8; WAKE_SERVICE_DATA_BYTES] {
    let mut out = [0u8; WAKE_SERVICE_DATA_BYTES];
    out[..14].copy_from_slice(&WAKE_PREFIX);
    out[14..].copy_from_slice(&random);
    out
}

/// A fresh wake-up (new random bytes each time the advertiser restarts).
pub fn wake_service_data_random() -> Result<[u8; WAKE_SERVICE_DATA_BYTES]> {
    let mut r = [0u8; 10];
    SystemRandom::new()
        .fill(&mut r)
        .map_err(|_| anyhow!("random"))?;
    Ok(wake_service_data(r))
}

/// Whether heard service data is a Quick Share wake-up (a phone nearby is looking for receivers).
#[must_use]
pub fn is_wake(service_data: &[u8]) -> bool {
    service_data.len() == WAKE_SERVICE_DATA_BYTES && service_data[..14] == WAKE_PREFIX
}

/// The URL a QR invite encodes.
pub const QR_URL_PREFIX: &str = "https://quickshare.google/qrcode#key=";

/// Endpoint-info record type of the QR answer.
pub const QR_RECORD: u8 = 1;

struct Len16;

impl hkdf::KeyType for Len16 {
    fn len(&self) -> usize {
        16
    }
}

fn derive(ikm: &[u8], info: &[u8]) -> [u8; 16] {
    let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &[]).extract(ikm);
    let okm = prk
        .expand(&[info], Len16)
        .expect("16 bytes is a valid HKDF-SHA256 length");
    let mut out = [0u8; 16];
    okm.fill(&mut out).expect("16 bytes");
    out
}

/// A one-time QR invite: a P-256 public key in the documented form (2-byte version 0, the compressed-point prefix
/// 2 or 3, X), and the two keys derived from it.
#[derive(Clone)]
pub struct QrInvite {
    key: Vec<u8>,
    token: [u8; 16],
    name_key: [u8; 16],
}

impl std::fmt::Debug for QrInvite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QrInvite(..)")
    }
}

/// A receiver that answered the QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrMatch {
    /// Its name, in the clear or decrypted (hidden phones encrypt it to the QR key); display-safe.
    pub name: Option<String>,
}

fn clean(raw: &str) -> Option<String> {
    let c: String = raw
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(64)
        .collect();
    let c = c.trim().to_owned();
    (!c.is_empty()).then_some(c)
}

impl QrInvite {
    /// A fresh invite. The key pair is used only for the QR code, never for the connection.
    pub fn new() -> Result<Self> {
        let rng = SystemRandom::new();
        let private =
            EphemeralPrivateKey::generate(&ECDH_P256, &rng).map_err(|_| anyhow!("P-256 key"))?;
        let public = private
            .compute_public_key()
            .map_err(|_| anyhow!("P-256 public key"))?;
        let p = public.as_ref();
        if p.len() != 65 || p[0] != 4 {
            bail!("unexpected P-256 public key encoding");
        }
        let mut key = vec![0, 0, 2 | (p[64] & 1)];
        key.extend_from_slice(&p[1..33]);
        Ok(Self::from_key(key))
    }

    /// An invite from raw key bytes (tests, or a key decoded from a URL).
    #[must_use]
    pub fn from_key(key: Vec<u8>) -> Self {
        Self {
            token: derive(&key, b"advertisingContext"),
            name_key: derive(&key, b"encryptionKey"),
            key,
        }
    }

    /// The invite read back from its URL.
    #[must_use]
    pub fn from_url(url: &str) -> Option<Self> {
        let b64 = url.strip_prefix(QR_URL_PREFIX)?;
        let key = BASE64URL_NOPAD
            .decode(b64.trim_end_matches('=').as_bytes())
            .ok()?;
        (key.len() == 35).then(|| Self::from_key(key))
    }

    /// What to show as a QR code.
    #[must_use]
    pub fn url(&self) -> String {
        format!("{QR_URL_PREFIX}{}", BASE64URL_NOPAD.encode(&self.key))
    }

    /// The token a visible receiver puts in its endpoint info.
    #[must_use]
    pub fn advertising_token(&self) -> [u8; 16] {
        self.token
    }

    /// The record a receiver that scanned this code would add (visible: the token; hidden: its name sealed with
    /// AES-128-GCM under the name key, the token as AAD). For tests and for our own receiver.
    #[must_use]
    pub fn answer(&self, hidden_name: Option<(&str, [u8; 12])>) -> Vec<u8> {
        let Some((name, iv)) = hidden_name else {
            return self.token.to_vec();
        };
        let key =
            LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &self.name_key).expect("16-byte key"));
        let mut buf = name.as_bytes().to_vec();
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(iv),
            Aad::from(&self.token),
            &mut buf,
        )
        .expect("seal");
        let mut out = iv.to_vec();
        out.extend(buf);
        out
    }

    /// Whether a receiver's endpoint info answers this invite.
    #[must_use]
    pub fn matches(&self, info: &[u8]) -> Option<QrMatch> {
        for (kind, value) in endpoint::records(info) {
            if kind != QR_RECORD {
                continue;
            }
            if value == self.token {
                return Some(QrMatch {
                    name: endpoint::parse(info).and_then(|p| p.name),
                });
            }
            if value.len() > 12 + 16 {
                let (iv, sealed) = value.split_at(12);
                let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &self.name_key).ok()?);
                let nonce = Nonce::try_assume_unique_for_key(iv).ok()?;
                let mut buf = sealed.to_vec();
                if let Ok(plain) = key.open_in_place(nonce, Aad::from(&self.token), &mut buf) {
                    return Some(QrMatch {
                        name: std::str::from_utf8(plain).ok().and_then(clean),
                    });
                }
            }
        }
        None
    }
}

/// A Quick Share receiver heard over mDNS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub addr: SocketAddr,
    pub info: Vec<u8>,
}

/// Listens for Quick Share receivers for `wait` (blocking; run it on a blocking thread).
pub fn browse(wait: Duration) -> Result<Vec<Found>> {
    let daemon = ServiceDaemon::new().context("mDNS daemon")?;
    let rx = daemon
        .browse(endpoint::SERVICE_TYPE)
        .context("mDNS browse")?;
    let end = Instant::now() + wait;
    let mut out: Vec<Found> = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(ServiceEvent::ServiceResolved(s)) => {
                let Some(n) = s.get_property_val_str("n") else {
                    continue;
                };
                let Ok(info) = BASE64URL_NOPAD.decode(n.trim_end_matches('=').as_bytes()) else {
                    continue;
                };
                for ip in s.get_addresses() {
                    let f = Found {
                        addr: SocketAddr::new(ip.to_ip_addr(), s.get_port()),
                        info: info.clone(),
                    };
                    if !out.contains(&f) {
                        out.push(f);
                    }
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    let _ = daemon.shutdown();
    Ok(out)
}

/// Shows nothing itself: the caller displays `invite.url()` as a QR code, then calls this. It browses until a
/// receiver answers the invite (at most `wait`), and sends `paths` to it as `name`.
pub async fn send_via_qr(
    invite: &QrInvite,
    name: &str,
    paths: &[PathBuf],
    wait: Duration,
) -> Result<(client::Sent, QrMatch)> {
    let end = Instant::now() + wait;
    while Instant::now() < end {
        let heard = tokio::task::spawn_blocking(|| browse(Duration::from_secs(2)))
            .await
            .context("browse task")??;
        if let Some((addr, m)) = heard
            .iter()
            .find_map(|f| invite.matches(&f.info).map(|m| (f.addr, m)))
        {
            let sent = client::send(addr, name, paths).await?;
            return Ok((sent, m));
        }
    }
    bail!("no phone scanned the code in time")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_up_bytes() {
        let d = wake_service_data([7; 10]);
        assert_eq!(&d[..5], &[0xfc, 0x12, 0x8e, 0x01, 0x42]);
        assert_eq!(&d[5..14], &[0; 9]);
        assert_eq!(&d[14..], &[7; 10]);
        assert!(is_wake(&d));
        assert!(!is_wake(&d[..23]));
        assert!(is_wake(&wake_service_data_random().unwrap()));
        // Legacy advertisement: flags (3) + service data AD (1 length + 1 type + 2 UUID + 24) = 31.
        const { assert!(3 + 1 + 1 + 2 + WAKE_SERVICE_DATA_BYTES == 31) };
    }

    #[test]
    fn qr_invites_round_trip_and_match_visible_and_hidden_answers() {
        let inv = QrInvite::new().unwrap();
        let url = inv.url();
        assert!(url.starts_with(QR_URL_PREFIX));
        let again = QrInvite::from_url(&url).unwrap();
        assert_eq!(again.advertising_token(), inv.advertising_token());
        let raw = BASE64URL_NOPAD
            .decode(url.trim_start_matches(QR_URL_PREFIX).as_bytes())
            .unwrap();
        assert_eq!(raw.len(), 35);
        assert_eq!(&raw[..2], &[0, 0]);
        assert!(raw[2] == 2 || raw[2] == 3);

        let mut visible = endpoint::info(endpoint::Device::Phone, "Pixel 8");
        let rec = inv.answer(None);
        visible.extend([QR_RECORD, u8::try_from(rec.len()).unwrap()]);
        visible.extend(&rec);
        assert_eq!(
            inv.matches(&visible),
            Some(QrMatch {
                name: Some("Pixel 8".into())
            })
        );

        let mut hidden = vec![0b0001_0010u8]; // hidden, phone
        hidden.extend([0u8; 16]);
        let rec = inv.answer(Some(("Galaxy\u{202e} S24", [5; 12])));
        hidden.extend([QR_RECORD, u8::try_from(rec.len()).unwrap()]);
        hidden.extend(&rec);
        assert_eq!(
            inv.matches(&hidden),
            Some(QrMatch {
                name: Some("Galaxy S24".into())
            })
        );

        let other = QrInvite::new().unwrap();
        assert_eq!(other.matches(&visible), None, "another invite's token");
        assert_eq!(
            other.matches(&hidden),
            None,
            "cannot decrypt another invite's name"
        );
        assert_eq!(
            inv.matches(&endpoint::info(endpoint::Device::Phone, "x")),
            None
        );
        assert!(QrInvite::from_url("https://quickshare.google/qrcode#key=AAAA").is_none());
    }

    #[test]
    fn derivation_is_hkdf_sha256_with_an_empty_salt() {
        // RFC 5869 defines an empty salt as HashLen zero bytes; both must give the same key.
        let ikm = [9u8; 35];
        let a = derive(&ikm, b"advertisingContext");
        let prk = hkdf::Salt::new(hkdf::HKDF_SHA256, &[0u8; 32]).extract(&ikm);
        let mut b = [0u8; 16];
        prk.expand(&[&b"advertisingContext"[..]], Len16)
            .unwrap()
            .fill(&mut b)
            .unwrap();
        assert_eq!(a, b);
        assert_ne!(a, derive(&ikm, b"encryptionKey"));
    }
}

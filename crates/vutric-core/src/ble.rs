//! Bluetooth Low Energy for finding and the handshake only (backlog R4, ADR-0071). Files never go over BLE: it
//! carries a 10-byte advertisement and, over GATT, the pairing link (with its hotspot keys) or a wake-up, at most
//! [`MAX_PAYLOAD_BYTES`]. Then the devices switch to QUIC on the LAN, the hotspot or the relay. Pure: the radio is
//! the host's (`vutric-net` feature `ble` on desktops, W1/W6 on phones).
//!
//! - **Advertisement**: service data for [`SERVICE_UUID`], `[version][flags][rotating id, 8 bytes]` ([`Advert`]),
//!   which fits a legacy 31-byte advertisement next to the flags.
//! - **Rotating id**: BLAKE3 keyed with the device's [`BleKey`] over the 15-minute epoch ([`rotating_id`]). Only a
//!   paired device, which received the key over an authenticated session ([`crate::Message::BleKey`]), can tell
//!   whose it is ([`resolve`]); to everyone else it is a fresh random-looking value every 15 minutes. Never a hash
//!   of a phone number, an e-mail or the endpoint id (the `AirDrop` lesson).
//! - **GATT**: [`BlePayload`] is written to [`INBOX_UUID`] or read from [`OUTBOX_UUID`] in chunks
//!   ([`chunks`], [`Reassembler`]) that fit the link's ATT MTU.
//! - **Wi-Fi Aware / Direct** (W6) reuse [`LinkHello`] as the discovery message (it fits Aware's 255-byte
//!   service-specific info).

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::trust::Admission;
use crate::{EndpointId, Error, Message, Result};

/// The Vutric GATT service and advertisement UUID (random, version 4), as written: `fc1be29b-cb87-4116-a6ba-5a8e1c9d918b`.
pub const SERVICE_UUID: u128 = 0xfc1b_e29b_cb87_4116_a6ba_5a8e_1c9d_918b;

/// Characteristic the other device writes a [`BlePayload`] to (write with response).
pub const INBOX_UUID: u128 = 0x0f34_b527_843f_43b5_8ceb_856b_e921_f7b5;

/// Characteristic the other device reads (or subscribes to) for this device's [`BlePayload`].
pub const OUTBOX_UUID: u128 = 0x2fff_dfd4_abf9_4d7a_a6aa_3779_58c9_f563;

/// Version in the first byte of [`Advert`]; other versions are ignored, never guessed at.
pub const ADVERT_VERSION: u8 = 1;

/// Bytes of [`Advert::encode`].
pub const ADVERT_BYTES: usize = 10;

/// Length of one rotating-id epoch.
pub const EPOCH_SECS: u64 = 15 * 60;

/// Largest [`BlePayload`] on the wire (backlog R4: ≤ 64 KB).
pub const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// Longest pairing link in a [`BlePayload::Link`].
pub const MAX_LINK_BYTES: usize = 2048;

/// Smallest ATT MTU (BLE 4.0); a chunk is at most `mtu - 3` bytes.
pub const MIN_MTU: usize = 23;

/// Header of every GATT chunk: index and count, both `u16` little-endian.
pub const CHUNK_HEADER: usize = 4;

/// A device's key for rotating ids: 16 random-looking bytes derived from its identity, given only to paired devices.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BleKey(pub [u8; 16]);

impl fmt::Debug for BleKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BleKey(..)")
    }
}

impl BleKey {
    /// The key of the device whose identity secret is `secret` (stable, so it never needs storing).
    #[must_use]
    pub fn from_identity(secret: &[u8; 32]) -> Self {
        let full = blake3::derive_key("vutric/1 ble rotating id key", secret);
        let mut k = [0u8; 16];
        k.copy_from_slice(&full[..16]);
        Self(k)
    }

    #[must_use]
    pub fn to_hex(&self) -> String {
        use core::fmt::Write as _;
        let mut s = String::with_capacity(32);
        for b in self.0 {
            write!(s, "{b:02x}").expect("writing to a String cannot fail");
        }
        s
    }

    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 32 || !s.is_ascii() {
            return None;
        }
        let mut k = [0u8; 16];
        for (i, b) in k.iter_mut().enumerate() {
            *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(Self(k))
    }
}

impl Serialize for BleKey {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.serialize_str(&self.to_hex())
        } else {
            self.0.serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for BleKey {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        if d.is_human_readable() {
            let s = String::deserialize(d)?;
            Self::from_hex(&s).ok_or_else(|| serde::de::Error::custom("ble key: 32 hex characters"))
        } else {
            <[u8; 16]>::deserialize(d).map(Self)
        }
    }
}

/// The epoch of Unix second `now`.
#[must_use]
pub fn epoch(now: u64) -> u64 {
    now / EPOCH_SECS
}

/// The rotating id `key` advertises during `epoch`.
#[must_use]
pub fn rotating_id(key: &BleKey, epoch: u64) -> [u8; 8] {
    let mut k = [0u8; 32];
    k[..16].copy_from_slice(&key.0);
    k[16..].copy_from_slice(&blake3::hash(b"vutric/1 ble rid").as_bytes()[..16]);
    let h = blake3::keyed_hash(&k, &epoch.to_le_bytes());
    let mut rid = [0u8; 8];
    rid.copy_from_slice(&h.as_bytes()[..8]);
    rid
}

/// Which paired device advertises `rid` at Unix second `now`, trying the epoch before and after too (clocks
/// drift). `None` for a stranger, or a device whose key we do not have.
pub fn resolve<'a>(
    rid: &[u8; 8],
    now: u64,
    keys: impl IntoIterator<Item = (EndpointId, &'a BleKey)>,
) -> Option<EndpointId> {
    let e = epoch(now);
    keys.into_iter().find_map(|(id, key)| {
        [e.saturating_sub(1), e, e + 1]
            .iter()
            .any(|&ep| &rotating_id(key, ep) == rid)
            .then_some(id)
    })
}

/// Whether a host should take a [`Message::BleKey`] from a peer with `admission`: only from a paired device.
#[must_use]
pub fn accepts_key(msg: &Message, admission: Admission) -> bool {
    matches!(msg, Message::BleKey { .. }) && admission == Admission::Trusted
}

/// Bits of [`Advert::flags`]; unknown bits are 0 when sent and ignored when read.
pub mod flag {
    /// The advertiser is a phone.
    pub const PHONE: u8 = 1 << 0;
    /// Its pairing window is open: the rotating id is a fresh random value, and strangers may ask for its link.
    pub const PAIRING: u8 = 1 << 1;
    /// It has something to send and asks the other side to wake up its Vutric (open a session).
    pub const WAKE: u8 = 1 << 2;
}

/// The service data of a Vutric advertisement.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advert {
    pub flags: u8,
    pub rid: [u8; 8],
}

impl Advert {
    #[must_use]
    pub fn encode(&self) -> [u8; ADVERT_BYTES] {
        let mut out = [0u8; ADVERT_BYTES];
        out[0] = ADVERT_VERSION;
        out[1] = self.flags & (flag::PHONE | flag::PAIRING | flag::WAKE);
        out[2..].copy_from_slice(&self.rid);
        out
    }

    /// Reads service data. Any other version or length is `None`: a newer phone is ignored, not misread.
    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != ADVERT_BYTES || data[0] != ADVERT_VERSION {
            return None;
        }
        let mut rid = [0u8; 8];
        rid.copy_from_slice(&data[2..]);
        Some(Self {
            flags: data[1] & (flag::PHONE | flag::PAIRING | flag::WAKE),
            rid,
        })
    }

    #[must_use]
    pub fn has(&self, f: u8) -> bool {
        self.flags & f != 0
    }
}

/// What goes over GATT, either way. Unauthenticated, like the beacon: a link is dialed and then checked by QUIC
/// (and, for a stranger, the six digits or the link's one-time secret).
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlePayload {
    /// A pairing link (`vutric://pair?…`), with hotspot keys and a one-time secret when the sender has them
    /// (ADR-0058, ADR-0064). The receiver parses it with `PairingTicket::parse`.
    Link { uri: String },
    /// "Open Vutric and connect to me": the receiver's app wakes, opens a session to a trusted device it already
    /// knows (by the rotating id), and the sender sends over that.
    Wake,
}

impl BlePayload {
    /// Bytes for [`chunks`]. Checked first, so an oversized link fails on the sender.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        postcard::to_allocvec(self).map_err(|e| Error::Malformed(format!("ble payload: {e}")))
    }

    /// Reads one payload; trailing bytes are an error.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PAYLOAD_BYTES {
            return Err(Error::Malformed("ble payload too long".into()));
        }
        let (p, rest): (Self, _) = postcard::take_from_bytes(bytes)
            .map_err(|e| Error::Malformed(format!("ble payload: {e}")))?;
        if !rest.is_empty() {
            return Err(Error::Malformed("ble payload: trailing bytes".into()));
        }
        p.validate()?;
        Ok(p)
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Link { uri }
                if uri.len() > MAX_LINK_BYTES
                    || !uri.starts_with("vutric://")
                    || !uri.bytes().all(|b| b.is_ascii_graphic()) =>
            {
                Err(Error::Malformed("ble link".into()))
            }
            _ => Ok(()),
        }
    }
}

/// Splits `payload` into GATT values of at most `mtu - 3` bytes, each `[index u16][count u16][bytes]`.
pub fn chunks(payload: &[u8], mtu: usize) -> Result<Vec<Vec<u8>>> {
    if mtu < MIN_MTU || payload.len() > MAX_PAYLOAD_BYTES {
        return Err(Error::Malformed("ble chunking: mtu or size".into()));
    }
    let room = mtu.min(517) - 3 - CHUNK_HEADER;
    let parts: Vec<&[u8]> = if payload.is_empty() {
        vec![&[][..]]
    } else {
        payload.chunks(room).collect()
    };
    let count =
        u16::try_from(parts.len()).map_err(|_| Error::Malformed("too many chunks".into()))?;
    Ok(parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut c = Vec::with_capacity(CHUNK_HEADER + p.len());
            c.extend_from_slice(&(i as u16).to_le_bytes());
            c.extend_from_slice(&count.to_le_bytes());
            c.extend_from_slice(p);
            c
        })
        .collect())
}

/// Puts [`chunks`] back together, in any order, refusing anything that does not add up: a changed count, a
/// repeated index with other bytes, or more than [`MAX_PAYLOAD_BYTES`] in total.
#[derive(Debug, Default)]
pub struct Reassembler {
    parts: Vec<Option<Vec<u8>>>,
    have: usize,
    bytes: usize,
}

impl Reassembler {
    /// Takes one chunk; `Some(payload)` once the last one is in. After an error, start a new reassembler.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Option<Vec<u8>>> {
        let bad = |w: &str| Err(Error::Malformed(format!("ble chunk: {w}")));
        if chunk.len() < CHUNK_HEADER {
            return bad("short");
        }
        let index = usize::from(u16::from_le_bytes([chunk[0], chunk[1]]));
        let count = usize::from(u16::from_le_bytes([chunk[2], chunk[3]]));
        let body = &chunk[CHUNK_HEADER..];
        if count == 0
            || index >= count
            || count > MAX_PAYLOAD_BYTES / (MIN_MTU - 3 - CHUNK_HEADER) + 1
        {
            return bad("index");
        }
        if self.parts.is_empty() {
            self.parts = vec![None; count];
        } else if self.parts.len() != count {
            return bad("count changed");
        }
        match &self.parts[index] {
            Some(old) if old == body => return Ok(None),
            Some(_) => return bad("index repeated"),
            None => {}
        }
        self.bytes += body.len();
        if self.bytes > MAX_PAYLOAD_BYTES {
            return bad("too long");
        }
        self.parts[index] = Some(body.to_vec());
        self.have += 1;
        if self.have < count {
            return Ok(None);
        }
        Ok(Some(self.parts.drain(..).flatten().flatten().collect()))
    }
}

/// The discovery message of Wi-Fi Aware and Wi-Fi Direct (W6), and what a GATT client may read to dial at once:
/// who I am and where my QUIC listens on that link. Unauthenticated: QUIC checks the key.
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkHello {
    pub id: EndpointId,
    pub port: u16,
    /// [`flag`] bits.
    pub flags: u8,
}

/// Bytes of [`LinkHello::encode`]: version, flags, port, id.
pub const LINK_HELLO_BYTES: usize = 36;

impl LinkHello {
    #[must_use]
    pub fn encode(&self) -> [u8; LINK_HELLO_BYTES] {
        let mut out = [0u8; LINK_HELLO_BYTES];
        out[0] = ADVERT_VERSION;
        out[1] = self.flags & (flag::PHONE | flag::PAIRING | flag::WAKE);
        out[2..4].copy_from_slice(&self.port.to_le_bytes());
        out[4..].copy_from_slice(&self.id.0);
        out
    }

    #[must_use]
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() != LINK_HELLO_BYTES || data[0] != ADVERT_VERSION {
            return None;
        }
        let port = u16::from_le_bytes([data[2], data[3]]);
        if port == 0 {
            return None;
        }
        let mut id = [0u8; 32];
        id.copy_from_slice(&data[4..]);
        Some(Self {
            id: EndpointId(id),
            port,
            flags: data[1] & (flag::PHONE | flag::PAIRING | flag::WAKE),
        })
    }
}

/// Most service-data bytes Android fits in a legacy scan response under a 128-bit UUID (issue #24): 31 − 2 − 16.
pub const MAX_ANDROID_SERVICE_DATA: usize = 13;

const _: () = assert!(ADVERT_BYTES <= MAX_ANDROID_SERVICE_DATA);

/// What a paired phone advertises at `now_ms` (Unix milliseconds), for the Android advertiser (issue #24): the
/// service data for [`SERVICE_UUID`], and the milliseconds until the next epoch, when it must ask again (and
/// Android should rotate its random address at the same moment).
#[must_use]
pub fn advert_now(identity_secret: &[u8; 32], now_ms: u64, flags: u8) -> (Advert, u64) {
    let e = epoch(now_ms / 1000);
    let rid = rotating_id(&BleKey::from_identity(identity_secret), e);
    let next_ms = (e + 1) * EPOCH_SECS * 1000;
    (Advert { flags, rid }, next_ms.saturating_sub(now_ms).max(1))
}

/// The advert while the pairing window is open: a fresh random id (never the keyed one) and the pairing flag, so
/// strangers can find the device without being able to follow it later.
#[must_use]
pub fn pairing_advert(random: [u8; 8], flags: u8) -> Advert {
    Advert {
        flags: flags | flag::PAIRING,
        rid: random,
    }
}

/// Matches heard service data against the trust list (issue #24): `Some(id)` for a paired device whose key we
/// hold, `None` for a stranger or anything that is not a Vutric advert.
#[must_use]
pub fn resolve_heard(
    service_data: &[u8],
    trust: &crate::trust::TrustStore,
    now: u64,
) -> Option<EndpointId> {
    let a = Advert::decode(service_data)?;
    resolve(&a.rid, now, trust.ble_keys())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const T: u64 = 1_795_000_000;

    #[test]
    fn android_helpers_fit_and_resolve() {
        let secret = [8u8; 32];
        let now_ms = T * 1000 + 1234;
        let (a, next) = advert_now(&secret, now_ms, flag::PHONE);
        assert_eq!(
            a.rid,
            rotating_id(&BleKey::from_identity(&secret), epoch(T))
        );
        assert!(next > 0 && next <= EPOCH_SECS * 1000);
        assert_eq!(
            epoch((now_ms + next) / 1000),
            epoch(T) + 1,
            "asks again at the next epoch"
        );
        assert!(a.encode().len() <= MAX_ANDROID_SERVICE_DATA);

        let mut trust = crate::trust::TrustStore::default();
        let id = EndpointId([3; 32]);
        trust.trust(id, "Pixel", None, crate::trust::PairMethod::Ticket, T);
        assert_eq!(resolve_heard(&a.encode(), &trust, T), None, "no key yet");
        assert!(trust.set_ble_key(&id, BleKey::from_identity(&secret)));
        assert_eq!(resolve_heard(&a.encode(), &trust, T), Some(id));
        assert_eq!(resolve_heard(&a.encode()[..9], &trust, T), None);
        let p = pairing_advert([1; 8], flag::PHONE);
        assert!(p.has(flag::PAIRING) && p.has(flag::PHONE));
        assert_eq!(
            resolve_heard(&p.encode(), &trust, T),
            None,
            "a pairing id resolves to nobody"
        );
    }

    #[test]
    fn advert_fits_a_legacy_advertisement() {
        // flags (3) + service data header (2) + 128-bit UUID (16) + ours = at most 31.
        const {
            assert!(3 + 2 + 16 + ADVERT_BYTES <= 31);
        };
        let a = Advert {
            flags: flag::PHONE | 0x80,
            rid: [7; 8],
        };
        let wire = a.encode();
        assert_eq!(wire[1], flag::PHONE, "unknown bits are not sent");
        assert_eq!(Advert::decode(&wire).unwrap().rid, [7; 8]);
        let mut newer = wire;
        newer[0] = 2;
        assert_eq!(Advert::decode(&newer), None);
        assert_eq!(Advert::decode(&wire[..9]), None);
    }

    #[test]
    fn rotating_ids_change_every_epoch_and_only_friends_resolve_them() {
        let alice = BleKey::from_identity(&[1; 32]);
        let bob = BleKey::from_identity(&[2; 32]);
        assert_ne!(alice, bob);
        let e = epoch(T);
        assert_ne!(rotating_id(&alice, e), rotating_id(&alice, e + 1));
        let rid = rotating_id(&alice, e);
        let keys = [(EndpointId([1; 32]), &alice), (EndpointId([2; 32]), &bob)];
        assert_eq!(resolve(&rid, T, keys), Some(EndpointId([1; 32])));
        assert_eq!(
            resolve(&rid, T + EPOCH_SECS, keys),
            Some(EndpointId([1; 32])),
            "a clock one epoch ahead"
        );
        assert_eq!(
            resolve(&rid, T + 3 * EPOCH_SECS, keys),
            None,
            "an old id stops working"
        );
        assert_eq!(resolve(&rid, T, [(EndpointId([2; 32]), &bob)]), None);
        assert_eq!(format!("{alice:?}"), "BleKey(..)", "never in logs");
    }

    #[test]
    fn keys_round_trip_as_hex_in_json() {
        let k = BleKey::from_identity(&[9; 32]);
        let json = serde_json::to_string(&k).unwrap();
        assert_eq!(json, format!("\"{}\"", k.to_hex()));
        assert_eq!(serde_json::from_str::<BleKey>(&json).unwrap(), k);
        assert!(serde_json::from_str::<BleKey>("\"zz\"").is_err());
        assert_eq!(BleKey::from_hex(&"a".repeat(31)), None);
    }

    #[test]
    fn only_paired_devices_hand_over_their_key() {
        let m = Message::BleKey {
            key: BleKey([1; 16]),
        };
        assert!(accepts_key(&m, Admission::Trusted));
        assert!(!accepts_key(&m, Admission::Unknown));
    }

    #[test]
    fn payloads_are_checked() {
        let link = BlePayload::Link {
            uri: "vutric://pair?v=2&id=ab".into(),
        };
        assert_eq!(BlePayload::decode(&link.encode().unwrap()).unwrap(), link);
        assert_eq!(
            BlePayload::decode(&BlePayload::Wake.encode().unwrap()).unwrap(),
            BlePayload::Wake
        );
        for bad in [
            "https://x",
            "vutric://a b",
            &format!("vutric://{}", "a".repeat(MAX_LINK_BYTES)),
        ] {
            assert!(
                BlePayload::Link { uri: bad.into() }.encode().is_err(),
                "{bad}"
            );
        }
        let mut wire = BlePayload::Wake.encode().unwrap();
        wire.push(0);
        assert!(BlePayload::decode(&wire).is_err());
    }

    #[test]
    fn link_hello_round_trips() {
        let h = LinkHello {
            id: EndpointId([5; 32]),
            port: 47_110,
            flags: flag::PHONE,
        };
        assert_eq!(LinkHello::decode(&h.encode()), Some(h));
        const {
            assert!(
                LINK_HELLO_BYTES <= 255,
                "fits Wi-Fi Aware service-specific info"
            );
        };
        let mut zero = h.encode();
        zero[2..4].copy_from_slice(&[0, 0]);
        assert_eq!(LinkHello::decode(&zero), None);
    }

    #[test]
    fn reassembler_refuses_what_does_not_add_up() {
        let c = chunks(&[1; 100], MIN_MTU).unwrap();
        let mut r = Reassembler::default();
        assert!(r.push(&c[0]).unwrap().is_none());
        assert!(
            r.push(&c[0]).unwrap().is_none(),
            "a repeat of the same chunk is fine"
        );
        let mut other = c[0].clone();
        *other.last_mut().unwrap() = 9;
        assert!(r.push(&other).is_err());
        let mut r = Reassembler::default();
        r.push(&c[0]).unwrap();
        let mut grown = c[1].clone();
        grown[2] = grown[2].wrapping_add(1);
        assert!(r.push(&grown).is_err(), "count changed");
        assert!(Reassembler::default().push(&[0, 0]).is_err());
        assert!(
            Reassembler::default().push(&[1, 0, 1, 0]).is_err(),
            "index past count"
        );
        assert!(chunks(&vec![0; MAX_PAYLOAD_BYTES + 1], 247).is_err());
        assert!(chunks(&[0; 10], MIN_MTU - 1).is_err());
    }

    proptest! {
        #[test]
        fn any_payload_survives_chunking_in_any_order(
            payload in proptest::collection::vec(any::<u8>(), 0..4096),
            mtu in MIN_MTU..600usize,
            seed in any::<u64>(),
        ) {
            let mut c = chunks(&payload, mtu).unwrap();
            prop_assert!(c.iter().all(|x| x.len() <= mtu - 3));
            // A deterministic shuffle, and every chunk sent twice (GATT retries).
            let n = c.len();
            for i in (1..n).rev() {
                let j = (seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(i as u64) % (i as u64 + 1)) as usize;
                c.swap(i, j);
            }
            let mut r = Reassembler::default();
            let mut out = None;
            for x in c.iter().chain(c.iter()) {
                if let Some(p) = r.push(x).unwrap() {
                    out = Some(p);
                    break;
                }
            }
            prop_assert_eq!(out, Some(payload));
        }

        #[test]
        fn rotating_ids_never_resolve_to_a_stranger(seed in any::<[u8; 32]>(), other in any::<[u8; 32]>(), now in 0u64..u64::MAX / 2) {
            prop_assume!(seed != other);
            let mine = BleKey::from_identity(&seed);
            let theirs = BleKey::from_identity(&other);
            let rid = rotating_id(&mine, epoch(now));
            prop_assert_eq!(resolve(&rid, now, [(EndpointId([1; 32]), &mine)]), Some(EndpointId([1; 32])));
            prop_assert_eq!(resolve(&rid, now, [(EndpointId([2; 32]), &theirs)]), None);
        }

        #[test]
        fn adverts_and_hellos_never_panic(data in proptest::collection::vec(any::<u8>(), 0..64)) {
            if let Some(a) = Advert::decode(&data) {
                prop_assert_eq!(Advert::decode(&a.encode()), Some(a));
            }
            if let Some(h) = LinkHello::decode(&data) {
                prop_assert_eq!(LinkHello::decode(&h.encode()), Some(h));
            }
            let _ = BlePayload::decode(&data);
        }
    }
}

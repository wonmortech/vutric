//! How a Quick Share receiver names itself: the mDNS service name and the "endpoint info" (TXT `n`, and the
//! same bytes inside a connection request). Layout from `NearDrop`'s `PROTOCOL.md`.

use data_encoding::BASE64URL_NOPAD;
use ring::rand::{SecureRandom as _, SystemRandom};

/// mDNS service type: the first 6 bytes of SHA-256("NearbySharing") in hex.
pub const SERVICE_TYPE: &str = "_FC9F5ED42C8A._tcp.local.";
const SERVICE_ID: [u8; 3] = [0xFC, 0x9F, 0x5E];
/// Longest device name in the endpoint info (one length byte).
pub const MAX_NAME: usize = 255;

/// Device types the other side uses to pick an icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Unknown = 0,
    Phone = 1,
    Tablet = 2,
    Laptop = 3,
}

impl Device {
    fn from_bits(b: u8) -> Self {
        match b {
            1 | 5 => Self::Phone,
            2 => Self::Tablet,
            3 => Self::Laptop,
            _ => Self::Unknown,
        }
    }
}

/// 4 random alphanumeric characters that identify us in this session.
#[must_use]
pub fn random_endpoint_id() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut b = [0u8; 4];
    SystemRandom::new().fill(&mut b).expect("system random");
    b.iter()
        .map(|x| char::from(ALPHABET[usize::from(*x) % ALPHABET.len()]))
        .collect()
}

/// The mDNS instance name for `endpoint_id` (4 ASCII characters).
#[must_use]
pub fn service_name(endpoint_id: &str) -> String {
    let mut b = vec![0x23];
    b.extend(endpoint_id.bytes().take(4));
    b.resize(5, b'0');
    b.extend_from_slice(&SERVICE_ID);
    b.extend_from_slice(&[0, 0]);
    BASE64URL_NOPAD.encode(&b)
}

/// Our endpoint info: visible, `device`, 16 random bytes, the name (cut to 255 bytes on a character boundary).
#[must_use]
pub fn info(device: Device, name: &str) -> Vec<u8> {
    let mut b = vec![(device as u8) << 1];
    let mut salt = [0u8; 16];
    SystemRandom::new().fill(&mut salt).expect("system random");
    b.extend_from_slice(&salt);
    let mut end = name.len().min(MAX_NAME);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    b.push(u8::try_from(end).expect("at most 255"));
    b.extend_from_slice(&name.as_bytes()[..end]);
    b
}

/// The TXT `n` value.
#[must_use]
pub fn txt(info: &[u8]) -> String {
    BASE64URL_NOPAD.encode(info)
}

/// What a sender said about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub device: Device,
    /// `None` when the sender is hidden (its name is not in the clear).
    pub name: Option<String>,
}

/// Reads a sender's endpoint info. The name is made display-safe (no control or bidi-override characters).
#[must_use]
pub fn parse(b: &[u8]) -> Option<Peer> {
    let (&first, rest) = b.split_first()?;
    let rest = rest.get(16..)?;
    let device = Device::from_bits((first >> 1) & 7);
    let hidden = (first >> 4) & 1 == 1;
    let name = if hidden {
        None
    } else {
        let (&n, rest) = rest.split_first()?;
        let raw = std::str::from_utf8(rest.get(..usize::from(n))?).ok()?;
        let clean: String = raw
            .chars()
            .filter(|c| {
                !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
            .collect();
        let clean = clean.trim().to_owned();
        (!clean.is_empty()).then_some(clean)
    };
    Some(Peer { device, name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_info() {
        let n = service_name("AB12");
        let raw = BASE64URL_NOPAD.decode(n.as_bytes()).unwrap();
        assert_eq!(raw, [0x23, b'A', b'B', b'1', b'2', 0xFC, 0x9F, 0x5E, 0, 0]);
        let i = info(Device::Laptop, "Ota kompyuteri");
        assert_eq!(
            parse(&i),
            Some(Peer {
                device: Device::Laptop,
                name: Some("Ota kompyuteri".into())
            })
        );
        let long = "\u{2018}".repeat(200);
        let i = info(Device::Phone, &long);
        assert!(i.len() <= 1 + 16 + 1 + MAX_NAME);
        assert!(
            parse(&i)
                .unwrap()
                .name
                .unwrap()
                .chars()
                .all(|c| c == '\u{2018}')
        );
        let mut evil = info(Device::Phone, "a\u{202e}b\n");
        assert_eq!(parse(&evil).unwrap().name.as_deref(), Some("ab"));
        evil.truncate(18);
        assert_eq!(parse(&evil), None, "length byte with no name");
        assert_eq!(parse(&[0x10; 17]).unwrap().name, None, "hidden");
        assert_eq!(random_endpoint_id().len(), 4);
    }
}

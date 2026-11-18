//! The short connect code (issue #9, ADR-0059): eight characters shown next to the QR, typed when the camera
//! cannot scan it.
//!
//! The code is the first 40 bits of the device's endpoint id in Crockford base32, written `ABCD-EFGH`. It finds
//! the device through the LAN beacon (every answer carries the full id), so nothing new goes on the wire. The
//! id found that way is not authenticated by the code alone, so a code connection always ends in the 6-digit
//! comparison of ADR-0050 before the device is trusted.

use core::fmt;

use crate::EndpointId;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Characters in a code, without the dash.
pub const LEN: usize = 8;

/// The 40-bit prefix of an endpoint id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShortCode([u8; 5]);

impl ShortCode {
    #[must_use]
    pub fn of(id: &EndpointId) -> Self {
        Self(id.0[..5].try_into().expect("5 bytes"))
    }

    /// Whether `id` is the device this code names.
    #[must_use]
    pub fn matches(&self, id: &EndpointId) -> bool {
        id.0[..5] == self.0
    }

    /// Reads what a person typed: any case, dashes and spaces anywhere, `O` for `0`, `I` and `L` for `1`.
    /// `None` unless exactly eight code characters remain.
    #[must_use]
    pub fn parse(typed: &str) -> Option<Self> {
        let mut bits: u64 = 0;
        let mut n = 0;
        for c in typed.chars() {
            let v = match c.to_ascii_uppercase() {
                '-' | ' ' => continue,
                'O' => 0,
                'I' | 'L' => 1,
                c => u64::try_from(ALPHABET.iter().position(|&a| char::from(a) == c)?).ok()?,
            };
            n += 1;
            if n > LEN {
                return None;
            }
            bits = bits << 5 | v;
        }
        if n != LEN {
            return None;
        }
        let b = bits.to_be_bytes();
        Some(Self(b[3..].try_into().expect("5 bytes")))
    }
}

impl fmt::Display for ShortCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = [0u8; 8];
        b[3..].copy_from_slice(&self.0);
        let bits = u64::from_be_bytes(b);
        for i in 0..LEN {
            if i == 4 {
                f.write_str("-")?;
            }
            let v = (bits >> (5 * (LEN - 1 - i))) & 31;
            write!(
                f,
                "{}",
                char::from(ALPHABET[usize::try_from(v).expect("5 bits")])
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_and_reads_back() {
        let id = EndpointId([0xa5; 32]);
        let code = ShortCode::of(&id);
        let shown = code.to_string();
        assert_eq!(shown.len(), 9);
        assert_eq!(&shown[4..5], "-");
        assert_eq!(ShortCode::parse(&shown), Some(code));
        assert!(code.matches(&id));
        assert!(!code.matches(&EndpointId([0xa4; 32])));
    }

    #[test]
    fn known_value() {
        let mut id = [0u8; 32];
        id[..5].copy_from_slice(&[0x00, 0x44, 0x32, 0x14, 0xc7]);
        assert_eq!(ShortCode::of(&EndpointId(id)).to_string(), "0123-4567");
        id[..5].copy_from_slice(&[0xff; 5]);
        assert_eq!(ShortCode::of(&EndpointId(id)).to_string(), "ZZZZ-ZZZZ");
    }

    #[test]
    fn forgiving_typing() {
        let want = ShortCode::parse("0123-4567").unwrap();
        for typed in [
            "01234567",
            "o123 4567",
            "0I23-4567",
            "0l23-4567",
            " 0123 - 4567 ",
        ] {
            assert_eq!(ShortCode::parse(typed), Some(want), "{typed}");
        }
        assert_eq!(ShortCode::parse("abcd-efgh"), ShortCode::parse("ABCDEFGH"));
    }

    #[test]
    fn refuses_wrong_lengths_and_letters() {
        for bad in [
            "",
            "0123-456",
            "0123-45678",
            "0123-456U",
            "0123+4567",
            "\u{e4}\u{e4}\u{e4}\u{e4}-\u{e4}\u{e4}\u{e4}\u{e4}",
        ] {
            assert_eq!(ShortCode::parse(bad), None, "{bad}");
        }
    }
}

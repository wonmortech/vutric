//! Wormhole codes: `<nameplate>-<rest>`, for example `7-4821-0937` (server-protocol.md "Nameplates").
//!
//! The nameplate is up to 40 digits; the whole code is the SPAKE2 password. We generate the rest as two groups of
//! four digits (about 26 bits): language-neutral and easy to read aloud in Uzbek, Russian or English. Codes from
//! the official client (`7-guitarist-revenge`) are accepted as typed.

/// Longest code accepted, in characters.
pub const MAX_CODE: usize = 128;
/// Longest nameplate, in digits (server-protocol.md).
pub const MAX_NAMEPLATE: usize = 40;

#[derive(Clone, PartialEq, Eq)]
pub struct Code(String);

impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Code({}-..)", self.nameplate())
    }
}

impl Code {
    /// Reads a typed code: surrounding spaces dropped, letters lower-cased, inner spaces read as `-`. Refuses a
    /// code without a numeric nameplate, without a rest, or with characters other than ASCII letters, digits and
    /// `-`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_CODE {
            return None;
        }
        let code: String = text
            .chars()
            .map(|c| {
                if c == ' ' {
                    '-'
                } else {
                    c.to_ascii_lowercase()
                }
            })
            .collect();
        if !code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return None;
        }
        let (nameplate, rest) = code.split_once('-')?;
        if nameplate.is_empty()
            || nameplate.len() > MAX_NAMEPLATE
            || !nameplate.bytes().all(|b| b.is_ascii_digit())
            || rest.is_empty()
            || rest.split('-').any(str::is_empty)
        {
            return None;
        }
        Some(Self(code))
    }

    /// A new code on `nameplate` with two random groups of four digits.
    #[must_use]
    pub fn generate(nameplate: &str) -> Self {
        use ring::rand::SecureRandom as _;
        let mut b = [0u8; 8];
        ring::rand::SystemRandom::new()
            .fill(&mut b)
            .expect("OS random generator");
        let n = u64::from_le_bytes(b);
        Self(format!(
            "{nameplate}-{:04}-{:04}",
            n % 10_000,
            (n / 10_000) % 10_000
        ))
    }

    #[must_use]
    pub fn nameplate(&self) -> &str {
        self.0.split_once('-').map_or("", |(n, _)| n)
    }

    /// The whole code, which is also the SPAKE2 password. Show it to the user; never log it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_and_ours() {
        let c = Code::parse(" 7-Guitarist-Revenge ").unwrap();
        assert_eq!(c.as_str(), "7-guitarist-revenge");
        assert_eq!(c.nameplate(), "7");
        assert_eq!(
            Code::parse("12 4821 0937").unwrap().as_str(),
            "12-4821-0937"
        );
        for bad in [
            "", "7", "7-", "-a-b", "x-a-b", "7--a", "7-a-", "7-a/b", "7-ü",
        ] {
            assert!(Code::parse(bad).is_none(), "{bad:?}");
        }
        assert!(Code::parse(&format!("{}-a", "1".repeat(41))).is_none());
        assert!(Code::parse(&format!("1-{}", "a".repeat(130))).is_none());
    }

    #[test]
    fn generated_codes_parse_and_differ() {
        let a = Code::generate("4");
        let b = Code::generate("4");
        assert_eq!(Code::parse(a.as_str()), Some(a.clone()));
        assert_eq!(a.as_str().len(), "4-0000-0000".len());
        assert_ne!(a, b, "two codes in a row collided (1 in 10^8)");
        assert_eq!(format!("{a:?}"), "Code(4-..)");
    }
}

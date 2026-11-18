//! Hotspot details in the pairing QR (backlog R3, ADR-0058).
//!
//! When there is no shared network, one device opens a hotspot (Android `LocalOnlyHotspot`, a Windows mobile
//! hotspot) and its QR carries the network next to the usual pairing link:
//! `vutric://pair?…&ws=<ssid>&wk=<wpa2|wpa3|open>&wp=<password>&wh=1`. Older apps ignore the extra keys and pair
//! as before. [`Wifi::to_wifi_qr`] gives the standard `WIFI:` payload as well, for a phone that joins with its
//! camera app before Vutric is installed.

use crate::pairing::{percent_decode, percent_encode};
use crate::{Error, PairingTicket, Result};

/// How the hotspot is secured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Wpa2,
    Wpa3,
    Open,
}

impl Security {
    fn key(self) -> &'static str {
        match self {
            Security::Wpa2 => "wpa2",
            Security::Wpa3 => "wpa3",
            Security::Open => "open",
        }
    }
}

/// A Wi-Fi network to join. Built only through [`Wifi::new`], so every value is one a phone can join. `Debug`
/// never prints the password, so a logged ticket does not leak it.
#[derive(Clone, PartialEq, Eq)]
pub struct Wifi {
    ssid: String,
    password: Option<String>,
    security: Security,
    hidden: bool,
}

impl core::fmt::Debug for Wifi {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Wifi")
            .field("ssid", &self.ssid)
            .field("password", &self.password.as_ref().map(|_| "<hidden>"))
            .field("security", &self.security)
            .field("hidden", &self.hidden)
            .finish()
    }
}

/// Longest SSID, in bytes (IEEE 802.11).
pub const MAX_SSID_BYTES: usize = 32;

impl Wifi {
    /// Checks the values: an SSID of 1-32 bytes without control characters; for WPA2/WPA3 a passphrase of 8-63
    /// printable ASCII characters or 64 hex digits; for an open network no password.
    pub fn new(
        ssid: &str,
        password: Option<&str>,
        security: Security,
        hidden: bool,
    ) -> Result<Self> {
        if ssid.is_empty() || ssid.len() > MAX_SSID_BYTES || ssid.chars().any(char::is_control) {
            return Err(Error::InvalidTicket("bad hotspot name"));
        }
        let password = match (security, password) {
            (Security::Open, None) => None,
            (Security::Open, Some(_)) => {
                return Err(Error::InvalidTicket("open hotspot with a password"));
            }
            (_, Some(p)) if passphrase_ok(p) => Some(p.to_owned()),
            _ => return Err(Error::InvalidTicket("bad hotspot password")),
        };
        Ok(Self {
            ssid: ssid.to_owned(),
            password,
            security,
            hidden,
        })
    }

    #[must_use]
    pub fn ssid(&self) -> &str {
        &self.ssid
    }

    #[must_use]
    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    #[must_use]
    pub fn security(&self) -> Security {
        self.security
    }

    #[must_use]
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// The standard `WIFI:` QR payload (as camera apps on Android and iPhone read it). `T:WPA` stands for WPA2
    /// and WPA3 personal alike, which is how those readers take it.
    #[must_use]
    pub fn to_wifi_qr(&self) -> String {
        let mut s = String::from("WIFI:T:");
        s.push_str(if self.security == Security::Open {
            "nopass"
        } else {
            "WPA"
        });
        s.push_str(";S:");
        escape_wifi(&self.ssid, &mut s);
        if let Some(p) = &self.password {
            s.push_str(";P:");
            escape_wifi(p, &mut s);
        }
        if self.hidden {
            s.push_str(";H:true");
        }
        s.push_str(";;");
        s
    }

    /// The hotspot in a pairing link, `None` when the link carries none. A link with a half or broken hotspot
    /// part is an error, so the user is not sent to a network that cannot work.
    pub fn in_link(uri: &str) -> Result<Option<Self>> {
        let query = uri
            .strip_prefix("vutric://pair?")
            .ok_or(Error::InvalidTicket("not a vutric pairing link"))?;
        let (mut ssid, mut password, mut security, mut hidden) = (None, None, None, false);
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "ws" => ssid = Some(percent_decode(value)?),
                "wp" => password = Some(percent_decode(value)?),
                "wk" => {
                    security = Some(match value {
                        "wpa2" => Security::Wpa2,
                        "wpa3" => Security::Wpa3,
                        "open" => Security::Open,
                        _ => return Err(Error::InvalidTicket("unknown hotspot security")),
                    });
                }
                "wh" => hidden = value == "1",
                _ => {}
            }
        }
        match (ssid, security) {
            (None, None) if password.is_none() => Ok(None),
            (Some(ssid), Some(security)) => {
                Self::new(&ssid, password.as_deref(), security, hidden).map(Some)
            }
            _ => Err(Error::InvalidTicket("incomplete hotspot")),
        }
    }
}

/// A pairing link together with the hotspot to join first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotspotTicket {
    pub pair: PairingTicket,
    pub wifi: Wifi,
}

impl HotspotTicket {
    /// The pairing link with the hotspot keys added. Still plain ASCII.
    #[must_use]
    pub fn to_uri(&self) -> String {
        let mut uri = self.pair.to_uri();
        uri.push_str("&ws=");
        percent_encode(&self.wifi.ssid, &mut uri);
        uri.push_str("&wk=");
        uri.push_str(self.wifi.security.key());
        if let Some(p) = &self.wifi.password {
            uri.push_str("&wp=");
            percent_encode(p, &mut uri);
        }
        if self.wifi.hidden {
            uri.push_str("&wh=1");
        }
        uri
    }

    /// Parses a pairing link that must carry a hotspot.
    pub fn parse(uri: &str) -> Result<Self> {
        let pair = PairingTicket::parse(uri)?;
        let wifi = Wifi::in_link(uri)?.ok_or(Error::InvalidTicket("no hotspot in the link"))?;
        Ok(Self { pair, wifi })
    }
}

fn passphrase_ok(p: &str) -> bool {
    (8..=63).contains(&p.len()) && p.bytes().all(|b| (0x20..=0x7e).contains(&b))
        || p.len() == 64 && p.bytes().all(|b| b.is_ascii_hexdigit())
}

fn escape_wifi(v: &str, out: &mut String) {
    for c in v.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EndpointId;

    fn pair() -> PairingTicket {
        PairingTicket {
            endpoint: EndpointId([7; 32]),
            device_name: "Telefon".into(),
            addrs: vec!["[fd00::1]:47110".parse().unwrap()],
            relay: None,
        }
    }

    #[test]
    fn round_trip_and_old_parsers_still_pair() {
        let t = HotspotTicket {
            pair: pair(),
            wifi: Wifi::new(
                "AndroidShare_1234",
                Some("p&ss=w;rd ok"),
                Security::Wpa2,
                false,
            )
            .unwrap(),
        };
        let uri = t.to_uri();
        assert!(uri.is_ascii());
        assert_eq!(HotspotTicket::parse(&uri).unwrap(), t);
        assert_eq!(
            PairingTicket::parse(&uri).unwrap(),
            pair(),
            "an older app pairs as before"
        );
        assert_eq!(Wifi::in_link(&pair().to_uri()).unwrap(), None);
        assert!(HotspotTicket::parse(&pair().to_uri()).is_err());
    }

    #[test]
    fn unicode_hidden_and_open() {
        let t = HotspotTicket {
            pair: pair(),
            wifi: Wifi::new("Uy tarmog'i 📶", None, Security::Open, true).unwrap(),
        };
        assert_eq!(HotspotTicket::parse(&t.to_uri()).unwrap(), t);
    }

    #[test]
    fn refuses_networks_a_phone_cannot_join() {
        assert!(Wifi::new("", Some("12345678"), Security::Wpa2, false).is_err());
        assert!(Wifi::new(&"x".repeat(33), Some("12345678"), Security::Wpa2, false).is_err());
        assert!(Wifi::new("a\nb", Some("12345678"), Security::Wpa2, false).is_err());
        assert!(Wifi::new("n", Some("short"), Security::Wpa3, false).is_err());
        assert!(Wifi::new("n", Some(&"x".repeat(64)), Security::Wpa2, false).is_err());
        assert!(
            Wifi::new("n", Some(&"ab".repeat(32)), Security::Wpa2, false).is_ok(),
            "64 hex = raw key"
        );
        assert!(Wifi::new("n", Some("12345678"), Security::Open, false).is_err());
        assert!(Wifi::new("n", None, Security::Wpa2, false).is_err());
        let base = pair().to_uri();
        for bad in [
            "&ws=n",
            "&wk=wpa2",
            "&ws=n&wk=wep",
            "&ws=n&wk=wpa2&wp=short",
            "&wp=12345678",
        ] {
            assert!(Wifi::in_link(&format!("{base}{bad}")).is_err(), "{bad}");
        }
    }

    #[test]
    fn debug_hides_the_password() {
        let w = Wifi::new("n", Some("secret-pass"), Security::Wpa2, false).unwrap();
        let shown = format!("{w:?}");
        assert!(!shown.contains("secret-pass"), "{shown}");
        assert!(shown.contains("<hidden>"));
    }

    #[test]
    fn standard_wifi_qr_escapes() {
        let w = Wifi::new("a;b:c", Some(r#"p\w,"d"x"#), Security::Wpa3, true).unwrap();
        assert_eq!(
            w.to_wifi_qr(),
            r#"WIFI:T:WPA;S:a\;b\:c;P:p\\w\,\"d\"x;H:true;;"#
        );
        let open = Wifi::new("Cafe", None, Security::Open, false).unwrap();
        assert_eq!(open.to_wifi_qr(), "WIFI:T:nopass;S:Cafe;;");
    }
}

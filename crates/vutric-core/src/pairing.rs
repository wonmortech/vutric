//! The one-time pairing payload shown as a QR code by the desktop.
//!
//! Format: `vutric://pair?v=1&id=<64 hex>&n=<percent-encoded name>&a=<ip:port>,<ip:port>&r=<relay url>`
//! Address hints let the phone connect when multicast/mDNS is blocked on the network. The optional relay URL
//! (percent-encoded) lets it connect from another network at once; older links without it still parse, and
//! older apps ignore it.

use core::fmt::Write as _;
use std::net::SocketAddr;

use crate::{EndpointId, Error, PROTOCOL_VERSION, Result};

const PREFIX: &str = "vutric://pair?";
const MAX_ADDRS: usize = 8;
const MAX_NAME_CHARS: usize = 64;
const MAX_RELAY_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairingTicket {
    pub endpoint: EndpointId,
    pub device_name: String,
    pub addrs: Vec<SocketAddr>,
    /// The device's home relay (`https://…`) when it had one: lets a device on another network reach it.
    pub relay: Option<String>,
}

impl PairingTicket {
    #[must_use]
    pub fn to_uri(&self) -> String {
        let mut uri = format!("{PREFIX}v={PROTOCOL_VERSION}&id={}&n=", self.endpoint);
        percent_encode(&self.device_name, &mut uri);
        if !self.addrs.is_empty() {
            uri.push_str("&a=");
            let addrs: Vec<String> = self.addrs.iter().map(ToString::to_string).collect();
            uri.push_str(&addrs.join(","));
        }
        if let Some(relay) = &self.relay {
            uri.push_str("&r=");
            percent_encode(relay, &mut uri);
        }
        uri
    }

    pub fn parse(uri: &str) -> Result<Self> {
        let query = uri
            .strip_prefix(PREFIX)
            .ok_or(Error::InvalidTicket("not a vutric pairing link"))?;
        let (mut version, mut endpoint, mut name, mut addrs) = (None, None, None, Vec::new());
        let mut relay = None;
        for pair in query.split('&') {
            let (key, value) = pair
                .split_once('=')
                .ok_or(Error::InvalidTicket("bad query"))?;
            match key {
                "v" => version = value.parse::<u16>().ok(),
                "id" => endpoint = Some(value.parse::<EndpointId>()?),
                "n" => name = Some(percent_decode(value)?),
                "r" => {
                    let url = percent_decode(value)?;
                    if url.len() > MAX_RELAY_CHARS
                        || !url.starts_with("https://")
                        || url.chars().any(|c| c.is_control() || c.is_whitespace())
                    {
                        return Err(Error::InvalidTicket("bad relay url"));
                    }
                    relay = Some(url);
                }
                "a" => {
                    for addr in value.split(',').filter(|s| !s.is_empty()) {
                        addrs.push(
                            addr.parse::<SocketAddr>()
                                .map_err(|_| Error::InvalidTicket("bad address hint"))?,
                        );
                    }
                }
                _ => {}
            }
        }
        match version {
            Some(PROTOCOL_VERSION) => {}
            Some(other) => return Err(Error::UnsupportedVersion(other)),
            None => return Err(Error::InvalidTicket("missing version")),
        }
        if addrs.len() > MAX_ADDRS {
            return Err(Error::InvalidTicket("too many address hints"));
        }
        let device_name: String = name
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_NAME_CHARS)
            .collect();
        Ok(Self {
            endpoint: endpoint.ok_or(Error::InvalidTicket("missing endpoint id"))?,
            device_name,
            addrs,
            relay,
        })
    }
}

fn percent_encode(input: &str, out: &mut String) {
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            write!(out, "%{byte:02X}").expect("writing to a String cannot fail");
        }
    }
}

fn percent_decode(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = input
                .get(i + 1..i + 3)
                .ok_or(Error::InvalidTicket("truncated escape"))?;
            out.push(u8::from_str_radix(hex, 16).map_err(|_| Error::InvalidTicket("bad escape"))?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::InvalidTicket("name is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> PairingTicket {
        PairingTicket {
            endpoint: EndpointId([0x5a; 32]),
            device_name: "Studio PC — ish".into(),
            addrs: vec![
                "192.168.1.24:47110".parse().unwrap(),
                "[fe80::1]:47110".parse().unwrap(),
            ],
            relay: Some("https://euc1-1.relay.n0.iroh.link./".into()),
        }
    }

    #[test]
    fn round_trips_including_unicode_name_and_ipv6() {
        let uri = ticket().to_uri();
        assert!(uri.is_ascii(), "QR payload must stay ASCII: {uri}");
        assert_eq!(PairingTicket::parse(&uri).unwrap(), ticket());
    }

    #[test]
    fn rejects_foreign_links_and_wrong_versions() {
        assert!(PairingTicket::parse("https://example.com/pair?v=1").is_err());
        let uri = ticket().to_uri().replace("v=1", "v=7");
        assert_eq!(
            PairingTicket::parse(&uri),
            Err(Error::UnsupportedVersion(7))
        );
    }

    #[test]
    fn strips_control_characters_from_name() {
        let uri = format!("{PREFIX}v=1&id={}&n=a%0Ab%1Bc", EndpointId([1; 32]));
        assert_eq!(PairingTicket::parse(&uri).unwrap().device_name, "abc");
    }

    #[test]
    fn relay_is_optional_and_must_be_https() {
        let old = format!("{PREFIX}v=1&id={}&n=x", EndpointId([1; 32]));
        assert_eq!(PairingTicket::parse(&old).unwrap().relay, None);
        let bad = format!("{old}&r=http%3A%2F%2Fevil.example%2F");
        assert!(PairingTicket::parse(&bad).is_err());
    }

    #[test]
    fn missing_id_is_an_error() {
        assert!(PairingTicket::parse(&format!("{PREFIX}v=1&n=x")).is_err());
    }
}

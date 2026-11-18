//! KDE Connect packets (`kdeconnect-meta/protocol.md`, protocol version 8): one JSON object per line.
//!
//! Every parser here takes untrusted bytes. A line is size-checked before it is parsed, only the fields we use
//! are kept, every string is bounded, device names lose the characters the spec forbids, and file names come out
//! of [`vutric_core::filename::sanitize`].

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Protocol version we speak. Version 7 peers are refused (they use a different, weaker TLS order).
pub const PROTOCOL_VERSION: u64 = 8;
/// Longest line read before TLS (the UDP and first TCP identity are well under 1 KiB).
pub const MAX_PREAUTH_LINE: usize = 4096;
/// Longest line read after TLS (a shared text can be long; files travel on their own connection).
pub const MAX_LINE: usize = 1 << 20;
/// Longest device name after cleaning (spec: 1–32 characters).
pub const MAX_NAME: usize = 32;
/// Most capabilities kept from a peer.
pub const MAX_CAPS: usize = 128;
/// Longest shared text or link kept.
pub const MAX_TEXT: usize = 64 * 1024;

pub const IDENTITY: &str = "kdeconnect.identity";
pub const PAIR: &str = "kdeconnect.pair";
pub const SHARE: &str = "kdeconnect.share.request";
pub const SHARE_UPDATE: &str = "kdeconnect.share.request.update";

/// What we announce: only the share plugin (ADR-0010).
pub const CAPABILITIES: [&str; 2] = [SHARE, SHARE_UPDATE];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Phone,
    Tablet,
    Tv,
}

impl DeviceType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Laptop => "laptop",
            Self::Phone => "phone",
            Self::Tablet => "tablet",
            Self::Tv => "tv",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "laptop" => Self::Laptop,
            "phone" => Self::Phone,
            "tablet" => Self::Tablet,
            "tv" => Self::Tv,
            _ => Self::Desktop,
        }
    }
}

/// A packet as read from the wire, before its body is interpreted.
#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    pub kind: String,
    pub body: Map<String, Value>,
    pub payload_size: Option<u64>,
    /// `payloadTransferInfo.port`.
    pub payload_port: Option<u16>,
}

#[derive(Deserialize)]
struct Raw {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    body: Map<String, Value>,
    #[serde(rename = "payloadSize", default)]
    payload_size: Option<Value>,
    #[serde(rename = "payloadTransferInfo", default)]
    payload_transfer_info: Option<Map<String, Value>>,
}

impl Packet {
    /// Parses one line (without its newline). `None` for anything that is not a well-formed packet within `max`.
    #[must_use]
    pub fn parse(line: &[u8], max: usize) -> Option<Self> {
        if line.len() > max {
            return None;
        }
        let raw: Raw = serde_json::from_slice(line).ok()?;
        if raw.kind.len() > 64
            || !raw.kind.starts_with("kdeconnect.")
            || !raw
                .kind
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_')
        {
            return None;
        }
        let payload_size = match raw.payload_size {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.as_u64()?),
        };
        let payload_port = match raw.payload_transfer_info {
            None => None,
            Some(m) => match m.get("port") {
                None => None,
                Some(p) => Some(u16::try_from(p.as_u64()?).ok().filter(|p| *p != 0)?),
            },
        };
        Some(Self {
            kind: raw.kind,
            body: raw.body,
            payload_size,
            payload_port,
        })
    }

    fn str(&self, key: &str, max: usize) -> Option<String> {
        self.body
            .get(key)?
            .as_str()
            .map(|s| s.chars().take(max).collect())
    }

    fn u64(&self, key: &str) -> Option<u64> {
        self.body.get(key)?.as_u64()
    }
}

/// One line of JSON plus the newline.
fn line(kind: &str, body: &Value, payload: Option<(u64, u16)>) -> Vec<u8> {
    let mut v = json!({ "id": 0, "type": kind, "body": body });
    if let Some((size, port)) = payload {
        v["payloadSize"] = json!(size);
        v["payloadTransferInfo"] = json!({ "port": port });
    }
    let mut out = serde_json::to_vec(&v).expect("a JSON value serialises");
    out.push(b'\n');
    out
}

/// Whether a device id is one we accept: 32–38 of `[A-Za-z0-9_-]` (spec, v8).
#[must_use]
pub fn valid_device_id(id: &str) -> bool {
    (32..=38).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A new device id: 32 lower-case hex characters (a `UUIDv4` without hyphens, as the spec suggests).
#[must_use]
pub fn new_device_id() -> String {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("system random");
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    b.iter().fold(String::with_capacity(32), |mut s, x| {
        use std::fmt::Write as _;
        let _ = write!(s, "{x:02x}");
        s
    })
}

/// Removes what the spec forbids in a name (`"',;:.!?()[]<>`) and control characters; cuts to 32 characters.
#[must_use]
pub fn clean_name(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| !c.is_control() && !"\"',;:.!?()[]<>".contains(*c))
        .take(MAX_NAME)
        .collect();
    let s = s.trim().to_owned();
    if s.is_empty() {
        "KDE Connect".into()
    } else {
        s
    }
}

/// An identity packet, in any of its three forms (UDP, first TCP line, after TLS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub device_id: String,
    pub protocol_version: u64,
    pub device_name: String,
    pub device_type: DeviceType,
    pub incoming: Vec<String>,
    pub outgoing: Vec<String>,
    pub tcp_port: Option<u16>,
    pub target_device_id: Option<String>,
    pub target_protocol_version: Option<u64>,
}

impl Identity {
    /// Reads an identity packet; `None` if it is not one or breaks a rule.
    #[must_use]
    pub fn from_packet(p: &Packet) -> Option<Self> {
        if p.kind != IDENTITY {
            return None;
        }
        let device_id = p.body.get("deviceId")?.as_str()?.to_owned();
        if !valid_device_id(&device_id) {
            return None;
        }
        let caps = |key: &str| -> Vec<String> {
            p.body
                .get(key)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .filter(|s| s.len() <= 64)
                        .take(MAX_CAPS)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let target_device_id = match p.body.get("targetDeviceId") {
            None => None,
            Some(v) => Some(v.as_str().filter(|s| valid_device_id(s))?.to_owned()),
        };
        Some(Self {
            device_id,
            protocol_version: p.u64("protocolVersion")?,
            device_name: clean_name(&p.str("deviceName", 256).unwrap_or_default()),
            device_type: DeviceType::parse(&p.str("deviceType", 16).unwrap_or_default()),
            incoming: caps("incomingCapabilities"),
            outgoing: caps("outgoingCapabilities"),
            tcp_port: p
                .u64("tcpPort")
                .and_then(|n| u16::try_from(n).ok())
                .filter(|n| *n != 0),
            target_device_id,
            target_protocol_version: p.u64("targetProtocolVersion"),
        })
    }

    /// The UDP broadcast: who we are and where we listen.
    #[must_use]
    pub fn udp(device_id: &str, name: &str, kind: DeviceType, tcp_port: u16) -> Vec<u8> {
        line(
            IDENTITY,
            &json!({
                "deviceId": device_id,
                "deviceName": clean_name(name),
                "deviceType": kind.as_str(),
                "protocolVersion": PROTOCOL_VERSION,
                "tcpPort": tcp_port,
                "incomingCapabilities": CAPABILITIES,
                "outgoingCapabilities": CAPABILITIES,
            }),
            None,
        )
    }

    /// The first, plaintext line on a TCP connection we opened: it names the device we expect.
    #[must_use]
    pub fn first(device_id: &str, target: &str) -> Vec<u8> {
        line(
            IDENTITY,
            &json!({
                "deviceId": device_id,
                "protocolVersion": PROTOCOL_VERSION,
                "targetDeviceId": target,
                "targetProtocolVersion": PROTOCOL_VERSION,
            }),
            None,
        )
    }

    /// The identity both sides send once TLS is up.
    #[must_use]
    pub fn full(device_id: &str, name: &str, kind: DeviceType) -> Vec<u8> {
        line(
            IDENTITY,
            &json!({
                "deviceId": device_id,
                "deviceName": clean_name(name),
                "deviceType": kind.as_str(),
                "protocolVersion": PROTOCOL_VERSION,
                "incomingCapabilities": CAPABILITIES,
                "outgoingCapabilities": CAPABILITIES,
            }),
            None,
        )
    }
}

/// `kdeconnect.pair`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pair {
    pub pair: bool,
    /// Seconds since the epoch; required in a request.
    pub timestamp: Option<u64>,
}

impl Pair {
    #[must_use]
    pub fn from_packet(p: &Packet) -> Option<Self> {
        if p.kind != PAIR {
            return None;
        }
        Some(Self {
            pair: p.body.get("pair")?.as_bool()?,
            timestamp: p.u64("timestamp"),
        })
    }

    #[must_use]
    pub fn request(timestamp: u64) -> Vec<u8> {
        line(PAIR, &json!({ "pair": true, "timestamp": timestamp }), None)
    }

    #[must_use]
    pub fn answer(yes: bool) -> Vec<u8> {
        line(PAIR, &json!({ "pair": yes }), None)
    }
}

/// `kdeconnect.share.request`: a file (with a payload), a text or a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Share {
    File {
        /// Already cleaned for every desktop OS.
        name: String,
        size: u64,
        port: u16,
        number_of_files: Option<u64>,
        total_size: Option<u64>,
    },
    Text(String),
    Url(String),
}

impl Share {
    #[must_use]
    pub fn from_packet(p: &Packet) -> Option<Self> {
        if p.kind != SHARE {
            return None;
        }
        if let Some(name) = p.body.get("filename") {
            let name = vutric_core::filename::sanitize(name.as_str()?);
            return Some(Self::File {
                name,
                size: p.payload_size?,
                port: p.payload_port?,
                number_of_files: p.u64("numberOfFiles"),
                total_size: p.u64("totalPayloadSize"),
            });
        }
        if let Some(t) = p.str("text", MAX_TEXT) {
            return Some(Self::Text(t));
        }
        let url = p.str("url", MAX_TEXT)?;
        url.chars()
            .all(|c| !c.is_control())
            .then_some(Self::Url(url))
    }

    /// The packet that offers one file, whose bytes wait on `port`.
    #[must_use]
    pub fn file(name: &str, size: u64, port: u16, number: u64, total: u64) -> Vec<u8> {
        line(
            SHARE,
            &json!({
                "filename": name,
                "numberOfFiles": number,
                "totalPayloadSize": total,
            }),
            Some((size, port)),
        )
    }

    /// Announces a batch before its files.
    #[must_use]
    pub fn update(number: u64, total: u64) -> Vec<u8> {
        line(
            SHARE_UPDATE,
            &json!({ "numberOfFiles": number, "totalPayloadSize": total }),
            None,
        )
    }

    #[must_use]
    pub fn text(text: &str) -> Vec<u8> {
        line(SHARE, &json!({ "text": text }), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(b: &[u8]) -> Option<Packet> {
        Packet::parse(b.strip_suffix(b"\n").unwrap_or(b), MAX_LINE)
    }

    #[test]
    fn identity_round_trips_in_all_three_forms() {
        let id = new_device_id();
        assert!(valid_device_id(&id));
        let udp = Identity::from_packet(
            &parse(&Identity::udp(&id, "Ish PC", DeviceType::Desktop, 1716)).unwrap(),
        )
        .unwrap();
        assert_eq!(udp.tcp_port, Some(1716));
        assert_eq!(udp.protocol_version, 8);
        assert!(udp.incoming.iter().any(|c| c == SHARE));
        let target = new_device_id();
        let first = Identity::from_packet(&parse(&Identity::first(&id, &target)).unwrap()).unwrap();
        assert_eq!(first.target_device_id.as_deref(), Some(target.as_str()));
        assert_eq!(first.target_protocol_version, Some(8));
        let full = Identity::from_packet(
            &parse(&Identity::full(&id, "a.b(c)", DeviceType::Phone)).unwrap(),
        )
        .unwrap();
        assert_eq!(full.device_name, "abc");
        assert_eq!(full.device_type, DeviceType::Phone);
    }

    #[test]
    fn refuses_bad_packets() {
        for bad in [
            &b"{}"[..],
            b"not json",
            br#"{"type":"other.thing","body":{}}"#,
            br#"{"type":"kdeconnect.Identity","body":{}}"#,
            br#"{"type":"kdeconnect.share.request","body":{},"payloadSize":-1}"#,
            br#"{"type":"kdeconnect.share.request","body":{},"payloadTransferInfo":{"port":70000}}"#,
            br#"{"type":"kdeconnect.share.request","body":{},"payloadTransferInfo":{"port":0}}"#,
        ] {
            assert_eq!(Packet::parse(bad, MAX_LINE), None, "{}", String::from_utf8_lossy(bad));
        }
        let long = format!(
            r#"{{"type":"kdeconnect.pair","body":{{"x":"{}"}}}}"#,
            "a".repeat(5000)
        );
        assert!(Packet::parse(long.as_bytes(), MAX_PREAUTH_LINE).is_none());
        assert!(Packet::parse(long.as_bytes(), MAX_LINE).is_some());
        let short_id =
            br#"{"type":"kdeconnect.identity","body":{"deviceId":"abc","protocolVersion":8}}"#;
        assert!(Identity::from_packet(&Packet::parse(short_id, MAX_LINE).unwrap()).is_none());
    }

    #[test]
    fn share_file_text_and_url() {
        let f = Share::from_packet(&parse(&Share::file("../../x:y.txt", 10, 1739, 2, 30)).unwrap())
            .unwrap();
        let Share::File {
            name,
            size,
            port,
            number_of_files,
            total_size,
        } = f
        else {
            panic!("file expected")
        };
        assert!(!name.contains('/') && !name.contains(':'));
        assert_eq!(
            (size, port, number_of_files, total_size),
            (10, 1739, Some(2), Some(30))
        );
        assert_eq!(
            Share::from_packet(&parse(&Share::text("salom")).unwrap()),
            Some(Share::Text("salom".into()))
        );
        let url = br#"{"type":"kdeconnect.share.request","body":{"url":"https://kde.org"}}"#;
        assert_eq!(
            Share::from_packet(&Packet::parse(url, MAX_LINE).unwrap()),
            Some(Share::Url("https://kde.org".into()))
        );
        let no_payload = br#"{"type":"kdeconnect.share.request","body":{"filename":"a"}}"#;
        assert_eq!(
            Share::from_packet(&Packet::parse(no_payload, MAX_LINE).unwrap()),
            None
        );
    }

    #[test]
    fn pair_packets() {
        let p = Pair::from_packet(&parse(&Pair::request(642_247_200)).unwrap()).unwrap();
        assert_eq!(
            p,
            Pair {
                pair: true,
                timestamp: Some(642_247_200)
            }
        );
        assert_eq!(
            Pair::from_packet(&parse(&Pair::answer(false)).unwrap()),
            Some(Pair {
                pair: false,
                timestamp: None
            })
        );
    }
}

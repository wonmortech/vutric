//! `LocalSend` protocol v2.2 messages (<https://github.com/localsend/protocol>, commit `62bd340`, read again
//! 2026-11-18): pure types and strict parsers, no I/O. Everything a foreign device sends before a user has
//! accepted anything goes through here, so every parser checks the size before it allocates, bounds every
//! collection and string, and is fuzzed (`fuzz/`, targets `localsend_*`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The protocol version we speak (`major.minor`). Peers with another major version are refused.
pub const VERSION: &str = "2.2";
/// Default HTTP(S) port and multicast port.
pub const PORT: u16 = 53317;
/// Default multicast group (inside `224.0.0.0/24`, which some Android devices require).
pub const MULTICAST: std::net::Ipv4Addr = std::net::Ipv4Addr::new(224, 0, 0, 167);

/// Largest JSON body read before parsing (a `prepare-upload` with thousands of files and small previews).
pub const MAX_BODY: usize = 1 << 20;
/// Largest multicast datagram read.
pub const MAX_DATAGRAM: usize = 4096;
/// Most files in one `prepare-upload`.
pub const MAX_FILES: usize = 4096;
/// Longest file id, token or session id kept (characters).
pub const MAX_TEXT: usize = 128;
/// Largest preview kept (bytes of its string); larger previews are dropped, not refused.
pub const MAX_PREVIEW: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Mobile,
    Desktop,
    Web,
    Headless,
    Server,
    /// Anything newer: shown as a desktop, as the official app does.
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Http,
    Https,
}

/// A device's description: the multicast announcement, `register`, `info` and the `info` of `prepare-upload`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub alias: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_type: Option<DeviceType>,
    /// Only for avoiding self-discovery; in HTTPS mode the real identity is the certificate hash.
    #[serde(default)]
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<Scheme>,
    #[serde(default)]
    pub download: bool,
    /// Multicast only: `true` asks everyone to answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub announce: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileTimes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accessed: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub id: String,
    pub file_name: String,
    pub size: u64,
    #[serde(default)]
    pub file_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<FileTimes>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareUpload {
    pub info: DeviceInfo,
    pub files: BTreeMap<String, FileMeta>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareResponse {
    pub session_id: String,
    /// File id to upload token, for the accepted files only.
    pub files: BTreeMap<String, String>,
}

/// Why a foreign message was refused. Maps to the HTTP status of the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    TooLarge,
    Malformed(String),
    /// Another major protocol version.
    Version(String),
    Invalid(&'static str),
}

impl ProtoError {
    #[must_use]
    pub fn status(&self) -> u16 {
        match self {
            Self::TooLarge => 413,
            Self::Malformed(_) | Self::Version(_) | Self::Invalid(_) => 400,
        }
    }
}

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("message too large"),
            Self::Malformed(e) => write!(f, "malformed: {e}"),
            Self::Version(v) => write!(f, "unsupported protocol version {v}"),
            Self::Invalid(why) => write!(f, "invalid: {why}"),
        }
    }
}

impl std::error::Error for ProtoError {}

/// Display-safe text: no control or bidi override characters, at most `max` characters, trimmed.
#[must_use]
pub fn clean(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl DeviceInfo {
    /// Checks the version and bounds every string. The alias becomes display-safe (at most 64 characters,
    /// never empty).
    pub fn checked(mut self) -> Result<Self, ProtoError> {
        let major = self.version.split('.').next().unwrap_or_default();
        if major != "2" {
            return Err(ProtoError::Version(clean(&self.version, 16)));
        }
        self.version = clean(&self.version, 16);
        self.alias = clean(&self.alias, 64);
        if self.alias.is_empty() {
            "LocalSend".clone_into(&mut self.alias);
        }
        self.device_model = self
            .device_model
            .map(|m| clean(&m, 64))
            .filter(|m| !m.is_empty());
        self.fingerprint = clean(&self.fingerprint, MAX_TEXT);
        if self.port == Some(0) {
            return Err(ProtoError::Invalid("port 0"));
        }
        Ok(self)
    }
}

fn parse_json<T: for<'de> Deserialize<'de>>(body: &[u8], max: usize) -> Result<T, ProtoError> {
    if body.len() > max {
        return Err(ProtoError::TooLarge);
    }
    serde_json::from_slice(body).map_err(|e| ProtoError::Malformed(e.to_string()))
}

/// A multicast announcement or answer.
pub fn parse_announcement(datagram: &[u8]) -> Result<DeviceInfo, ProtoError> {
    parse_json::<DeviceInfo>(datagram, MAX_DATAGRAM)?.checked()
}

/// A `register` body (or its answer).
pub fn parse_register(body: &[u8]) -> Result<DeviceInfo, ProtoError> {
    parse_json::<DeviceInfo>(body, 16 * 1024)?.checked()
}

/// A `prepare-upload` body. File names come out sanitized for saving
/// ([`vutric_core::filename::sanitize`]), ids and checksums are bounded, oversized previews are dropped.
pub fn parse_prepare_upload(body: &[u8]) -> Result<PrepareUpload, ProtoError> {
    let mut p: PrepareUpload = parse_json(body, MAX_BODY)?;
    p.info = p.info.checked()?;
    if p.files.is_empty() {
        return Err(ProtoError::Invalid("no files"));
    }
    if p.files.len() > MAX_FILES {
        return Err(ProtoError::TooLarge);
    }
    let mut out = BTreeMap::new();
    for (key, mut f) in p.files {
        if key.is_empty()
            || key.chars().count() > MAX_TEXT
            || f.id != key
            || key.chars().any(char::is_control)
        {
            return Err(ProtoError::Invalid("file id"));
        }
        f.file_name = vutric_core::filename::sanitize(&f.file_name);
        f.file_type = clean(&f.file_type, MAX_TEXT);
        f.sha256 = match f.sha256 {
            Some(h) if is_hex64(&h) => Some(h.to_ascii_lowercase()),
            Some(h) if h.is_empty() => None,
            Some(_) => return Err(ProtoError::Invalid("sha256")),
            None => None,
        };
        if f.preview.as_ref().is_some_and(|p| p.len() > MAX_PREVIEW) {
            f.preview = None;
        }
        f.metadata = f.metadata.map(|m| FileTimes {
            modified: m.modified.map(|t| clean(&t, 40)),
            accessed: m.accessed.map(|t| clean(&t, 40)),
        });
        out.insert(key, f);
    }
    p.files = out;
    Ok(p)
}

/// The answer to our `prepare-upload`.
pub fn parse_prepare_response(body: &[u8]) -> Result<PrepareResponse, ProtoError> {
    let r: PrepareResponse = parse_json(body, MAX_BODY)?;
    if r.session_id.is_empty() || r.session_id.chars().count() > MAX_TEXT {
        return Err(ProtoError::Invalid("session id"));
    }
    if r.files.len() > MAX_FILES
        || r.files
            .iter()
            .any(|(k, v)| k.chars().count() > MAX_TEXT || v.chars().count() > MAX_TEXT)
    {
        return Err(ProtoError::Invalid("tokens"));
    }
    Ok(r)
}

/// Query parameters (`sessionId`, `fileId`, `token`, `pin`). Unknown keys are kept but bounded; values are
/// percent-decoded; a broken escape drops that pair.
#[must_use]
pub fn query(q: &str) -> BTreeMap<String, String> {
    q.split('&')
        .filter_map(|kv| kv.split_once('='))
        .take(16)
        .filter_map(|(k, v)| Some((pct_decode(k)?, pct_decode(v)?)))
        .filter(|(k, v)| k.len() <= 32 && v.chars().count() <= MAX_TEXT)
        .collect()
}

fn pct_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Percent-encodes a query value.
#[must_use]
pub fn pct_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANNOUNCE: &str = r#"{"alias":"Nice Orange","version":"2.0","deviceModel":"Samsung","deviceType":"mobile",
        "fingerprint":"random string","port":53317,"protocol":"https","download":true,"announce":true}"#;

    #[test]
    fn spec_examples_parse() {
        let a = parse_announcement(ANNOUNCE.as_bytes()).unwrap();
        assert_eq!(a.alias, "Nice Orange");
        assert_eq!(a.device_type, Some(DeviceType::Mobile));
        assert_eq!(a.protocol, Some(Scheme::Https));
        assert_eq!(a.announce, Some(true));

        let prep = r#"{"info":{"alias":"Nice Orange","version":"2.0","deviceModel":"Samsung","deviceType":"mobile",
            "fingerprint":"x","port":53317,"protocol":"https","download":true},
          "files":{"some file id":{"id":"some file id","fileName":"my image.png","size":324242,"fileType":"image/jpeg",
            "sha256":null,"preview":null,"metadata":{"modified":"2021-01-01T12:34:56Z","accessed":null}},
            "another file id":{"id":"another file id","fileName":"../../etc/passwd","size":1234,"fileType":"image/jpeg"}}}"#;
        let p = parse_prepare_upload(prep.as_bytes()).unwrap();
        assert_eq!(p.files.len(), 2);
        assert_eq!(p.files["another file id"].file_name, "passwd");

        let r = parse_prepare_response(br#"{"sessionId":"s","files":{"a":"t"}}"#).unwrap();
        assert_eq!(r.files["a"], "t");
    }

    #[test]
    fn unknown_device_type_and_fields_are_tolerated() {
        let a =
            parse_register(br#"{"alias":"x","version":"2.1","deviceType":"fridge","newField":1}"#)
                .unwrap();
        assert_eq!(a.device_type, Some(DeviceType::Unknown));
    }

    #[test]
    fn refusals() {
        assert!(matches!(
            parse_register(br#"{"alias":"x","version":"3.0"}"#),
            Err(ProtoError::Version(_))
        ));
        assert!(matches!(
            parse_register(br#"{"alias":"x","version":"2.0","port":0}"#),
            Err(ProtoError::Invalid(_))
        ));
        assert_eq!(
            parse_announcement(&vec![b' '; MAX_DATAGRAM + 1]),
            Err(ProtoError::TooLarge)
        );
        let mismatch =
            br#"{"info":{"alias":"a","version":"2.0"},"files":{"a":{"id":"b","fileName":"f","size":1}}}"#;
        assert!(parse_prepare_upload(mismatch).is_err());
        let bad_hash = br#"{"info":{"alias":"a","version":"2.0"},"files":{"a":{"id":"a","fileName":"f","size":1,"sha256":"zz"}}}"#;
        assert!(parse_prepare_upload(bad_hash).is_err());
        let none = br#"{"info":{"alias":"a","version":"2.0"},"files":{}}"#;
        assert!(parse_prepare_upload(none).is_err());
    }

    #[test]
    fn alias_is_display_safe() {
        let rlo = char::from_u32(0x202E).unwrap();
        let body = format!(
            r#"{{"alias":"ev{rlo}il\u0007 {}","version":"2.0"}}"#,
            "x".repeat(200)
        );
        let a = parse_register(body.as_bytes()).unwrap();
        assert!(!a.alias.contains(rlo) && !a.alias.contains('\u{7}'));
        assert_eq!(a.alias.chars().count(), 64);
        assert_eq!(
            parse_register(br#"{"alias":"  ","version":"2.0"}"#)
                .unwrap()
                .alias,
            "LocalSend"
        );
    }

    #[test]
    fn query_round_trip() {
        let q = query(&format!(
            "sessionId={}&fileId=a%20b&token=t+1&x",
            pct_encode("s/1 ?")
        ));
        assert_eq!(q["sessionId"], "s/1 ?");
        assert_eq!(q["fileId"], "a b");
        assert_eq!(q["token"], "t 1");
        assert!(query("a=%zz").is_empty());
    }
}

//! Messages between the daemon and the adapter process: one JSON object per line, stdin (to the adapter) and
//! stdout (from it). The adapter is the side that touched foreign bytes, so the daemon treats everything it
//! says as untrusted too: lines are size-limited, enums are closed, and paths are checked by [`crate::host`].

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::proto::DeviceType;

/// Longest line either side reads.
pub const MAX_LINE: usize = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Start {
    pub alias: String,
    pub device_type: DeviceType,
    /// HTTPS and multicast port; 0 for any (tests).
    pub port: u16,
    pub bind: IpAddr,
    /// Where accepted files are written (the only folder the adapter writes to, besides its identity file).
    pub staging: PathBuf,
    /// The certificate and key kept across restarts.
    pub identity: PathBuf,
    pub pin: Option<String>,
    /// Join the multicast group and answer announcements.
    pub multicast: bool,
    pub decide_timeout_secs: u64,
    /// Also receive from Android's Quick Share (C3, ADR-0056); advertised over mDNS when `multicast` is on.
    #[serde(default)]
    pub quickshare: bool,
}

/// Which protocol an offer came over.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    #[default]
    Localsend,
    Quickshare,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToAdapter {
    Start(Start),
    /// The user's answer to an offer; an empty `accept` rejects it.
    Decide {
        session_id: String,
        accept: Vec<String>,
    },
    Announce,
    Send {
        request_id: u64,
        to: SocketAddr,
        https: bool,
        paths: Vec<PathBuf>,
        pin: Option<String>,
    },
    /// Open a browser link (C2): download `paths` (none: upload only) for `ttl_secs`.
    Share {
        request_id: u64,
        paths: Vec<PathBuf>,
        ttl_secs: u64,
    },
    Unshare {
        token: String,
    },
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferFile {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub file_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FromAdapter {
    Ready {
        fingerprint: String,
        port: u16,
        #[serde(default)]
        quickshare_port: Option<u16>,
    },
    Seen {
        alias: String,
        device_type: Option<DeviceType>,
        to: SocketAddr,
        https: bool,
    },
    Offer {
        session_id: String,
        peer: IpAddr,
        /// The sender's certificate hash, when it presented one: what the user pins.
        peer_cert: Option<String>,
        alias: String,
        files: Vec<OfferFile>,
        #[serde(default)]
        via: Via,
        /// Quick Share's 4-digit code, shown on both screens.
        #[serde(default)]
        code: Option<String>,
    },
    /// Text or a link shared over Quick Share (kind: 1 text, 2 URL, 3 address, 4 phone number).
    Text {
        session_id: String,
        kind: i64,
        text: String,
    },
    Received {
        session_id: String,
        file_id: String,
        name: String,
        path: PathBuf,
        size: u64,
    },
    Finished {
        session_id: String,
    },
    Cancelled {
        session_id: String,
    },
    Sent {
        request_id: u64,
        files: usize,
        bytes: u64,
        peer_cert: Option<String>,
    },
    SendFailed {
        request_id: u64,
        reason: String,
    },
    /// The page is at `https://<address>:<port>/s/<token>/`.
    Shared {
        request_id: u64,
        token: String,
    },
    ShareFailed {
        request_id: u64,
        reason: String,
    },
    Downloaded {
        token: String,
        name: String,
    },
    Error {
        message: String,
    },
}

/// One message as a line (with the trailing newline).
#[must_use]
pub fn line<T: Serialize>(m: &T) -> Vec<u8> {
    let mut v = serde_json::to_vec(m).unwrap_or_default();
    v.push(b'\n');
    v
}

/// Parses one line; `None` for anything too long or malformed.
#[must_use]
pub fn parse<T: for<'de> Deserialize<'de>>(line: &[u8]) -> Option<T> {
    if line.len() > MAX_LINE {
        return None;
    }
    serde_json::from_slice(line).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_refusals() {
        let m = FromAdapter::Finished {
            session_id: "s".into(),
        };
        let l = line(&m);
        assert_eq!(l.last(), Some(&b'\n'));
        assert_eq!(parse::<FromAdapter>(&l[..l.len() - 1]), Some(m));
        assert_eq!(parse::<FromAdapter>(br#"{"t":"nope"}"#), None);
        assert_eq!(parse::<FromAdapter>(&vec![b' '; MAX_LINE + 1]), None);
    }
}

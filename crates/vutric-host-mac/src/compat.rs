//! Foreground `LocalSend` and Quick Share on macOS (C1/C3, ADR-0117).
//!
//! `vutric-mac-compat` runs only while the menu-bar agent has "`LocalSend` / Quick Share" switched on. It starts the
//! `vutric-compat` adapter process through [`vutric_compat_localsend::host::Adapter`] (the same low-privilege child
//! and path checks the daemon uses, ADR-0054) and gives the agent a small, already-checked JSON-lines protocol: the
//! agent never sees a foreign byte that was not cleaned here first.
//!
//! This module is the pure part: the two message sets and the state that turns adapter events into agent events.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vutric_compat_localsend::ipc::{FromAdapter, Via};
use vutric_core::companion::shown;

/// Longest alias or file name shown in the agent.
pub const MAX_NAME: usize = 80;
/// File names listed in an offer question; the rest are counted.
pub const LISTED_NAMES: usize = 5;
/// Quick Share text kept, in characters (the 64 KiB `Clipboard` cap of ADR-0065 is bytes; this is below it).
pub const MAX_TEXT_CHARS: usize = 16 * 1024;

/// From the agent, one per line on stdin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    /// The user's answer to an [`Event::Offer`]: all files or none.
    Decide {
        session: String,
        accept: bool,
    },
    /// Send files to a `LocalSend` device from [`Event::Devices`].
    Send {
        request_id: u64,
        device: String,
        paths: Vec<PathBuf>,
    },
    /// Ask devices on the network to answer now (the menu opened).
    Announce,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub alias: String,
    pub via: Via,
}

/// To the agent, one per line on stdout. Every string in it has been through [`shown`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Ready {
        port: u16,
        quickshare_port: Option<u16>,
    },
    /// The devices that can be sent to (`LocalSend` only: Quick Share here receives, ADR-0056).
    Devices {
        devices: Vec<Device>,
    },
    Offer {
        session: String,
        via: Via,
        alias: String,
        files: usize,
        bytes: u64,
        names: Vec<String>,
        /// Quick Share's four digits, shown so the user can compare with the phone.
        code: Option<String>,
    },
    /// Text or a link from Quick Share: offered with a "Nusxalash" button, never acted on (ADR-0065 rules).
    Text {
        session: String,
        text: String,
        link: bool,
    },
    Received {
        session: String,
        name: String,
        path: PathBuf,
    },
    Finished {
        session: String,
    },
    Cancelled {
        session: String,
    },
    Sent {
        request_id: u64,
        files: usize,
    },
    SendFailed {
        request_id: u64,
        reason: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
struct Seen {
    alias: String,
    to: SocketAddr,
    https: bool,
}

/// What the bridge remembers between events.
#[derive(Debug, Default)]
pub struct Bridge {
    devices: HashMap<String, Seen>,
    offers: HashMap<String, Vec<String>>,
}

fn via_name(v: Via) -> &'static str {
    match v {
        Via::Quickshare => "quickshare",
        Via::Kdeconnect => "kdeconnect",
        Via::Wormhole => "wormhole",
        _ => "localsend",
    }
}

impl Bridge {
    /// Turns one adapter event into what the agent is told, if anything. `Received` is not handled here: its file
    /// is checked and moved by the caller ([`place`]).
    pub fn on_adapter(&mut self, ev: FromAdapter) -> Option<Event> {
        match ev {
            FromAdapter::Ready {
                port,
                quickshare_port,
                ..
            } => Some(Event::Ready {
                port,
                quickshare_port,
            }),
            FromAdapter::Seen {
                alias,
                to,
                https,
                via,
                ..
            } if via == Via::Localsend => {
                let id = format!("{}:{to}", via_name(via));
                let alias = shown(&alias, MAX_NAME, false);
                let changed = self
                    .devices
                    .get(&id)
                    .is_none_or(|s| s.alias != alias || s.https != https);
                self.devices.insert(id, Seen { alias, to, https });
                changed.then(|| self.devices_event())
            }
            FromAdapter::Offer {
                session_id,
                alias,
                files,
                via,
                code,
                ..
            } => {
                let bytes = files.iter().map(|f| f.size).fold(0u64, u64::saturating_add);
                let names = files
                    .iter()
                    .take(LISTED_NAMES)
                    .map(|f| shown(&f.name, MAX_NAME, false))
                    .collect();
                let count = files.len();
                self.offers.insert(
                    session_id.clone(),
                    files.into_iter().map(|f| f.id).collect(),
                );
                Some(Event::Offer {
                    session: session_id,
                    via,
                    alias: shown(&alias, MAX_NAME, false),
                    files: count,
                    bytes,
                    names,
                    code: code.map(|c| shown(&c, 8, false)),
                })
            }
            FromAdapter::Text {
                session_id,
                kind,
                text,
            } => Some(Event::Text {
                session: session_id,
                text: shown(&text, MAX_TEXT_CHARS, true),
                link: kind == 2,
            }),
            FromAdapter::Finished { session_id } => {
                self.offers.remove(&session_id);
                Some(Event::Finished {
                    session: session_id,
                })
            }
            FromAdapter::Cancelled { session_id } => {
                self.offers.remove(&session_id);
                Some(Event::Cancelled {
                    session: session_id,
                })
            }
            FromAdapter::Sent {
                request_id, files, ..
            } => Some(Event::Sent { request_id, files }),
            FromAdapter::SendFailed { request_id, reason } => Some(Event::SendFailed {
                request_id,
                reason: shown(&reason, 200, false),
            }),
            FromAdapter::Error { message } => Some(Event::Error {
                message: shown(&message, 200, false),
            }),
            _ => None,
        }
    }

    /// The file ids to accept for `session` (all of them), or none for an unknown session.
    pub fn accept(&mut self, session: &str, accept: bool) -> Vec<String> {
        let ids = self.offers.get(session).cloned().unwrap_or_default();
        if accept { ids } else { Vec::new() }
    }

    /// Where to send for a device id from [`Event::Devices`].
    #[must_use]
    pub fn target(&self, device: &str) -> Option<(SocketAddr, bool)> {
        self.devices.get(device).map(|s| (s.to, s.https))
    }

    fn devices_event(&self) -> Event {
        let mut devices: Vec<Device> = self
            .devices
            .iter()
            .map(|(id, s)| Device {
                id: id.clone(),
                alias: s.alias.clone(),
                via: Via::Localsend,
            })
            .collect();
        devices.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        Event::Devices { devices }
    }
}

/// Moves a checked staged file into `dest`, never over an existing file: `photo.jpg`, `photo (2).jpg`, …
///
/// # Errors
/// When `dest` cannot be created or the file cannot be moved or copied there.
pub fn place(staged: &Path, dest: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dest)?;
    let raw = staged
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");
    let name = vutric_core::filename::unique(&vutric_core::filename::sanitize(raw), |c| {
        dest.join(c).exists()
    });
    let to = dest.join(name);
    if std::fs::rename(staged, &to).is_err() {
        // Another volume: copy, then remove the staged copy.
        std::fs::copy(staged, &to)?;
        let _ = std::fs::remove_file(staged);
    }
    Ok(to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vutric_compat_localsend::ipc::OfferFile;

    fn offer(session: &str, n: usize) -> FromAdapter {
        FromAdapter::Offer {
            session_id: session.into(),
            peer: "192.168.1.5".parse().unwrap(),
            peer_cert: None,
            alias: "Pixel\u{202E}gpj.exe\n".into(),
            files: (0..n)
                .map(|i| OfferFile {
                    id: format!("f{i}"),
                    name: format!("rasm{i}.jpg"),
                    size: 1000,
                    file_type: "image/jpeg".into(),
                })
                .collect(),
            via: Via::Quickshare,
            code: Some("1234".into()),
        }
    }

    #[test]
    fn offers_are_cleaned_counted_and_accepted_whole() {
        let mut b = Bridge::default();
        let Some(Event::Offer {
            alias,
            files,
            bytes,
            names,
            code,
            ..
        }) = b.on_adapter(offer("s1", 7))
        else {
            panic!("an offer");
        };
        assert_eq!(alias, "Pixelgpj.exe", "bidi override and newline removed");
        assert_eq!((files, bytes), (7, 7000));
        assert_eq!(names.len(), LISTED_NAMES);
        assert_eq!(code.as_deref(), Some("1234"));
        assert_eq!(b.accept("s1", true).len(), 7);
        assert!(b.accept("s1", false).is_empty(), "refusing sends no ids");
        assert!(b.accept("unknown", true).is_empty());
        b.on_adapter(FromAdapter::Finished {
            session_id: "s1".into(),
        });
        assert!(b.accept("s1", true).is_empty(), "forgotten when finished");
    }

    #[test]
    fn devices_are_localsend_only_and_reported_on_change() {
        let mut b = Bridge::default();
        let seen = |alias: &str, via| FromAdapter::Seen {
            alias: alias.into(),
            device_type: None,
            to: "192.168.1.9:53317".parse().unwrap(),
            https: true,
            via,
            device_id: None,
        };
        let Some(Event::Devices { devices }) = b.on_adapter(seen("Mac", Via::Localsend)) else {
            panic!("devices");
        };
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, "localsend:192.168.1.9:53317");
        assert!(
            b.on_adapter(seen("Mac", Via::Localsend)).is_none(),
            "same again: quiet"
        );
        assert!(b.on_adapter(seen("Mac 2", Via::Localsend)).is_some());
        assert!(b.on_adapter(seen("Phone", Via::Kdeconnect)).is_none());
        assert_eq!(
            b.target("localsend:192.168.1.9:53317"),
            Some(("192.168.1.9:53317".parse().unwrap(), true))
        );
        assert_eq!(b.target("nope"), None);
    }

    #[test]
    fn text_is_cleaned_and_links_marked() {
        let mut b = Bridge::default();
        let Some(Event::Text { text, link, .. }) = b.on_adapter(FromAdapter::Text {
            session_id: "s".into(),
            kind: 2,
            text: "https://example.com/\u{7}".into(),
        }) else {
            panic!("text");
        };
        assert_eq!(text, "https://example.com/");
        assert!(link);
    }

    #[test]
    fn commands_parse_and_unknown_ops_are_refused() {
        let c: Command =
            serde_json::from_str(r#"{"op":"decide","session":"a","accept":true}"#).unwrap();
        assert_eq!(
            c,
            Command::Decide {
                session: "a".into(),
                accept: true
            }
        );
        assert!(serde_json::from_str::<Command>(r#"{"op":"rm","path":"/"}"#).is_err());
        let e = serde_json::to_string(&Event::Finished {
            session: "a".into(),
        })
        .unwrap();
        assert_eq!(e, r#"{"kind":"finished","session":"a"}"#);
    }

    #[test]
    fn place_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("vutric-place-{}", std::process::id()));
        let staging = dir.join("staging");
        let dest = dir.join("dest");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("a.txt"), "old").unwrap();
        let staged = staging.join("a.txt");
        std::fs::write(&staged, "new").unwrap();
        let to = place(&staged, &dest).unwrap();
        assert_eq!(to.file_name().unwrap(), "a (2).txt");
        assert_eq!(std::fs::read_to_string(dest.join("a.txt")).unwrap(), "old");
        assert!(!staged.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}

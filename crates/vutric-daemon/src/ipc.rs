//! Local IPC between the daemon and the desktop windows (settings, history, devices).
//!
//! One JSON object per line, UTF-8, over a per-user named pipe (`\\.\pipe\vutric-<hash>`) on Windows and a Unix
//! socket in `$XDG_RUNTIME_DIR` elsewhere. A client sends [`Request`]s; the daemon answers each with exactly one
//! [`Reply`] in order, and may interleave [`Reply::Event`] lines at any time once the client has subscribed.
//! Lines longer than [`MAX_LINE`] are refused so a broken client cannot make the daemon buffer without bound.

use serde::{Deserialize, Serialize};
use vutric_core::storage::Settings;

use crate::prefs::DesktopPrefs;

/// Protocol version; bumped on any incompatible change. A window built for another version is told so.
pub const VERSION: u32 = 1;

/// Longest accepted line, newline excluded. Settings with many device overrides stay far below this.
pub const MAX_LINE: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// First line of every connection.
    Hello {
        version: u32,
    },
    Status,
    GetSettings,
    /// Replaces the receiving settings and the desktop preferences, saved atomically by the daemon.
    SetSettings {
        settings: Box<Settings>,
        prefs: DesktopPrefs,
    },
    /// Stops (`true`) or resumes (`false`) receiving.
    Pause {
        paused: bool,
    },
    /// The pairing link the phone scans (`vutric://` URI).
    Pairing,
    /// Plays the chosen effect once on the overlay with a sample card, for the settings preview button.
    PreviewEffect,
    /// Streams [`Event`]s on this connection from now on.
    Subscribe,
    /// Brings the pairing card up (sent by a second launch before it exits).
    Show,
    Quit,
    /// Paired devices and who is connected now.
    Devices,
    /// Unpairs a device (64-hex endpoint id).
    Forget {
        id: String,
    },
    /// "Telefonni topish": rings one connected device, or every connected paired one when `device` is `None`.
    Ring {
        #[serde(default)]
        device: Option<String>,
        #[serde(default = "default_ring_secs")]
        secs: u16,
    },
    StopRing {
        #[serde(default)]
        device: Option<String>,
    },
    /// Sends files to a connected device (Explorer "Vutric bilan yuborish", the Send Zone, the macOS Share menu
    /// and Services). `device` `None`: the only connected paired device, or the most recent one.
    /// Absolute paths, folders allowed (the macOS agent caps a request at 1000 paths, ADR-0112).
    SendFiles {
        paths: Vec<std::path::PathBuf>,
        #[serde(default)]
        device: Option<String>,
    },
    /// Sends text to a connected paired device's clipboard prompt (ADR-0065). A guest, or text over 64 KiB, gets
    /// the file at `fallback_path` instead (the macOS agent writes it first, ADR-0115).
    SendText {
        text: String,
        #[serde(default)]
        fallback_path: Option<std::path::PathBuf>,
        #[serde(default)]
        device: Option<String>,
    },
    /// "Everyone nearby" for 10 minutes (S3): unknown devices may offer files, each offer asked about.
    OpenEveryone,
}

fn default_ring_secs() -> u16 {
    vutric_core::ring::DEFAULT_RING_SECS
}

/// One device for the desktop window's device list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    /// Endpoint id, 64 hex.
    pub id: String,
    pub name: String,
    /// `Android`, `Ios`, `Windows`, `Linux`, `MacOs`, or empty when unknown.
    pub platform: String,
    pub trusted: bool,
    pub connected: bool,
    /// `ticket`, `code`, `pinned`; empty for a guest.
    pub method: String,
    /// Unix seconds.
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Hello {
        version: u32,
        daemon: String,
    },
    Status(Status),
    Settings {
        settings: Box<Settings>,
        prefs: DesktopPrefs,
    },
    Pairing {
        link: String,
    },
    Devices {
        devices: Vec<Device>,
    },
    Ok,
    Error {
        message: String,
    },
    Event(Event),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Status {
    pub paused: bool,
    /// Our endpoint id, 64 hex.
    pub id: String,
    pub name: String,
    /// True once a relay is reachable (the pairing link then also works from another network).
    pub online: bool,
    pub transfers: u32,
    /// `off`, `trusted_only`, `everyone` or `pairing` (ADR-0050).
    #[serde(default)]
    pub visibility: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Offer {
        id: String,
        from: String,
        files: u32,
        bytes: u64,
    },
    Progress {
        id: String,
        bytes_done: u64,
        bytes_total: u64,
        files_done: u32,
        files_total: u32,
    },
    Received {
        id: String,
        files: u32,
        folder: String,
    },
    Failed {
        id: String,
        reason: String,
    },
    /// The device list changed: paired, forgotten, connected or gone.
    Devices,
    /// A send from this computer finished.
    Sent {
        id: String,
        files: u32,
        to: String,
    },
    /// A notice mirrored from a trusted phone, already cleaned (ADR-0065). The macOS agent shows it itself.
    Notice {
        from: String,
        app: String,
        title: String,
        text: String,
    },
    /// Clipboard text from a trusted phone, cleaned. Written to the clipboard only after the user's "Nusxalash".
    Clipboard {
        from: String,
        text: String,
    },
    Paused {
        paused: bool,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("line longer than {MAX_LINE} bytes")]
    TooLong,
    #[error("bad message: {0}")]
    Bad(String),
}

/// Encodes one message as a line (with the trailing `\n`).
pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let mut v = serde_json::to_vec(msg).unwrap_or_else(|_| b"{}".to_vec());
    v.push(b'\n');
    v
}

/// Decodes one line (without its `\n`; a trailing `\r` is tolerated).
pub fn decode<T: for<'de> Deserialize<'de>>(line: &[u8]) -> Result<T, CodecError> {
    if line.len() > MAX_LINE {
        return Err(CodecError::TooLong);
    }
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    serde_json::from_slice(line).map_err(|e| CodecError::Bad(e.to_string()))
}

/// Splits a byte stream into lines without trusting the peer: holds at most [`MAX_LINE`] bytes of a partial line.
#[derive(Debug, Default)]
pub struct LineBuffer {
    buf: Vec<u8>,
    /// Inside an overlong line: drop bytes until its newline.
    skipping: bool,
}

impl LineBuffer {
    /// Feeds bytes; returns every complete line, or [`CodecError::TooLong`] in place of an overlong one.
    pub fn push(&mut self, mut data: &[u8]) -> Vec<Result<Vec<u8>, CodecError>> {
        let mut out = Vec::new();
        while let Some(nl) = data.iter().position(|&b| b == b'\n') {
            let (head, rest) = data.split_at(nl);
            data = &rest[1..];
            if self.skipping {
                self.skipping = false;
                self.buf.clear();
                continue;
            }
            if self.buf.len() + head.len() > MAX_LINE {
                self.buf.clear();
                out.push(Err(CodecError::TooLong));
                continue;
            }
            self.buf.extend_from_slice(head);
            out.push(Ok(std::mem::take(&mut self.buf)));
        }
        if !self.skipping {
            if self.buf.len() + data.len() > MAX_LINE {
                self.buf = Vec::new();
                self.skipping = true;
                out.push(Err(CodecError::TooLong));
            } else {
                self.buf.extend_from_slice(data);
            }
        }
        out
    }
}

/// The per-user endpoint name. The user name is hashed so it never appears in a world-visible pipe list.
#[must_use]
pub fn endpoint_name(user: &str) -> String {
    let h = blake3::hash(user.as_bytes()).to_hex();
    let short = &h.as_str()[..16];
    if cfg!(windows) {
        format!(r"\\.\pipe\vutric-{short}")
    } else {
        format!("vutric-{short}.sock")
    }
}

/// Where the Unix socket lives: `$VUTRIC_IPC` when set; on macOS `~/Library/Application Support/Vutric/daemon.sock`
/// (the menu-bar agent's path, ADR-0111); elsewhere `name` in `$XDG_RUNTIME_DIR`. The macOS folder is made 0700:
/// the socket is the only gate, as the per-user pipe is on Windows.
#[cfg(unix)]
pub fn socket_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    use anyhow::Context as _;
    if let Some(p) = std::env::var_os("VUTRIC_IPC").filter(|p| !p.is_empty()) {
        return Ok(p.into());
    }
    if cfg!(target_os = "macos") {
        use std::os::unix::fs::PermissionsExt as _;
        let home = std::env::var_os("HOME").context("no HOME")?;
        let dir = std::path::PathBuf::from(home).join("Library/Application Support/Vutric");
        std::fs::create_dir_all(&dir)?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        return Ok(dir.join("daemon.sock"));
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").context("no XDG_RUNTIME_DIR")?;
    Ok(std::path::PathBuf::from(dir).join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        let all = [
            Request::Hello { version: VERSION },
            Request::Status,
            Request::GetSettings,
            Request::SetSettings {
                settings: Box::default(),
                prefs: DesktopPrefs::default(),
            },
            Request::Pause { paused: true },
            Request::Pairing,
            Request::PreviewEffect,
            Request::Subscribe,
            Request::Show,
            Request::Quit,
            Request::Devices,
            Request::Forget { id: "ab".into() },
            Request::Ring {
                device: None,
                secs: 30,
            },
            Request::StopRing {
                device: Some("ab".into()),
            },
            Request::SendFiles {
                paths: vec![r"C:\a.jpg".into()],
                device: None,
            },
            Request::SendText {
                text: "salom".into(),
                fallback_path: None,
                device: None,
            },
            Request::OpenEveryone,
        ];
        for r in all {
            let line = encode(&r);
            assert_eq!(*line.last().unwrap(), b'\n');
            let back: Request = decode(&line[..line.len() - 1]).unwrap();
            assert_eq!(back, r);
        }
    }

    #[test]
    fn the_macos_agent_requests_parse() {
        let r: Request =
            decode(br#"{"op":"send_files","paths":["/Users/me/a.jpg","/Users/me/Folder"]}"#)
                .unwrap();
        assert!(matches!(r, Request::SendFiles { ref paths, device: None } if paths.len() == 2));
        let r: Request = decode(
            br#"{"op":"send_text","text":"salom","fallback_path":"/Users/me/Outbox/Matn.txt"}"#,
        )
        .unwrap();
        assert!(matches!(
            r,
            Request::SendText {
                fallback_path: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn notice_events_have_the_agents_shape() {
        let line = encode(&Reply::Event(Event::Notice {
            from: "Pixel 8".into(),
            app: "Telegram".into(),
            title: "Ali".into(),
            text: "salom".into(),
        }));
        assert_eq!(
            line,
            b"{\"kind\":\"event\",\"event\":\"notice\",\"from\":\"Pixel 8\",\"app\":\"Telegram\",\"title\":\"Ali\",\"text\":\"salom\"}\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn vutric_ipc_wins_over_the_default_socket() {
        // SAFETY: tests in this module do not read the environment concurrently with this one.
        unsafe { std::env::set_var("VUTRIC_IPC", "/tmp/vutric-test.sock") };
        let p = socket_path("x").unwrap();
        // SAFETY: as above.
        unsafe { std::env::remove_var("VUTRIC_IPC") };
        assert_eq!(p, std::path::PathBuf::from("/tmp/vutric-test.sock"));
    }

    #[test]
    fn ring_defaults_to_thirty_seconds_and_every_phone() {
        let r: Request = decode(br#"{"op":"ring"}"#).unwrap();
        assert_eq!(
            r,
            Request::Ring {
                device: None,
                secs: vutric_core::ring::DEFAULT_RING_SECS
            }
        );
    }

    #[test]
    fn wire_shape_is_stable() {
        assert_eq!(
            encode(&Request::Pause { paused: false }),
            b"{\"op\":\"pause\",\"paused\":false}\n"
        );
        let ev = Reply::Event(Event::Paused { paused: true });
        assert_eq!(
            String::from_utf8(encode(&ev)).unwrap(),
            "{\"kind\":\"event\",\"event\":\"paused\",\"paused\":true}\n"
        );
    }

    #[test]
    fn crlf_and_garbage() {
        let r: Request = decode(b"{\"op\":\"status\"}\r").unwrap();
        assert_eq!(r, Request::Status);
        assert!(matches!(
            decode::<Request>(b"{\"op\":\"nope\"}"),
            Err(CodecError::Bad(_))
        ));
        assert!(matches!(
            decode::<Request>(b"\xff\xfe"),
            Err(CodecError::Bad(_))
        ));
    }

    #[test]
    fn line_buffer_splits_and_joins() {
        let mut b = LineBuffer::default();
        assert!(b.push(b"{\"op\":").is_empty());
        let got = b.push(b"\"status\"}\n{\"op\":\"quit\"}\npart");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].as_deref().unwrap(), b"{\"op\":\"status\"}");
        assert_eq!(got[1].as_deref().unwrap(), b"{\"op\":\"quit\"}");
        let got = b.push(b"ial\n");
        assert_eq!(got[0].as_deref().unwrap(), b"partial");
    }

    #[test]
    fn line_buffer_bounds_memory() {
        let mut b = LineBuffer::default();
        let big = vec![b'x'; MAX_LINE + 1];
        let got = b.push(&big);
        assert_eq!(got, vec![Err(CodecError::TooLong)]);
        assert!(b.buf.capacity() <= MAX_LINE + 1);
        assert!(b.push(&big).is_empty(), "still skipping the same line");
        let got = b.push(b"tail\n{\"op\":\"status\"}\n");
        assert_eq!(got.len(), 1, "the rest of the long line is dropped");
        assert_eq!(got[0].as_deref().unwrap(), b"{\"op\":\"status\"}");
    }

    #[test]
    fn endpoint_hides_the_user_name() {
        let n = endpoint_name("alice");
        assert!(!n.contains("alice"));
        assert_eq!(n, endpoint_name("alice"));
        assert_ne!(n, endpoint_name("bob"));
    }
}

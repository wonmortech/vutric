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
        ];
        for r in all {
            let line = encode(&r);
            assert_eq!(*line.last().unwrap(), b'\n');
            let back: Request = decode(&line[..line.len() - 1]).unwrap();
            assert_eq!(back, r);
        }
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

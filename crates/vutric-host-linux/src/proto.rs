//! The surface protocol between the daemon and `vutric-surface`, the process that owns the GPU on Linux
//! (ADR-0090 §3).
//!
//! Frames are `u32` little-endian length + postcard body over a Unix stream socket. The daemon is the server
//! (it listens in `$XDG_RUNTIME_DIR/vutric/`), the surface process connects, says [`FromSurface::Ready`], then
//! obeys. Images travel once per fling; frames never cross the socket, the timeline runs where the GPU is.

use serde::{Deserialize, Serialize};
use vutric_core::{Gesture, SendZone};

/// Bumped on any incompatible change; a surface built for another version is told to quit.
pub const VERSION: u32 = 1;

/// Largest frame accepted. A full-resolution 4096 × 4096 RGBA texture is 64 MiB; anything bigger is refused.
pub const MAX_FRAME: usize = 64 * 1024 * 1024 + 4096;

/// Straight (not premultiplied) sRGB RGBA, row-major.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.width > 0
            && self.height > 0
            && u64::from(self.width) * u64::from(self.height) * 4 == self.rgba.len() as u64
    }
}

/// A clickable area of a card, in buffer pixels: `[x0, y0, x1, y1]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub id: String,
    pub rect: [f32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToSurface {
    /// Create the GPU and the effect surface now (a phone connected; ADR-0031 "warm on connect").
    Warm,
    /// Play the Halqa effect with `image` on the card. `id` comes back in the replies.
    Play {
        id: u64,
        zone: SendZone,
        gesture: Gesture,
        image: Image,
    },
    /// Swap the card texture of a running effect (the full-resolution file landed).
    SetImage {
        id: u64,
        image: Image,
    },
    /// Show a card raster (premultiplied BGRA at `scale`) with its clickable areas.
    Card {
        width: u32,
        height: u32,
        scale: i32,
        bgra: Vec<u8>,
        hits: Vec<Hit>,
    },
    HideCard,
    /// Drop the GPU device and the effect surface; the process stays for cards.
    Release,
    Quit,
}

/// Per-effect frame pacing, the same numbers as the Phase 0.1 report.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameStats {
    pub frames: u32,
    pub fps: f32,
    pub first_ms: f32,
    pub p50_ms: f32,
    pub p95_ms: f32,
    pub p99_ms: f32,
    pub max_ms: f32,
    pub hitches: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FromSurface {
    Ready {
        version: u32,
        /// `layer-shell` or `gnome-shell`.
        backend: String,
        /// Buffer scale of the target output; cards are rasterised at it.
        scale: i32,
    },
    /// The GPU and the effect surface exist. `ms` is how long creating them took.
    Warmed {
        adapter: String,
        width: u32,
        height: u32,
        ms: f32,
    },
    /// First frame of effect `id` presented, `after_ms` after the `Play` arrived.
    FirstFrame {
        id: u64,
        after_ms: f32,
    },
    Played {
        id: u64,
        stats: FrameStats,
    },
    /// A card button was released under the pointer.
    Clicked {
        id: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    TooLong(usize),
    Bad(String),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong(n) => write!(f, "frame of {n} bytes exceeds {MAX_FRAME}"),
            Self::Bad(e) => write!(f, "bad frame: {e}"),
        }
    }
}

impl std::error::Error for CodecError {}

/// Encodes one message with its length prefix.
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, CodecError> {
    let body = postcard::to_stdvec(msg).map_err(|e| CodecError::Bad(e.to_string()))?;
    if body.len() > MAX_FRAME {
        return Err(CodecError::TooLong(body.len()));
    }
    let len = u32::try_from(body.len()).map_err(|_| CodecError::TooLong(body.len()))?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Accumulates bytes from the socket and yields whole messages.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole message, `Ok(None)` when more bytes are needed. After an error the stream is unusable.
    pub fn next_message<T: for<'de> Deserialize<'de>>(&mut self) -> Result<Option<T>, CodecError> {
        let Some(head) = self.buf.get(..4) else {
            return Ok(None);
        };
        let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
        if len > MAX_FRAME {
            return Err(CodecError::TooLong(len));
        }
        if self.buf.len() < 4 + len {
            return Ok(None);
        }
        let msg =
            postcard::from_bytes(&self.buf[4..4 + len]).map_err(|e| CodecError::Bad(e.to_string()));
        self.buf.drain(..4 + len);
        msg.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play() -> ToSurface {
        ToSurface::Play {
            id: 7,
            zone: SendZone::Right,
            gesture: Gesture {
                speed: 2400.0,
                angle: 0.1,
                exit_x: 0.5,
            },
            image: Image {
                width: 2,
                height: 1,
                rgba: vec![1, 2, 3, 4, 5, 6, 7, 8],
            },
        }
    }

    #[test]
    fn round_trip_in_pieces() {
        let mut bytes = encode(&play()).unwrap();
        bytes.extend(encode(&ToSurface::HideCard).unwrap());
        let mut d = Decoder::default();
        let mut got = Vec::new();
        for chunk in bytes.chunks(3) {
            d.push(chunk);
            while let Some(m) = d.next_message::<ToSurface>().unwrap() {
                got.push(m);
            }
        }
        assert_eq!(got, vec![play(), ToSurface::HideCard]);
    }

    #[test]
    fn oversized_length_is_refused_before_buffering() {
        let mut d = Decoder::default();
        d.push(&u32::MAX.to_le_bytes());
        assert!(matches!(
            d.next_message::<ToSurface>(),
            Err(CodecError::TooLong(_))
        ));
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let mut d = Decoder::default();
        d.push(&[3, 0, 0, 0, 0xff, 0xff, 0xff]);
        assert!(matches!(
            d.next_message::<ToSurface>(),
            Err(CodecError::Bad(_))
        ));
    }

    #[test]
    fn image_validation() {
        let ToSurface::Play { image, .. } = play() else {
            unreachable!()
        };
        assert!(image.is_valid());
        let bad = Image {
            width: 3,
            ..image.clone()
        };
        assert!(!bad.is_valid());
        let empty = Image {
            width: 0,
            height: 0,
            rgba: vec![],
        };
        assert!(!empty.is_valid());
    }

    #[test]
    fn replies_round_trip() {
        let r = FromSurface::Played {
            id: 1,
            stats: FrameStats {
                frames: 60,
                fps: 59.9,
                ..FrameStats::default()
            },
        };
        let mut d = Decoder::default();
        d.push(&encode(&r).unwrap());
        assert_eq!(d.next_message::<FromSurface>().unwrap(), Some(r));
    }
}

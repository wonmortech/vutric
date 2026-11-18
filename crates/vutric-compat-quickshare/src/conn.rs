//! Framing and the payload layer, shared by the receiver and the sender.
//!
//! Every message is a 4-byte big-endian length plus the bytes. A reader task owns the read half and hands whole
//! frames to the connection loop over a small channel, so the loop can wait for the user, keep-alives and the
//! peer at once without cutting a frame in half.

use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::sync::mpsc;

use crate::msg::{self, Offline, PayloadHeader};
use crate::ukey2::Channel;

/// Largest frame accepted: a 512 KiB chunk plus every wrapper, with room to spare.
pub const MAX_FRAME: usize = 1 << 20;
/// Largest `BYTES` payload kept in memory (negotiation frames, shared text).
pub const MAX_BYTES_PAYLOAD: usize = 64 * 1024;
/// Most `BYTES` payloads being assembled at once.
pub const MAX_OPEN_PAYLOADS: usize = 16;
/// Size of the chunks we send.
pub const CHUNK: usize = 512 * 1024;
/// How often we send a keep-alive, and how long silence may last.
pub const KEEP_ALIVE: Duration = Duration::from_secs(10);
pub const SILENCE: Duration = Duration::from_secs(45);

pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len).await?;
    let n = u32::from_be_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(std::io::Error::other("frame size"));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b).await?;
    Ok(b)
}

pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, b: &[u8]) -> std::io::Result<()> {
    let n = u32::try_from(b.len()).map_err(std::io::Error::other)?;
    w.write_all(&n.to_be_bytes()).await?;
    w.write_all(b).await
}

/// Starts the reader task; the channel closes at the end of input or on a bad frame.
pub fn reader<R: AsyncRead + Unpin + Send + 'static>(mut r: R) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel(4);
    tokio::spawn(async move {
        while let Ok(f) = read_frame(&mut r).await {
            if tx.send(f).await.is_err() {
                break;
            }
        }
    });
    rx
}

/// Waits for the next frame, at most [`SILENCE`].
pub async fn next(rx: &mut mpsc::Receiver<Vec<u8>>) -> Option<Vec<u8>> {
    tokio::time::timeout(SILENCE, rx.recv())
        .await
        .ok()
        .flatten()
}

/// Sends `frame` through the secure channel.
pub async fn send<W: AsyncWrite + Unpin>(
    w: &mut W,
    ch: &mut Channel,
    frame: &[u8],
) -> std::io::Result<()> {
    write_frame(w, &ch.seal(frame)).await
}

/// Sends a sharing frame as a `BYTES` payload: all of it, then an empty last chunk, as Android does.
pub async fn send_share<W: AsyncWrite + Unpin>(
    w: &mut W,
    ch: &mut Channel,
    share: &[u8],
) -> std::io::Result<()> {
    let id = i64::from(rand_u32()) + 1;
    let h = PayloadHeader {
        id,
        kind: 1,
        total_size: i64::try_from(share.len()).unwrap_or(i64::MAX),
    };
    send(w, ch, &msg::payload(&h, 0, share, false)).await?;
    send(
        w,
        ch,
        &msg::payload(&h, i64::try_from(share.len()).unwrap_or(0), &[], true),
    )
    .await
}

fn rand_u32() -> u32 {
    use ring::rand::SecureRandom as _;
    let mut b = [0u8; 4];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .expect("system random");
    u32::from_le_bytes(b) >> 1
}

/// Reassembles `BYTES` payloads (in order, bounded) into whole messages.
#[derive(Debug, Default)]
pub struct Bytes {
    open: HashMap<i64, Vec<u8>>,
}

/// What one payload frame meant.
#[derive(Debug, PartialEq, Eq)]
pub enum Piece {
    /// A whole `BYTES` payload.
    Message(i64, Vec<u8>),
    /// Part of a `BYTES` payload, or a frame we ignore.
    Nothing,
    /// A `FILE` payload frame: for the receiver to handle.
    File,
    /// The peer broke the rules (size, order, too many payloads).
    Bad,
}

impl Bytes {
    /// Feeds one decrypted payload frame.
    pub fn feed(&mut self, frame: &Offline<'_>) -> Piece {
        let Offline::Payload { header, chunk, .. } = frame else {
            return Piece::Nothing;
        };
        if header.kind == 2 {
            return Piece::File;
        }
        if header.kind != 1 {
            return Piece::Nothing;
        }
        let Some(c) = chunk else {
            return Piece::Nothing;
        };
        let total = usize::try_from(header.total_size).unwrap_or(usize::MAX);
        if total > MAX_BYTES_PAYLOAD {
            return Piece::Bad;
        }
        if !self.open.contains_key(&header.id) && self.open.len() >= MAX_OPEN_PAYLOADS {
            return Piece::Bad;
        }
        let buf = self.open.entry(header.id).or_default();
        if usize::try_from(c.offset).ok() != Some(buf.len()) || buf.len() + c.body.len() > total {
            return Piece::Bad;
        }
        buf.extend_from_slice(c.body);
        if c.flags & msg::LAST_CHUNK == 0 {
            return Piece::Nothing;
        }
        let whole = self.open.remove(&header.id).unwrap_or_default();
        if whole.len() == total {
            Piece::Message(header.id, whole)
        } else {
            Piece::Bad
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: i64, total: i64, offset: i64, body: &[u8], last: bool) -> Vec<u8> {
        msg::payload(
            &PayloadHeader {
                id,
                kind: 1,
                total_size: total,
            },
            offset,
            body,
            last,
        )
    }

    #[test]
    fn bytes_payloads() {
        let mut b = Bytes::default();
        let f1 = frame(5, 4, 0, b"ab", false);
        let f2 = frame(5, 4, 2, b"cd", true);
        assert_eq!(b.feed(&msg::parse_offline(&f1).unwrap()), Piece::Nothing);
        assert_eq!(
            b.feed(&msg::parse_offline(&f2).unwrap()),
            Piece::Message(5, b"abcd".to_vec())
        );
        // Out of order, over the announced size, too large.
        for f in [
            frame(6, 4, 2, b"cd", true),
            frame(7, 2, 0, b"abc", true),
            frame(8, 1 << 20, 0, b"a", true),
        ] {
            assert_eq!(b.feed(&msg::parse_offline(&f).unwrap()), Piece::Bad);
        }
        let mut b = Bytes::default();
        for id in 0..16 {
            let f = frame(id, 10, 0, b"a", false);
            assert_eq!(b.feed(&msg::parse_offline(&f).unwrap()), Piece::Nothing);
        }
        let f = frame(99, 10, 0, b"a", false);
        assert_eq!(
            b.feed(&msg::parse_offline(&f).unwrap()),
            Piece::Bad,
            "too many open"
        );
    }

    #[tokio::test]
    async fn frames_are_bounded() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::spawn(async move {
            let _ = a
                .write_all(&(u32::try_from(MAX_FRAME).unwrap() + 1).to_be_bytes())
                .await;
        });
        assert!(read_frame(&mut b).await.is_err());
    }
}

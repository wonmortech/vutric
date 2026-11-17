//! Length-prefixed framing for the control stream: `u32` little-endian length, then a postcard-encoded
//! [`Message`]. File bytes travel on their own stream and are never framed.

use crate::{Error, Message, Result};

/// Hard cap on one control frame. Generous enough for a [`crate::FlingStart`] with a full preview.
pub const MAX_FRAME_BYTES: usize = 128 * 1024;

const HEADER: usize = 4;

pub fn encode(msg: &Message) -> Result<Vec<u8>> {
    let body = postcard::to_stdvec(msg)?;
    if body.len() > MAX_FRAME_BYTES {
        return Err(Error::FrameTooLarge {
            len: body.len(),
            max: MAX_FRAME_BYTES,
        });
    }
    let len = u32::try_from(body.len()).expect("bounded by MAX_FRAME_BYTES");
    let mut out = Vec::with_capacity(HEADER + body.len());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decodes one frame body (without its header) and validates it.
pub fn decode(body: &[u8]) -> Result<Message> {
    let msg: Message = postcard::from_bytes(body)?;
    msg.validate()?;
    Ok(msg)
}

/// Incremental decoder for a byte stream that may deliver frames split or coalesced arbitrarily.
#[derive(Debug, Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Returns the next complete message, `Ok(None)` if more bytes are needed.
    ///
    /// An oversized length prefix is an error before its body is buffered, so a hostile peer cannot make us
    /// allocate more than [`MAX_FRAME_BYTES`].
    pub fn next_message(&mut self) -> Result<Option<Message>> {
        let Some(header) = self.buf.first_chunk::<HEADER>() else {
            return Ok(None);
        };
        let len = u32::from_le_bytes(*header) as usize;
        if len > MAX_FRAME_BYTES {
            return Err(Error::FrameTooLarge {
                len,
                max: MAX_FRAME_BYTES,
            });
        }
        if self.buf.len() < HEADER + len {
            return Ok(None);
        }
        let msg = decode(&self.buf[HEADER..HEADER + len]);
        self.buf.drain(..HEADER + len);
        msg.map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FlingId, Platform};

    fn messages() -> Vec<Message> {
        vec![
            Message::hello("studio-pc", Platform::Windows),
            Message::FlingComplete {
                id: FlingId([1; 16]),
            },
            Message::EffectStarted {
                id: FlingId([2; 16]),
                receiver_micros: 123_456,
            },
        ]
    }

    #[test]
    fn round_trips_byte_by_byte() {
        let wire: Vec<u8> = messages().iter().flat_map(|m| encode(m).unwrap()).collect();
        let mut decoder = Decoder::new();
        let mut out = Vec::new();
        for byte in wire {
            decoder.push(&[byte]);
            while let Some(msg) = decoder.next_message().unwrap() {
                out.push(msg);
            }
        }
        assert_eq!(out, messages());
    }

    #[test]
    fn rejects_oversized_length_before_buffering() {
        let mut decoder = Decoder::new();
        decoder.push(&u32::MAX.to_le_bytes());
        assert!(matches!(
            decoder.next_message(),
            Err(Error::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn garbage_body_is_an_error_not_a_panic() {
        let mut decoder = Decoder::new();
        decoder.push(&3u32.to_le_bytes());
        decoder.push(&[0xff, 0xff, 0xff]);
        assert!(decoder.next_message().is_err());
    }
}

//! Animated QR (backlog R9, ADR-0073): a payload too big for one QR code goes as a loop of QR frames, fountain
//! coded, so the camera can start at any frame and miss any frames. Pure; showing and scanning QR codes is the
//! app's job. Written from the published LT-code idea (Luby, 2002), not from any existing implementation.
//!
//! ```text
//! frame: "VQ" | version 1 | blocks u16 LE | block_bytes u16 LE | total_len u32 LE | seed u32 LE | digest 8 | data
//! ```
//!
//! - Seeds `0..blocks` are the plain blocks (systematic). Every later seed XORs a pseudo-random set of blocks,
//!   with a fixed integer degree table, so every platform derives the same set from the seed. No floating point.
//! - `digest` is the first 8 bytes of the payload's BLAKE3; frames of another payload are refused, and the joined
//!   payload must match it.

use crate::{Error, Result};

pub const MAGIC: [u8; 2] = *b"VQ";
pub const VERSION: u8 = 1;
pub const HEADER_BYTES: usize = 23;
pub const MAX_PAYLOAD_BYTES: usize = 1 << 20;
pub const MAX_BLOCKS: usize = 4096;
pub const MIN_BLOCK_BYTES: usize = 16;
pub const MAX_BLOCK_BYTES: usize = 2048;
/// A frame of 512 + 23 bytes: small enough for a QR code a phone camera reads at arm's length.
pub const SUGGESTED_BLOCK_BYTES: usize = 512;
/// Equations kept while waiting, per block (bounds memory on a long or hostile stream).
pub const MAX_PENDING_PER_BLOCK: usize = 4;

/// Degree table, per mille (sums to 1000): mostly 2 and 3, a few wide ones so the last blocks get covered.
const DEGREES: [(u16, u16); 8] = [
    (1, 50),
    (2, 500),
    (3, 170),
    (4, 80),
    (5, 50),
    (8, 50),
    (16, 50),
    (32, 50),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub blocks: u16,
    pub block_bytes: u16,
    pub total_len: u32,
    pub seed: u32,
    pub digest: [u8; 8],
}

impl Header {
    fn same_payload(&self, o: &Self) -> bool {
        (self.blocks, self.block_bytes, self.total_len, self.digest)
            == (o.blocks, o.block_bytes, o.total_len, o.digest)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub header: Header,
    pub data: Vec<u8>,
}

impl Frame {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let h = &self.header;
        let mut out = Vec::with_capacity(HEADER_BYTES + self.data.len());
        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.extend_from_slice(&h.blocks.to_le_bytes());
        out.extend_from_slice(&h.block_bytes.to_le_bytes());
        out.extend_from_slice(&h.total_len.to_le_bytes());
        out.extend_from_slice(&h.seed.to_le_bytes());
        out.extend_from_slice(&h.digest);
        out.extend_from_slice(&self.data);
        out
    }

    /// A scanned frame, or `None` for anything that is not a valid frame of this version.
    #[must_use]
    pub fn decode(b: &[u8]) -> Option<Self> {
        if b.len() < HEADER_BYTES || b[..2] != MAGIC || b[2] != VERSION {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
        let header = Header {
            blocks: u16_at(3),
            block_bytes: u16_at(5),
            total_len: u32_at(7),
            seed: u32_at(11),
            digest: b[15..23].try_into().ok()?,
        };
        let k = usize::from(header.blocks);
        let bb = usize::from(header.block_bytes);
        let total = usize::try_from(header.total_len).ok()?;
        let shape_ok = (1..=MAX_BLOCKS).contains(&k)
            && (MIN_BLOCK_BYTES..=MAX_BLOCK_BYTES).contains(&bb)
            && (1..=MAX_PAYLOAD_BYTES).contains(&total)
            && total.div_ceil(bb) == k;
        if !shape_ok || b.len() - HEADER_BYTES != bb {
            return None;
        }
        Some(Self {
            header,
            data: b[HEADER_BYTES..].to_vec(),
        })
    }
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The blocks a frame with `seed` XORs together (sorted, distinct), for a payload of `blocks` blocks.
#[must_use]
pub fn indices(seed: u32, blocks: u16) -> Vec<u16> {
    if blocks == 0 {
        return Vec::new();
    }
    if seed < u32::from(blocks) {
        return vec![u16::try_from(seed).expect("below a u16")];
    }
    let mut st = u64::from(seed) ^ (u64::from(blocks) << 32);
    let roll = u16::try_from(splitmix(&mut st) % 1000).expect("below 1000");
    let mut acc = 0u16;
    let mut degree = 1u16;
    for (d, p) in DEGREES {
        acc += p;
        if roll < acc {
            degree = d;
            break;
        }
    }
    let degree = usize::from(degree.min(blocks));
    let mut out: Vec<u16> = Vec::with_capacity(degree);
    while out.len() < degree {
        let i = u16::try_from(splitmix(&mut st) % u64::from(blocks)).expect("below blocks");
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out.sort_unstable();
    out
}

fn xor(a: &mut [u8], b: &[u8]) {
    for (x, y) in a.iter_mut().zip(b) {
        *x ^= y;
    }
}

fn digest(payload: &[u8]) -> [u8; 8] {
    blake3::hash(payload).as_bytes()[..8]
        .try_into()
        .expect("8 bytes")
}

/// Makes frames for one payload; any seed gives a frame, and seeds can loop forever.
#[derive(Debug, Clone)]
pub struct Encoder {
    base: Header,
    blocks: Vec<Vec<u8>>,
}

impl Encoder {
    pub fn new(payload: &[u8], block_bytes: usize) -> Result<Self> {
        let bad = |w: &str| Err(Error::Malformed(format!("qr stream: {w}")));
        if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES {
            return bad("payload size");
        }
        if !(MIN_BLOCK_BYTES..=MAX_BLOCK_BYTES).contains(&block_bytes) {
            return bad("block size");
        }
        let k = payload.len().div_ceil(block_bytes);
        if k > MAX_BLOCKS {
            return bad("too many blocks");
        }
        let blocks = payload
            .chunks(block_bytes)
            .map(|c| {
                let mut v = c.to_vec();
                v.resize(block_bytes, 0);
                v
            })
            .collect();
        Ok(Self {
            base: Header {
                blocks: u16::try_from(k).expect("<= 4096"),
                block_bytes: u16::try_from(block_bytes).expect("<= 2048"),
                total_len: u32::try_from(payload.len()).expect("<= 1 MiB"),
                seed: 0,
                digest: digest(payload),
            },
            blocks,
        })
    }

    #[must_use]
    pub fn blocks(&self) -> u16 {
        self.base.blocks
    }

    /// The frame for `seed`. Show seeds `0, 1, 2, …` in a loop; after the first `blocks()` frames every frame
    /// is a new combination, so a missed frame is replaced by any later one.
    #[must_use]
    pub fn frame(&self, seed: u32) -> Frame {
        let mut data = vec![0u8; usize::from(self.base.block_bytes)];
        for i in indices(seed, self.base.blocks) {
            xor(&mut data, &self.blocks[usize::from(i)]);
        }
        Frame {
            header: Header { seed, ..self.base },
            data,
        }
    }
}

/// Joins scanned frames, in any order and with any gaps or repeats, by peeling.
#[derive(Debug, Default)]
pub struct Decoder {
    header: Option<Header>,
    known: Vec<Option<Vec<u8>>>,
    solved: usize,
    pending: Vec<(Vec<u16>, Vec<u8>)>,
}

impl Decoder {
    /// `(blocks solved, blocks in total)`, for a progress bar (0, 0 before the first frame).
    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        (self.solved, self.known.len())
    }

    fn reduce(&self, idx: &mut Vec<u16>, data: &mut [u8]) {
        idx.retain(|&i| {
            if let Some(b) = &self.known[usize::from(i)] {
                xor(data, b);
                false
            } else {
                true
            }
        });
    }

    fn learn(&mut self, i: u16, data: Vec<u8>) {
        let mut queue = vec![(i, data)];
        while let Some((i, data)) = queue.pop() {
            if self.known[usize::from(i)].is_some() {
                continue;
            }
            self.known[usize::from(i)] = Some(data);
            self.solved += 1;
            let mut rest = Vec::with_capacity(self.pending.len());
            for (mut idx, mut d) in std::mem::take(&mut self.pending) {
                self.reduce(&mut idx, &mut d);
                match idx.len() {
                    0 => {}
                    1 => queue.push((idx[0], d)),
                    _ => rest.push((idx, d)),
                }
            }
            self.pending = rest;
        }
    }

    /// Feeds one frame; returns the payload once every block is solved and the digest matches. A frame of another
    /// payload is an error (the camera moved to another screen: start a new decoder).
    pub fn push(&mut self, frame: &Frame) -> Result<Option<Vec<u8>>> {
        let h = frame.header;
        match self.header {
            None => {
                self.header = Some(h);
                self.known = vec![None; usize::from(h.blocks)];
            }
            Some(mine) if !mine.same_payload(&h) => {
                return Err(Error::Malformed(
                    "qr stream: frame of another payload".into(),
                ));
            }
            Some(_) => {}
        }
        if frame.data.len() != usize::from(h.block_bytes) || h.blocks == 0 {
            return Err(Error::Malformed("qr stream: frame size".into()));
        }
        if self.solved == self.known.len() {
            return Ok(None);
        }
        let mut idx = indices(h.seed, h.blocks);
        let mut data = frame.data.clone();
        self.reduce(&mut idx, &mut data);
        match idx.len() {
            0 => {}
            1 => self.learn(idx[0], data),
            _ => {
                if self.pending.len() < MAX_PENDING_PER_BLOCK * self.known.len() {
                    self.pending.push((idx, data));
                }
            }
        }
        if self.solved < self.known.len() {
            return Ok(None);
        }
        let total = usize::try_from(h.total_len).expect("<= 1 MiB");
        let mut out = Vec::with_capacity(self.known.len() * usize::from(h.block_bytes));
        for b in self.known.iter().flatten() {
            out.extend_from_slice(b);
        }
        out.truncate(total);
        if digest(&out) != h.digest {
            return Err(Error::Malformed("qr stream: digest".into()));
        }
        self.pending.clear();
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| u8::try_from(i * 31 % 251).unwrap())
            .collect()
    }

    /// Plays the loop with `loss_pm` per mille of frames lost, starting at `start`; returns the payload and the
    /// number of frames the camera saw.
    fn play(enc: &Encoder, start: u32, loss_pm: u64, rng: u64) -> (Vec<u8>, u32) {
        let mut st = rng;
        let mut d = Decoder::default();
        let mut seen = 0;
        for seed in start.. {
            if splitmix(&mut st) % 1000 < loss_pm {
                continue;
            }
            seen += 1;
            let wire = enc.frame(seed).encode();
            let f = Frame::decode(&wire).expect("own frame decodes");
            if let Some(p) = d.push(&f).unwrap() {
                return (p, seen);
            }
            assert!(seen < 20 * u32::from(enc.blocks()) + 50, "never finished");
        }
        unreachable!()
    }

    #[test]
    fn whole_loop_without_loss_is_just_the_blocks() {
        let data = payload(5000);
        let enc = Encoder::new(&data, 512).unwrap();
        assert_eq!(enc.blocks(), 10);
        let (got, seen) = play(&enc, 0, 0, 1);
        assert_eq!(got, data);
        assert_eq!(seen, 10);
    }

    #[test]
    fn any_start_and_heavy_loss_still_finish() {
        for (len, bb, start, loss) in [
            (1, 16, 0, 0),
            (100, 16, 3, 300),
            (40_000, 512, 17, 300),
            (40_000, 512, 1000, 500),
            (300_000, 1024, 5, 200),
        ] {
            let data = payload(len);
            let enc = Encoder::new(&data, bb).unwrap();
            let (got, seen) = play(&enc, start, loss, u64::from(start) + 7);
            assert_eq!(got, data, "len {len}");
            assert!(
                seen <= 4 * u32::from(enc.blocks()) + 40,
                "len {len}: {seen} frames for {} blocks",
                enc.blocks()
            );
        }
    }

    #[test]
    fn indices_are_stable_distinct_and_in_range() {
        assert_eq!(indices(3, 10), vec![3]);
        for seed in 10..2000 {
            let i = indices(seed, 10);
            assert!(!i.is_empty() && i.len() <= 10);
            assert!(i.windows(2).all(|w| w[0] < w[1]));
            assert!(i.iter().all(|&x| x < 10));
            assert_eq!(i, indices(seed, 10), "deterministic");
        }
        assert!(indices(5, 0).is_empty());
    }

    #[test]
    fn bad_frames_and_mixed_payloads_are_refused() {
        let a = Encoder::new(&payload(2000), 256).unwrap();
        let b = Encoder::new(&payload(2001), 256).unwrap();
        let mut d = Decoder::default();
        assert_eq!(d.push(&a.frame(0)).unwrap(), None);
        assert!(d.push(&b.frame(1)).is_err(), "another payload");
        let wire = a.frame(2).encode();
        assert!(Frame::decode(&wire[..wire.len() - 1]).is_none());
        let mut bad = wire.clone();
        bad[0] = b'X';
        assert!(Frame::decode(&bad).is_none());
        let mut bad = wire.clone();
        bad[3] = 9; // blocks no longer matches total_len / block_bytes
        assert!(Frame::decode(&bad).is_none());
        let mut bad = wire;
        bad[2] = 2;
        assert!(Frame::decode(&bad).is_none(), "another version");
        assert!(Encoder::new(&[], 256).is_err());
        assert!(Encoder::new(&[1], 8).is_err());
        assert!(Encoder::new(&vec![0; MAX_PAYLOAD_BYTES + 1], 2048).is_err());
    }

    #[test]
    fn a_damaged_frame_fails_the_digest() {
        let data = payload(1000);
        let enc = Encoder::new(&data, 100).unwrap();
        let mut d = Decoder::default();
        let mut res = None;
        for seed in 0..10 {
            let mut f = enc.frame(seed);
            if seed == 4 {
                f.data[0] ^= 1;
            }
            res = Some(d.push(&f));
        }
        assert!(res.unwrap().is_err());
    }
}

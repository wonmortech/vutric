//! Pairing code over sound (backlog R9a, ADR-0074): a few bytes, such as a six-digit code and a short link secret,
//! played by one device and heard by another's microphone, for when the camera cannot see the screen. Pure: PCM
//! samples in and out; playing and recording are the apps'. A textbook 16-tone FSK with Goertzel detection, written
//! from first principles (no code of ggwave or any other modem was read).
//!
//! ```text
//! symbols: preamble 15 0 15 0 | length (2 nibbles) | payload (2 nibbles per byte) | CRC-8 (2 nibbles)
//! symbol: 50 ms of one tone, 1875 Hz + nibble × 62.5 Hz (1875 … 2812.5 Hz), 5 ms raised-cosine edges
//! ```

use core::f32::consts::PI;

pub const MAX_PAYLOAD_BYTES: usize = 32;
pub const TONES: usize = 16;
pub const BASE_HZ: f32 = 1875.0;
pub const SPACING_HZ: f32 = 62.5;
/// Symbols per second (50 ms each).
pub const SYMBOL_RATE: u32 = 20;
pub const MIN_RATE: u32 = 8000;
pub const MAX_RATE: u32 = 96_000;
/// Longest recording the decoder looks at.
pub const MAX_SECONDS: usize = 30;
const PREAMBLE: [u8; 4] = [15, 0, 15, 0];
/// A symbol counts when its tone is this many times stronger than the next one.
const MIN_RATIO: f32 = 2.0;

#[must_use]
pub fn samples_per_symbol(rate: u32) -> usize {
    usize::try_from(rate / SYMBOL_RATE).unwrap_or(0)
}

fn rate_ok(rate: u32) -> bool {
    (MIN_RATE..=MAX_RATE).contains(&rate)
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut c = 0u8;
    for &b in bytes {
        c ^= b;
        for _ in 0..8 {
            c = if c & 0x80 != 0 {
                (c << 1) ^ 0x07
            } else {
                c << 1
            };
        }
    }
    c
}

fn tone_hz(n: u8) -> f32 {
    BASE_HZ + f32::from(n) * SPACING_HZ
}

fn symbols_of(payload: &[u8]) -> Vec<u8> {
    let len = u8::try_from(payload.len()).expect("<= 32");
    let mut framed = vec![len];
    framed.extend_from_slice(payload);
    framed.push(crc8(&framed));
    let mut out = PREAMBLE.to_vec();
    for b in framed {
        out.push(b >> 4);
        out.push(b & 0x0f);
    }
    out
}

/// Mono samples in −0.5 ..= 0.5 at `rate` Hz, or `None` for an empty or too long payload or an odd rate.
#[must_use]
pub fn encode(payload: &[u8], rate: u32) -> Option<Vec<f32>> {
    if payload.is_empty() || payload.len() > MAX_PAYLOAD_BYTES || !rate_ok(rate) {
        return None;
    }
    let sps = samples_per_symbol(rate);
    let fade = sps / 10;
    let syms = symbols_of(payload);
    let mut out = Vec::with_capacity(syms.len() * sps);
    #[allow(clippy::cast_precision_loss)]
    let rate_f = rate as f32;
    for s in syms {
        let w = 2.0 * PI * tone_hz(s) / rate_f;
        for i in 0..sps {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f32;
            let edge = i.min(sps - 1 - i);
            let gain = if edge < fade {
                #[allow(clippy::cast_precision_loss)]
                let x = edge as f32 / fade as f32;
                0.5 - 0.5 * (PI * x).cos()
            } else {
                1.0
            };
            out.push(0.5 * gain * (w * t).sin());
        }
    }
    Some(out)
}

fn power(samples: &[f32], hz: f32, rate: f32) -> f32 {
    let c = 2.0 * (2.0 * PI * hz / rate).cos();
    let (mut s1, mut s2) = (0.0f32, 0.0f32);
    for &x in samples {
        let s0 = x + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    s1 * s1 + s2 * s2 - c * s1 * s2
}

/// The strongest tone in one symbol window and how much stronger it is than the runner-up.
fn symbol_at(samples: &[f32], start: usize, sps: usize, rate: f32) -> Option<(u8, f32)> {
    let win = samples.get(start + sps / 10..start + sps - sps / 10)?;
    let mut best = (0u8, 0.0f32);
    let mut second = 0.0f32;
    for n in 0..16u8 {
        let p = power(win, tone_hz(n), rate);
        if p > best.1 {
            second = best.1;
            best = (n, p);
        } else if p > second {
            second = p;
        }
    }
    if best.1 <= f32::EPSILON {
        return None;
    }
    Some((best.0, best.1 / second.max(f32::MIN_POSITIVE)))
}

fn preamble_score(samples: &[f32], start: usize, sps: usize, rate: f32) -> Option<f32> {
    let mut score = 0.0;
    for (i, want) in PREAMBLE.iter().enumerate() {
        let (got, ratio) = symbol_at(samples, start + i * sps, sps, rate)?;
        if got != *want || ratio < MIN_RATIO {
            return None;
        }
        score += ratio.min(1000.0);
    }
    Some(score)
}

fn read_from(samples: &[f32], start: usize, sps: usize, rate: f32) -> Option<Vec<u8>> {
    let nib = |k: usize| symbol_at(samples, start + k * sps, sps, rate).map(|s| s.0);
    let byte = |k: usize| Some((nib(k)? << 4) | nib(k + 1)?);
    let first = PREAMBLE.len();
    let len = usize::from(byte(first)?);
    if len == 0 || len > MAX_PAYLOAD_BYTES {
        return None;
    }
    let mut framed = vec![u8::try_from(len).ok()?];
    for i in 0..len {
        framed.push(byte(first + 2 + 2 * i)?);
    }
    let crc = byte(first + 2 + 2 * len)?;
    if crc8(&framed) != crc {
        return None;
    }
    Some(framed[1..].to_vec())
}

/// Finds and reads the first valid payload in a recording at `rate` Hz (any gain, any leading silence).
#[must_use]
pub fn decode(samples: &[f32], rate: u32) -> Option<Vec<u8>> {
    if !rate_ok(rate) || samples.len() > MAX_SECONDS * usize::try_from(rate).ok()? {
        return None;
    }
    let sps = samples_per_symbol(rate);
    #[allow(clippy::cast_precision_loss)]
    let rate_f = rate as f32;
    let step = (sps / 8).max(1);
    let mut off = 0;
    while off + PREAMBLE.len() * sps <= samples.len() {
        let quick = symbol_at(samples, off, sps, rate_f);
        if quick.is_some_and(|(n, r)| n == PREAMBLE[0] && r >= MIN_RATIO)
            && preamble_score(samples, off, sps, rate_f).is_some()
        {
            let best = (0..8)
                .map(|j| off + j * step)
                .filter_map(|o| preamble_score(samples, o, sps, rate_f).map(|s| (o, s)))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(off, |(o, _)| o);
            if let Some(p) = read_from(samples, best, sps, rate_f) {
                return Some(p);
            }
        }
        off += step;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(len: usize, amp: f32, seed: u64) -> Vec<f32> {
        let mut s = seed;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                #[allow(clippy::cast_precision_loss)]
                let u = (s >> 40) as f32 / (1u64 << 24) as f32;
                amp * (2.0 * u - 1.0)
            })
            .collect()
    }

    #[test]
    fn round_trip_at_common_rates() {
        for rate in [8000, 16_000, 44_100, 48_000] {
            let msg = b"482913 ab12";
            let pcm = encode(msg, rate).unwrap();
            assert_eq!(
                pcm.len(),
                (4 + 2 + 2 * msg.len() + 2) * samples_per_symbol(rate)
            );
            assert_eq!(decode(&pcm, rate).as_deref(), Some(&msg[..]), "rate {rate}");
        }
    }

    #[test]
    fn heard_after_silence_quietly_and_through_noise() {
        let rate = 48_000;
        let msg = [0u8, 255, 7, 128, 42, 1];
        let pcm = encode(&msg, rate).unwrap();
        for (lead, gain, amp) in [
            (0, 1.0, 0.0),
            (12_345, 0.05, 0.0),
            (777, 1.0, 0.2),
            (31_000, 0.3, 0.05),
        ] {
            let mut rec = noise(lead, amp, 3);
            rec.extend(pcm.iter().map(|x| x * gain));
            let tail = noise(usize::try_from(rate).unwrap() / 2, amp, 4);
            rec.extend(tail);
            let n = noise(rec.len(), amp, 5);
            for (r, e) in rec.iter_mut().zip(n) {
                *r += e;
            }
            assert_eq!(
                decode(&rec, rate).as_deref(),
                Some(&msg[..]),
                "lead {lead} gain {gain} noise {amp}"
            );
        }
    }

    #[test]
    fn nothing_is_heard_in_silence_noise_or_a_broken_frame() {
        let rate = 16_000;
        assert_eq!(decode(&vec![0.0; 32_000], rate), None);
        assert_eq!(decode(&noise(64_000, 0.5, 9), rate), None);
        let mut pcm = encode(b"123456", rate).unwrap();
        let sps = samples_per_symbol(rate);
        let at = (4 + 2 + 3) * sps;
        pcm[at..at + sps].fill(0.0); // one payload symbol lost
        assert_eq!(decode(&pcm, rate), None);
        assert_eq!(encode(&[], rate), None);
        assert_eq!(encode(&[0; MAX_PAYLOAD_BYTES + 1], rate), None);
        assert_eq!(encode(b"x", 4000), None);
        assert_eq!(decode(&pcm, 200_000), None);
    }
}

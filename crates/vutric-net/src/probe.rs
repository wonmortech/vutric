//! Latency probe for spike 0.2, shared by the desktop CLI and the Android app so both measure the same thing.
//!
//! One probe = the sender writes a `FlingStart` carrying a ~30 KB preview, the receiver answers `EffectStarted`
//! the moment it has decoded it. The sender times the round trip; without synchronised clocks the one-way time
//! (release → preview received, the roadmap criterion) is estimated as half of it.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use vutric_core::{EffectId, FlingId, FlingStart, Gesture, Message, Preview, PreviewFormat};

use crate::{Session, micros_since};

/// Preview size the roadmap measures with.
pub const PREVIEW_BYTES: usize = 30 * 1024;

fn fling(seq: u32) -> Message {
    let mut id = [0u8; 16];
    id[..4].copy_from_slice(&seq.to_le_bytes());
    id[4..12].copy_from_slice(
        &u64::from(seq)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .to_le_bytes(),
    );
    Message::FlingStart(FlingStart {
        id: FlingId(id),
        file_name: format!("probe_{seq:04}.jpg"),
        mime: "image/jpeg".into(),
        size: PREVIEW_BYTES as u64,
        blake3: [0; 32],
        gesture: Gesture {
            speed: 1800.0,
            angle: 0.0,
            exit_x: 0.5,
        },
        effect: EffectId::Halqa,
        preview: Some(Preview {
            format: PreviewFormat::Jpeg,
            width: 512,
            height: 384,
            // Incompressible-looking bytes, so no layer can shrink the probe below its real size.
            bytes: (0..PREVIEW_BYTES)
                .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[3] ^ seq as u8)
                .collect(),
        }),
    })
}

/// Sends one probe and waits for the answer; returns the round trip.
pub async fn send_one(session: &mut Session, seq: u32) -> Result<Duration> {
    let msg = fling(seq);
    let Message::FlingStart(ref start) = msg else {
        unreachable!()
    };
    let id = start.id;
    let t0 = Instant::now();
    session.send(&msg).await?;
    match session.recv().await? {
        Message::EffectStarted { id: got, .. } if got == id => Ok(t0.elapsed()),
        other => bail!("expected EffectStarted for this probe, got {other:?}"),
    }
}

/// Answers one incoming probe; returns the preview size that arrived.
pub async fn answer_one(session: &mut Session, clock: Instant) -> Result<usize> {
    match session.recv().await? {
        Message::FlingStart(f) => {
            let len = f.preview.as_ref().map_or(0, |p| p.bytes.len());
            session
                .send(&Message::EffectStarted {
                    id: f.id,
                    receiver_micros: micros_since(clock),
                })
                .await?;
            Ok(len)
        }
        other => bail!("expected FlingStart, got {other:?}"),
    }
}

/// Probes per direction that only warm the path up (QUIC path validation, first allocations); reported apart.
pub const WARMUP: u32 = 3;

/// Sends `WARMUP + runs` probes; returns (warm-up, measured).
pub async fn send_run(session: &mut Session, runs: u32) -> Result<(Stats, Stats)> {
    let (mut warm, mut stats) = (Stats::default(), Stats::default());
    for seq in 0..WARMUP + runs {
        let rtt = send_one(session, seq).await?;
        if seq < WARMUP {
            warm.push(rtt);
        } else {
            stats.push(rtt);
        }
    }
    Ok((warm, stats))
}

/// Answers `WARMUP + runs` probes and checks each preview arrived whole.
pub async fn answer_run(session: &mut Session, runs: u32) -> Result<()> {
    let clock = Instant::now();
    for _ in 0..WARMUP + runs {
        let len = answer_one(session, clock).await?;
        anyhow::ensure!(len == PREVIEW_BYTES, "preview arrived with {len} bytes");
    }
    Ok(())
}

/// Round trips of a run, with the summary the report needs.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub rtts: Vec<Duration>,
}

impl Stats {
    pub fn push(&mut self, rtt: Duration) {
        self.rtts.push(rtt);
    }

    /// Nearest-rank percentile of the round trip, `p` in 0..=100.
    #[must_use]
    pub fn rtt_percentile(&self, p: u32) -> Option<Duration> {
        if self.rtts.is_empty() {
            return None;
        }
        let mut sorted = self.rtts.clone();
        sorted.sort_unstable();
        let rank = (p.min(100) as usize * sorted.len()).div_ceil(100).max(1);
        Some(sorted[rank - 1])
    }

    /// One line for logs and the report: count, RTT p50/p95/max, and the one-way estimates.
    #[must_use]
    pub fn summary(&self) -> String {
        let ms = |d: Option<Duration>| d.map_or(f64::NAN, |d| d.as_secs_f64() * 1000.0);
        let (p50, p95) = (ms(self.rtt_percentile(50)), ms(self.rtt_percentile(95)));
        let max = ms(self.rtts.iter().max().copied());
        format!(
            "n={} rtt p50 {p50:.1} ms, p95 {p95:.1} ms, max {max:.1} ms; one-way ~ p50 {:.1} ms, p95 {:.1} ms",
            self.rtts.len(),
            p50 / 2.0,
            p95 / 2.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_nearest_rank() {
        let mut s = Stats::default();
        for ms in 1..=100 {
            s.push(Duration::from_millis(ms));
        }
        assert_eq!(s.rtt_percentile(50), Some(Duration::from_millis(50)));
        assert_eq!(s.rtt_percentile(95), Some(Duration::from_millis(95)));
        assert_eq!(s.rtt_percentile(0), Some(Duration::from_millis(1)));
        assert!(Stats::default().rtt_percentile(50).is_none());
    }

    #[test]
    fn probe_message_is_valid_and_sized() {
        let msg = fling(7);
        msg.validate().expect("valid");
        let Message::FlingStart(f) = msg else {
            panic!()
        };
        assert_eq!(f.preview.expect("preview").bytes.len(), PREVIEW_BYTES);
    }
}

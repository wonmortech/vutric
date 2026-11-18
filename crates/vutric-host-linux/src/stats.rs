//! Frame pacing for one effect, computed the same way as `apps/overlay-spike` (Phase 0.1 report).

use std::time::Duration;

use crate::proto::FrameStats;

/// A frame interval above this is a visible hitch at 60 Hz (one missed vblank plus margin).
pub const HITCH: Duration = Duration::from_millis(20);

#[derive(Debug, Default, Clone)]
pub struct Recorder {
    pub first: Option<Duration>,
    pub intervals: Vec<Duration>,
}

fn ms(d: Duration) -> f32 {
    (d.as_secs_f64() * 1000.0) as f32
}

impl Recorder {
    fn percentile(sorted: &[Duration], p: f64) -> Duration {
        if sorted.is_empty() {
            return Duration::ZERO;
        }
        sorted[((sorted.len() - 1) as f64 * p).round() as usize]
    }

    #[must_use]
    pub fn summary(&self) -> FrameStats {
        let mut sorted = self.intervals.clone();
        sorted.sort_unstable();
        let total: Duration = sorted.iter().sum();
        FrameStats {
            frames: u32::try_from(self.intervals.len() + usize::from(self.first.is_some()))
                .unwrap_or(u32::MAX),
            fps: if total.is_zero() {
                0.0
            } else {
                (sorted.len() as f64 / total.as_secs_f64()) as f32
            },
            first_ms: self.first.map_or(f32::NAN, ms),
            p50_ms: ms(Self::percentile(&sorted, 0.50)),
            p95_ms: ms(Self::percentile(&sorted, 0.95)),
            p99_ms: ms(Self::percentile(&sorted, 0.99)),
            max_ms: ms(sorted.last().copied().unwrap_or_default()),
            hitches: u32::try_from(sorted.iter().filter(|d| **d > HITCH).count())
                .unwrap_or(u32::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_spike_numbers() {
        let r = Recorder {
            first: Some(Duration::from_millis(4)),
            intervals: (1..=100)
                .map(|i| Duration::from_micros(16_000 + i))
                .chain([Duration::from_millis(40)])
                .collect(),
        };
        let s = r.summary();
        assert_eq!(s.frames, 102);
        assert!((s.fps - 61.0).abs() < 0.5, "{}", s.fps);
        assert!((s.first_ms - 4.0).abs() < 1e-3);
        assert!((s.max_ms - 40.0).abs() < 1e-3);
        assert_eq!(s.hitches, 1);
        assert!(s.p50_ms > 16.0 && s.p50_ms < 16.1);
    }

    #[test]
    fn empty_run_is_zeroes() {
        let s = Recorder::default().summary();
        assert_eq!(s.frames, 0);
        assert_eq!(s.fps, 0.0);
        assert!(s.first_ms.is_nan());
    }
}

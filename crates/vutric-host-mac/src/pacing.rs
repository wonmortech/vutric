//! Frame-pacing numbers for one effect run, the same quantities the Windows spike reports (ADR-0004), plus missed
//! refreshes counted against the display's own rate so 60 Hz and 120 Hz (`ProMotion`) screens are judged alike.

use std::time::Duration;

use serde::Serialize;

use crate::policy::hitch_limit_ms;

/// One run: present-to-present intervals and the focus checks taken at every frame.
#[derive(Debug, Clone)]
pub struct Run {
    refresh_hz: f64,
    first_frame: Option<Duration>,
    intervals: Vec<Duration>,
    focus_kept: bool,
    app_never_active: bool,
    panel_never_key: bool,
}

/// What a run measured, written into the probe's JSON.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RunReport {
    pub frames: usize,
    pub fps: f64,
    pub first_frame_ms: Option<f64>,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
    /// Intervals longer than 1.5 refreshes.
    pub hitches: usize,
    /// Refreshes that passed without a new frame, summed over all hitches.
    pub missed_refreshes: u64,
    /// The app that was frontmost before the run was still frontmost at every frame.
    pub focus_kept: bool,
    /// `NSApp.isActive` stayed false at every frame.
    pub app_never_active: bool,
    /// The overlay window never became the key window.
    pub panel_never_key: bool,
}

impl Run {
    #[must_use]
    pub fn new(refresh_hz: f64) -> Self {
        Self {
            refresh_hz: if refresh_hz > 0.0 { refresh_hz } else { 60.0 },
            first_frame: None,
            intervals: Vec::with_capacity(512),
            focus_kept: true,
            app_never_active: true,
            panel_never_key: true,
        }
    }

    pub fn first_frame(&mut self, since_trigger: Duration) {
        self.first_frame.get_or_insert(since_trigger);
    }

    pub fn interval(&mut self, d: Duration) {
        self.intervals.push(d);
    }

    pub fn focus(&mut self, frontmost_unchanged: bool, app_active: bool, panel_key: bool) {
        self.focus_kept &= frontmost_unchanged;
        self.app_never_active &= !app_active;
        self.panel_never_key &= !panel_key;
    }

    #[must_use]
    pub fn report(&self) -> RunReport {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let mut sorted = self.intervals.clone();
        sorted.sort_unstable();
        let pct = |p: f64| {
            if sorted.is_empty() {
                0.0
            } else {
                ms(sorted[((sorted.len() - 1) as f64 * p).round() as usize])
            }
        };
        let total: Duration = self.intervals.iter().sum();
        let limit = hitch_limit_ms(self.refresh_hz);
        let period = 1000.0 / self.refresh_hz;
        let mut hitches = 0;
        let mut missed = 0u64;
        for d in &self.intervals {
            let v = ms(*d);
            if v > limit {
                hitches += 1;
                missed += ((v / period).round() as u64).saturating_sub(1);
            }
        }
        RunReport {
            frames: self.intervals.len() + usize::from(self.first_frame.is_some()),
            fps: if total.is_zero() {
                0.0
            } else {
                self.intervals.len() as f64 / total.as_secs_f64()
            },
            first_frame_ms: self.first_frame.map(ms),
            p50_ms: pct(0.50),
            p95_ms: pct(0.95),
            p99_ms: pct(0.99),
            max_ms: sorted.last().copied().map_or(0.0, ms),
            hitches,
            missed_refreshes: missed,
            focus_kept: self.focus_kept,
            app_never_active: self.app_never_active,
            panel_never_key: self.panel_never_key,
        }
    }
}

/// The exit criteria of Phase 0.1, scaled to the display: fps within 2 % of the refresh rate, first frame inside
/// one 60 Hz frame (16 ms) of a warm panel, focus never taken.
#[must_use]
pub fn passes(r: &RunReport, refresh_hz: f64) -> bool {
    r.focus_kept
        && r.app_never_active
        && r.panel_never_key
        && r.fps >= refresh_hz * 0.98
        && r.first_frame_ms.is_some_and(|f| f < 16.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(refresh: f64, intervals_us: &[u64]) -> RunReport {
        let mut r = Run::new(refresh);
        r.first_frame(Duration::from_millis(3));
        for us in intervals_us {
            r.interval(Duration::from_micros(*us));
        }
        r.report()
    }

    #[test]
    fn steady_60() {
        let r = run_with(60.0, &[16_667; 120]);
        assert_eq!(r.frames, 121);
        assert!((r.fps - 60.0).abs() < 0.1, "{}", r.fps);
        assert_eq!(r.hitches, 0);
        assert_eq!(r.missed_refreshes, 0);
        assert!(passes(&r, 60.0));
    }

    #[test]
    fn missed_refreshes_are_counted_per_display_rate() {
        // One 33 ms gap = one missed refresh at 60 Hz, three at 120 Hz.
        let at60 = run_with(60.0, &[16_667, 33_333, 16_667]);
        assert_eq!((at60.hitches, at60.missed_refreshes), (1, 1));
        let at120 = run_with(120.0, &[8_333, 33_333, 8_333]);
        assert_eq!((at120.hitches, at120.missed_refreshes), (1, 3));
        assert!(!passes(&at120, 120.0));
    }

    #[test]
    fn promotion_steady_120_passes_but_60_does_not() {
        let r = run_with(120.0, &[8_333; 240]);
        assert!(passes(&r, 120.0));
        let half = run_with(120.0, &[16_667; 120]);
        assert!(!passes(&half, 120.0));
        assert_eq!(
            (half.hitches, half.missed_refreshes),
            (120, 120),
            "a 60 fps run on a 120 Hz screen skips every other refresh"
        );
    }

    #[test]
    fn focus_loss_fails() {
        let mut r = Run::new(60.0);
        r.first_frame(Duration::from_millis(1));
        r.interval(Duration::from_micros(16_667));
        r.focus(true, false, false);
        assert!(passes(&r.report(), 60.0));
        r.focus(true, true, false);
        let rep = r.report();
        assert!(!rep.app_never_active);
        assert!(!passes(&rep, 60.0));
    }

    #[test]
    fn empty_run_is_zero_not_nan() {
        let r = Run::new(0.0).report();
        assert_eq!(r.frames, 0);
        assert_eq!(r.fps, 0.0);
        assert_eq!(r.p99_ms, 0.0);
    }
}

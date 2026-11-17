use std::time::Duration;

/// A frame interval above this is a visible hitch at 60 Hz (one missed vblank plus margin).
const HITCH: Duration = Duration::from_millis(20);
const FIRST_FRAME_BUDGET: Duration = Duration::from_millis(16);

#[derive(Debug)]
pub struct RunStats {
    pub first_frame: Option<Duration>,
    pub intervals: Vec<Duration>,
    /// False if the overlay itself ever became the foreground window: the only real focus theft.
    pub focus_ok: bool,
    pub foreground_before: isize,
    /// The foreground window changed during the run. Informational: the user may have switched apps.
    pub foreground_changed: bool,
}

impl RunStats {
    pub fn new(foreground_before: isize) -> Self {
        Self {
            first_frame: None,
            intervals: Vec::with_capacity(512),
            focus_ok: true,
            foreground_before,
            foreground_changed: false,
        }
    }

    fn percentile(&self, p: f64) -> Duration {
        let mut sorted = self.intervals.clone();
        sorted.sort_unstable();
        if sorted.is_empty() {
            return Duration::ZERO;
        }
        let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
        sorted[idx]
    }

    fn hitches(&self) -> usize {
        self.intervals.iter().filter(|d| **d > HITCH).count()
    }

    fn fps(&self) -> f64 {
        let total: Duration = self.intervals.iter().sum();
        if total.is_zero() {
            0.0
        } else {
            self.intervals.len() as f64 / total.as_secs_f64()
        }
    }
}

#[derive(Debug, Default)]
pub struct Summary {
    pub adapter: String,
    pub runs: Vec<RunStats>,
}

impl Summary {
    pub fn passed(&self) -> bool {
        !self.runs.is_empty()
            && self.runs.iter().all(|r| {
                r.focus_ok
                    && r.fps() >= 59.0
                    && r.first_frame.is_some_and(|d| d < FIRST_FRAME_BUDGET)
            })
    }

    pub fn print(&self) {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!("\nVutric overlay spike — {}", self.adapter);
        println!(
            "run  frames   fps   first(ms)  p50(ms)  p95(ms)  p99(ms)  max(ms)  hitches  focus"
        );
        for (i, r) in self.runs.iter().enumerate() {
            let max = r.intervals.iter().max().copied().unwrap_or_default();
            println!(
                "{:>3}  {:>6}  {:>5.1}  {:>9.2}  {:>7.2}  {:>7.2}  {:>7.2}  {:>7.2}  {:>7}  {}",
                i + 1,
                r.intervals.len() + 1,
                r.fps(),
                r.first_frame.map_or(f64::NAN, ms),
                ms(r.percentile(0.50)),
                ms(r.percentile(0.95)),
                ms(r.percentile(0.99)),
                ms(max),
                r.hitches(),
                match (r.focus_ok, r.foreground_changed) {
                    (false, _) => "TAKEN",
                    (true, true) => "ok (user switched apps)",
                    (true, false) => "ok",
                },
            );
        }
        println!(
            "exit criteria (≥59 fps, first frame < 16 ms, focus never taken): {}",
            if self.passed() { "PASS" } else { "FAIL" }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_and_fps() {
        let mut r = RunStats::new(0);
        r.intervals = (1..=100)
            .map(|i| Duration::from_micros(16_000 + i))
            .collect();
        assert!((r.fps() - 62.4).abs() < 0.2, "{}", r.fps());
        assert_eq!(r.percentile(0.0), Duration::from_micros(16_001));
        assert_eq!(r.percentile(1.0), Duration::from_micros(16_100));
        assert_eq!(r.hitches(), 0);
    }
}

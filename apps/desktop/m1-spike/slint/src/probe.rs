//! The M1 measurement contract (see ../README.md), shared in spirit by all three spikes:
//! `VUTRIC_M1_LOG=<file>` receives JSON lines: `first_frame` (Unix ms) and, after the window closes or
//! `VUTRIC_M1_FRAMES` frames, `frames` with the frame-interval percentiles measured from presented frames.

use std::cell::RefCell;
use std::io::Write as _;
use std::rc::Rc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Default)]
pub struct Probe(Rc<RefCell<Inner>>);

#[derive(Default)]
struct Inner {
    log: Option<std::path::PathBuf>,
    started: bool,
    first: Option<Instant>,
    last: Option<Instant>,
    intervals: Vec<f64>,
    want: usize,
    written: bool,
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

impl Probe {
    pub fn from_env() -> Self {
        let log = std::env::var_os("VUTRIC_M1_LOG").map(Into::into);
        let want = std::env::var("VUTRIC_M1_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);
        Self(Rc::new(RefCell::new(Inner {
            log,
            want,
            ..Inner::default()
        })))
    }

    fn write(path: &std::path::Path, line: &str) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }

    pub fn frame(&self) {
        let mut s = self.0.borrow_mut();
        let now = Instant::now();
        if !s.started {
            s.started = true;
            s.first = Some(now);
            if let Some(p) = s.log.clone() {
                Self::write(
                    &p,
                    &format!(r#"{{"toolkit":"slint","event":"first_frame","unix_ms":{}}}"#, unix_ms()),
                );
            }
        }
        // Skip the first second: window creation and shader warm-up are measured by first_frame instead.
        let warm = s.first.is_some_and(|f| now.duration_since(f).as_secs_f64() > 1.0);
        if warm {
            if let Some(last) = s.last {
                let dt = now.duration_since(last).as_secs_f64() * 1000.0;
                s.intervals.push(dt);
            }
        }
        s.last = Some(now);
        if s.intervals.len() >= s.want && !s.written {
            drop(s);
            self.finish();
        }
    }

    pub fn finish(&self) {
        let mut s = self.0.borrow_mut();
        if s.written {
            return;
        }
        s.written = true;
        let Some(p) = s.log.clone() else { return };
        let mut v = s.intervals.clone();
        if v.is_empty() {
            return;
        }
        v.sort_by(f64::total_cmp);
        let pct = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
        let mean = v.iter().sum::<f64>() / v.len() as f64;
        Self::write(
            &p,
            &format!(
                r#"{{"toolkit":"slint","event":"frames","count":{},"fps":{:.1},"p50_ms":{:.2},"p99_ms":{:.2},"max_ms":{:.2}}}"#,
                v.len(),
                1000.0 / mean,
                pct(0.5),
                pct(0.99),
                v[v.len() - 1]
            ),
        );
    }
}

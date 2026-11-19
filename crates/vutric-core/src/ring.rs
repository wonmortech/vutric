//! "Ring my phone" (backlog F14, ADR-0069): [`Message::Ring`] makes a paired device play a loud sound until the
//! user finds it, the sender stops it, or [`Ring::Start::secs`] runs out. Pure: the host passes the admission and
//! the clock.
//!
//! - Taken only from a paired device (`Admission::Trusted`); a guest can never make a phone ring.
//! - [`RingLimit`] bounds how often one peer may start a ring; a stop always passes.
//! - The ringing device sends [`Ring::Stop`] back when its user stops it, so the other screen can say "found".

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Message;
use crate::trust::Admission;

/// Longest ring one message may ask for, in seconds. The ringing device stops by itself after it.
pub const MAX_RING_SECS: u16 = 60;

/// What the hosts ask for when the user does not choose.
pub const DEFAULT_RING_SECS: u16 = 30;

/// The payload of [`Message::Ring`].
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ring {
    /// Ring for `secs` seconds (1 ..= [`MAX_RING_SECS`]), loud, and on silent where the platform allows it.
    Start { secs: u16 },
    /// Either direction: stop ringing (sender: "never mind"; ringing device: "found it").
    Stop,
}

impl Ring {
    /// `Start` with `secs` clamped into 1 ..= [`MAX_RING_SECS`].
    #[must_use]
    pub fn start(secs: u16) -> Self {
        Self::Start {
            secs: secs.clamp(1, MAX_RING_SECS),
        }
    }

    /// Whether the fields are in range (called by `Message::validate`).
    #[must_use]
    pub fn is_valid(self) -> bool {
        match self {
            Self::Start { secs } => (1..=MAX_RING_SECS).contains(&secs),
            Self::Stop => true,
        }
    }

    /// How long the ringing device rings, `None` for a stop.
    #[must_use]
    pub fn duration(self) -> Option<Duration> {
        match self {
            Self::Start { secs } => Some(Duration::from_secs(u64::from(secs))),
            Self::Stop => None,
        }
    }
}

/// Whether a host should take `msg` from a peer with `admission`. `false` for anything that is not a ring.
#[must_use]
pub fn accepts(msg: &Message, admission: Admission) -> bool {
    matches!(msg, Message::Ring(_)) && admission == Admission::Trusted
}

/// At most [`RingLimit::BURST`] starts, then one per [`RingLimit::EVERY`]; a start over the limit is dropped.
/// [`Ring::Stop`] always passes. Keep one per peer.
#[derive(Clone, Debug)]
pub struct RingLimit {
    tokens: f64,
    last: Option<Instant>,
}

impl Default for RingLimit {
    fn default() -> Self {
        Self {
            tokens: f64::from(Self::BURST),
            last: None,
        }
    }
}

impl RingLimit {
    pub const BURST: u32 = 3;
    pub const EVERY: Duration = Duration::from_secs(20);

    /// Whether `ring`, arriving at `now`, may be carried out.
    pub fn allow(&mut self, ring: Ring, now: Instant) -> bool {
        if ring == Ring::Stop {
            return true;
        }
        if let Some(last) = self.last {
            let earned =
                now.saturating_duration_since(last).as_secs_f64() / Self::EVERY.as_secs_f64();
            self.tokens = (self.tokens + earned).min(f64::from(Self::BURST));
        }
        self.last = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_is_clamped_and_checked() {
        assert_eq!(Ring::start(0), Ring::Start { secs: 1 });
        assert_eq!(
            Ring::start(500),
            Ring::Start {
                secs: MAX_RING_SECS
            }
        );
        assert!(Ring::start(DEFAULT_RING_SECS).is_valid());
        assert!(!Ring::Start { secs: 0 }.is_valid());
        assert!(
            !Ring::Start {
                secs: MAX_RING_SECS + 1
            }
            .is_valid()
        );
        assert!(Ring::Stop.is_valid());
        assert_eq!(Ring::start(30).duration(), Some(Duration::from_secs(30)));
        assert_eq!(Ring::Stop.duration(), None);
    }

    #[test]
    fn only_paired_devices_ring() {
        let r = Message::Ring(Ring::start(5));
        assert!(accepts(&r, Admission::Trusted));
        assert!(!accepts(&r, Admission::Unknown));
        assert!(!accepts(&r, Admission::Refuse));
        assert!(!accepts(
            &Message::Clipboard { text: "x".into() },
            Admission::Trusted
        ));
    }

    #[test]
    fn limit_burst_then_one_per_window_and_stop_always_passes() {
        let t0 = Instant::now();
        let mut l = RingLimit::default();
        let go = Ring::start(10);
        let first = (0..10).filter(|_| l.allow(go, t0)).count();
        assert_eq!(first, RingLimit::BURST as usize);
        assert!(l.allow(Ring::Stop, t0));
        assert!(!l.allow(go, t0 + Duration::from_secs(5)));
        assert!(l.allow(go, t0 + Duration::from_secs(26)));
        assert!(!l.allow(go, t0 + Duration::from_secs(27)));
        let later = t0 + Duration::from_secs(3600);
        let refill = (0..10).filter(|_| l.allow(go, later)).count();
        assert_eq!(refill, RingLimit::BURST as usize);
    }
}

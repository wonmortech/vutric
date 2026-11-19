//! Phone as mouse and keyboard (backlog F15, ADR-0075): [`Message::Pointer`] carries relative moves, scrolls,
//! buttons, keys (USB HID usage ids) and short text from a paired phone to a desktop. Pure: the host injects the
//! input (W2) after these checks.
//!
//! - Taken only from a paired device (`Admission::Trusted`), and only while the desktop user turned it on.
//! - Every field is bounded ([`Pointer::is_valid`]); text has no control characters but tab and newline.
//! - [`PointerLimit`] caps the rate per peer. A release (button up, key up, [`Pointer::ReleaseAll`]) always
//!   passes, so nothing stays held because a press was dropped.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::Message;
use crate::trust::Admission;

/// Largest move or scroll in one message, in the sender's units (pixels for moves, 1/120 notch for scrolls).
pub const MAX_STEP: i16 = 2000;

/// Most bytes of [`Pointer::Text`].
pub const MAX_TEXT_BYTES: usize = 256;

/// Lowest and highest HID keyboard usage id taken (page 0x07: `a` = 0x04 … right GUI = 0xE7).
pub const MIN_KEY_USAGE: u16 = 0x04;
pub const MAX_KEY_USAGE: u16 = 0xE7;

#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Button {
    Left,
    Right,
    Middle,
    Back,
    Forward,
}

/// The payload of [`Message::Pointer`].
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pointer {
    /// Relative pointer move.
    Move {
        dx: i16,
        dy: i16,
    },
    /// Wheel or two-finger scroll, 120 = one notch.
    Scroll {
        dx: i16,
        dy: i16,
    },
    Button {
        button: Button,
        down: bool,
    },
    /// One key by its HID usage id (layout independent; modifiers are 0xE0 … 0xE7).
    Key {
        usage: u16,
        down: bool,
    },
    /// Text typed as text (the host types it in the current layout, an IME may be skipped).
    Text {
        text: String,
    },
    /// Let go of every held button and key (sent when the phone stops; the host also does it when the session ends).
    ReleaseAll,
}

impl Pointer {
    /// Whether every field is in range (called by `Message::validate`).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let step = |v: i16| v.unsigned_abs() <= MAX_STEP.unsigned_abs();
        match self {
            Self::Move { dx, dy } | Self::Scroll { dx, dy } => step(*dx) && step(*dy),
            Self::Key { usage, .. } => (MIN_KEY_USAGE..=MAX_KEY_USAGE).contains(usage),
            Self::Text { text } => {
                !text.is_empty()
                    && text.len() <= MAX_TEXT_BYTES
                    && !text
                        .chars()
                        .any(|c| c.is_control() && c != '\n' && c != '\t')
            }
            Self::Button { .. } | Self::ReleaseAll => true,
        }
    }

    /// Whether this lets something go (always carried out, never rate limited).
    #[must_use]
    pub fn is_release(&self) -> bool {
        matches!(
            self,
            Self::Button { down: false, .. } | Self::Key { down: false, .. } | Self::ReleaseAll
        )
    }

    /// Folds a later move or scroll into this one when the host is behind; `false` if they cannot be merged.
    pub fn coalesce(&mut self, next: &Self) -> bool {
        let add = |a: i16, b: i16| a.saturating_add(b).clamp(-MAX_STEP, MAX_STEP);
        match (self, next) {
            (Self::Move { dx, dy }, Self::Move { dx: x, dy: y })
            | (Self::Scroll { dx, dy }, Self::Scroll { dx: x, dy: y }) => {
                *dx = add(*dx, *x);
                *dy = add(*dy, *y);
                true
            }
            _ => false,
        }
    }
}

/// Whether a host should take `msg` from a peer with `admission`. `false` for anything that is not pointer input.
/// The host checks its own "Telefondan boshqarish" switch as well.
#[must_use]
pub fn accepts(msg: &Message, admission: Admission) -> bool {
    matches!(msg, Message::Pointer(_)) && admission == Admission::Trusted
}

/// A token bucket per peer: [`PointerLimit::RATE`] events per second, bursts up to [`PointerLimit::BURST`];
/// text costs [`PointerLimit::TEXT_COST`]. Releases always pass.
#[derive(Clone, Debug)]
pub struct PointerLimit {
    tokens: f64,
    last: Option<Instant>,
}

impl Default for PointerLimit {
    fn default() -> Self {
        Self {
            tokens: f64::from(Self::BURST),
            last: None,
        }
    }
}

impl PointerLimit {
    pub const RATE: u32 = 250;
    pub const BURST: u32 = 500;
    pub const TEXT_COST: u32 = 25;

    /// Whether `p`, arriving at `now`, may be injected.
    pub fn allow(&mut self, p: &Pointer, now: Instant) -> bool {
        if p.is_release() {
            return true;
        }
        if let Some(last) = self.last {
            let earned = now.saturating_duration_since(last).as_secs_f64() * f64::from(Self::RATE);
            self.tokens = (self.tokens + earned).min(f64::from(Self::BURST));
        }
        self.last = Some(now);
        let cost = if matches!(p, Pointer::Text { .. }) {
            f64::from(Self::TEXT_COST)
        } else {
            1.0
        };
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    /// Time until `cost` tokens are back, for a host that would rather wait than drop.
    #[must_use]
    pub fn refill_after(&self, cost: u32) -> Duration {
        let missing = (f64::from(cost) - self.tokens).max(0.0);
        Duration::from_secs_f64(missing / f64::from(Self::RATE))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_bounded() {
        assert!(
            Pointer::Move {
                dx: 2000,
                dy: -2000
            }
            .is_valid()
        );
        assert!(!Pointer::Move { dx: 2001, dy: 0 }.is_valid());
        assert!(
            !Pointer::Scroll {
                dx: 0,
                dy: i16::MIN
            }
            .is_valid()
        );
        assert!(
            Pointer::Key {
                usage: 0x04,
                down: true
            }
            .is_valid()
        );
        assert!(
            Pointer::Key {
                usage: 0xE7,
                down: false
            }
            .is_valid()
        );
        assert!(
            !Pointer::Key {
                usage: 0x03,
                down: true
            }
            .is_valid()
        );
        assert!(
            !Pointer::Key {
                usage: 0xE8,
                down: true
            }
            .is_valid()
        );
        assert!(
            Pointer::Text {
                text: "Salom,\tdunyo\n".into()
            }
            .is_valid()
        );
        assert!(
            !Pointer::Text {
                text: String::new()
            }
            .is_valid()
        );
        assert!(
            !Pointer::Text {
                text: "a\u{1b}[2J".into()
            }
            .is_valid(),
            "no escape sequences"
        );
        assert!(
            !Pointer::Text {
                text: "x".repeat(MAX_TEXT_BYTES + 1)
            }
            .is_valid()
        );
        assert!(Pointer::ReleaseAll.is_valid());
    }

    #[test]
    fn moves_and_scrolls_coalesce_and_clamp() {
        let mut m = Pointer::Move { dx: 1500, dy: -3 };
        assert!(m.coalesce(&Pointer::Move { dx: 1500, dy: -4 }));
        assert_eq!(m, Pointer::Move { dx: 2000, dy: -7 });
        assert!(!m.coalesce(&Pointer::Scroll { dx: 0, dy: 120 }));
        let mut s = Pointer::Scroll { dx: 0, dy: 120 };
        assert!(s.coalesce(&Pointer::Scroll { dx: 0, dy: 120 }));
        assert_eq!(s, Pointer::Scroll { dx: 0, dy: 240 });
        let mut r = Pointer::ReleaseAll;
        assert!(!r.coalesce(&Pointer::ReleaseAll));
    }

    #[test]
    fn only_trusted_devices_and_releases_always_pass() {
        let msg = Message::Pointer(Pointer::Move { dx: 1, dy: 1 });
        assert!(accepts(&msg, Admission::Trusted));
        assert!(!accepts(&msg, Admission::Unknown));
        assert!(!accepts(&msg, Admission::Refuse));
        assert!(!accepts(
            &Message::Ring(crate::ring::Ring::Stop),
            Admission::Trusted
        ));

        let t0 = Instant::now();
        let mut l = PointerLimit::default();
        let mv = Pointer::Move { dx: 1, dy: 0 };
        let passed = (0..1000).filter(|_| l.allow(&mv, t0)).count();
        assert_eq!(passed, PointerLimit::BURST as usize);
        assert!(
            l.allow(
                &Pointer::Key {
                    usage: 0x04,
                    down: false
                },
                t0
            ),
            "key up passes"
        );
        assert!(l.allow(&Pointer::ReleaseAll, t0));
        assert!(!l.allow(&mv, t0));
        assert!(l.refill_after(1) > Duration::ZERO);
        assert!(
            l.allow(&mv, t0 + Duration::from_millis(10)),
            "2.5 tokens back after 10 ms"
        );
        let mut l = PointerLimit::default();
        let text = Pointer::Text { text: "a".into() };
        let texts = (0..100).filter(|_| l.allow(&text, t0)).count();
        assert_eq!(
            texts,
            (PointerLimit::BURST / PointerLimit::TEXT_COST) as usize
        );
    }
}

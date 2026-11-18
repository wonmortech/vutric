//! Rules for the companion messages, [`Message::Notice`] and [`Message::Clipboard`] (ADR-0064, backlog A12, F12,
//! F13). Pure: the host passes the admission and the clock.
//!
//! - Both are accepted only from a paired device (`Admission::Trusted`); from anyone else they are dropped
//!   unread, like an offer would never be auto-accepted from a stranger.
//! - A notice is shown, never acted on: no buttons, no links opened, no reply.
//! - Clipboard text goes into a prompt ("Nusxalash"); the host never writes the clipboard on its own.
//! - Text is shown only after [`shown`]: no control characters, no bidi overrides, bounded length.
//! - Notices are rate-limited ([`NoticeLimit`]), so a chatty phone cannot flood the desktop.

use std::time::{Duration, Instant};

use crate::Message;
use crate::trust::Admission;

/// Whether a host should take `msg` from a peer with `admission`. `false` for anything that is not a companion
/// message.
#[must_use]
pub fn accepts(msg: &Message, admission: Admission) -> bool {
    matches!(msg, Message::Notice(_) | Message::Clipboard { .. }) && admission == Admission::Trusted
}

/// `raw` made safe to show: control characters (except `\n` when `newlines` is true) and bidi overrides removed,
/// at most `max_chars` characters, trimmed. A notice line uses `newlines = false`.
#[must_use]
pub fn shown(raw: &str, max_chars: usize, newlines: bool) -> String {
    raw.chars()
        .filter(|&c| {
            (newlines && c == '\n')
                || !(c.is_control()
                    || matches!(c, '\u{200E}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'))
        })
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// At most [`NoticeLimit::BURST`] notices, then one per [`NoticeLimit::EVERY`]; the rest are dropped.
#[derive(Clone, Debug)]
pub struct NoticeLimit {
    tokens: f64,
    last: Option<Instant>,
}

impl Default for NoticeLimit {
    fn default() -> Self {
        Self {
            tokens: f64::from(Self::BURST),
            last: None,
        }
    }
}

impl NoticeLimit {
    pub const BURST: u32 = 10;
    pub const EVERY: Duration = Duration::from_millis(500);

    /// Whether a notice arriving at `now` may be shown.
    pub fn allow(&mut self, now: Instant) -> bool {
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
    use crate::message::Notice;

    #[test]
    fn only_paired_devices_and_only_companion_messages() {
        let n = Message::Notice(Notice {
            app: "a".into(),
            title: String::new(),
            text: String::new(),
            at_ms: 0,
        });
        let c = Message::Clipboard { text: "x".into() };
        assert!(accepts(&n, Admission::Trusted));
        assert!(accepts(&c, Admission::Trusted));
        assert!(!accepts(&n, Admission::Unknown));
        assert!(!accepts(&c, Admission::Unknown));
        assert!(!accepts(
            &Message::PairResult { confirmed: true },
            Admission::Trusted
        ));
    }

    #[test]
    fn shown_text_is_clean_and_short() {
        assert_eq!(shown("  a\u{202E}b\u{0007}c\nd  ", 80, false), "abcd");
        assert_eq!(shown("a\nb\r\n", 80, true), "a\nb");
        assert_eq!(shown("ʻʻʻʻ", 2, false), "ʻʻ");
        assert_eq!(shown("\u{2066}\u{0000}", 10, true), "");
    }

    #[test]
    fn burst_then_steady_rate() {
        let t0 = Instant::now();
        let mut l = NoticeLimit::default();
        let first: usize = (0..20).filter(|_| l.allow(t0)).count();
        assert_eq!(first, NoticeLimit::BURST as usize);
        assert!(!l.allow(t0 + Duration::from_millis(100)));
        assert!(l.allow(t0 + Duration::from_millis(600)));
        assert!(!l.allow(t0 + Duration::from_millis(700)));
        let later = t0 + Duration::from_secs(60);
        let refill: usize = (0..20).filter(|_| l.allow(later)).count();
        assert_eq!(refill, NoticeLimit::BURST as usize);
    }
}
